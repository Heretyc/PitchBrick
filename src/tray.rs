//! System tray icon and context menu for PitchBrick.
//!
//! Spawns a background thread that owns the tray icon and runs a Win32
//! message loop. Communicates with the iced main thread via an mpsc channel
//! for outbound commands (rebuild menu, quit) and via the global
//! `MenuEvent::receiver()` for inbound menu click events.

use crate::config::Gender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu},
    TrayIconBuilder,
};

/// The pitch display state to reflect in the tray icon color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    /// Voice is in target range (green).
    Green,
    /// Voice is in the wrong direction (red).
    Red,
    /// Not speaking or no voice detected (gray).
    Inactive,
}

/// Commands sent from the iced main thread to the tray thread.
pub enum TrayCommand {
    /// Rebuild the menu and tooltip with updated state.
    Rebuild {
        gender: Gender,
        input_devices: Vec<String>,
        output_devices: Vec<String>,
        selected_input: String,
        selected_output: String,
    },
    /// Update the tray icon color to reflect the current pitch state.
    SetState(TrayState),
    /// Shut down the tray icon and exit the message loop.
    Quit,
}

/// Stores the `MenuId` for each menu item so that iced can map incoming
/// `MenuEvent` IDs to the correct `Message` variant.
pub struct TrayMenuIds {
    pub gender_toggle: MenuId,
    pub open_config: MenuId,
    pub patreon: MenuId,
    pub quit: MenuId,
    /// `(menu_id, device_name)` pairs for input devices.
    pub input_devices: Vec<(MenuId, String)>,
    /// `(menu_id, device_name)` pairs for output devices.
    pub output_devices: Vec<(MenuId, String)>,
}

/// Constructs a fresh native context menu reflecting the given state.
///
/// Returns the `Menu` to attach to the tray icon and a `TrayMenuIds` mapping
/// each item's `MenuId` to the corresponding action.
fn build_tray_menu(
    gender: Gender,
    input_devices: &[String],
    output_devices: &[String],
    selected_input: &str,
    selected_output: &str,
) -> (Menu, TrayMenuIds) {
    let gender_item = MenuItem::new(format!("Target: {}", gender), true, None);
    let open_config_item = MenuItem::new("Open Config", true, None);

    let input_submenu = Submenu::new("Input Device", true);
    let mut input_ids = Vec::new();
    for dev in input_devices {
        let is_selected = dev == selected_input;
        let label = if is_selected {
            format!("✓ {}", dev)
        } else {
            format!("  {}", dev)
        };
        let item = MenuItem::new(label, true, None);
        input_ids.push((item.id().clone(), dev.clone()));
        input_submenu.append(&item).ok();
    }

    let output_submenu = Submenu::new("Output Device", true);
    let mut output_ids = Vec::new();
    for dev in output_devices {
        let is_selected = dev == selected_output;
        let label = if is_selected {
            format!("✓ {}", dev)
        } else {
            format!("  {}", dev)
        };
        let item = MenuItem::new(label, true, None);
        output_ids.push((item.id().clone(), dev.clone()));
        output_submenu.append(&item).ok();
    }

    let patreon_item = MenuItem::new("Written by Lexi", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let ids = TrayMenuIds {
        gender_toggle: gender_item.id().clone(),
        open_config: open_config_item.id().clone(),
        patreon: patreon_item.id().clone(),
        quit: quit_item.id().clone(),
        input_devices: input_ids,
        output_devices: output_ids,
    };

    let menu = Menu::new();
    menu.append(&gender_item).ok();
    menu.append(&open_config_item).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&input_submenu).ok();
    menu.append(&output_submenu).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&patreon_item).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&quit_item).ok();

    (menu, ids)
}

/// Creates a 32×32 solid-color tray icon for the given state.
fn create_icon(state: TrayState) -> tray_icon::Icon {
    let side = 32u32;
    let color: [u8; 4] = match state {
        TrayState::Green    => [0x4C, 0xAF, 0x50, 0xFF],
        TrayState::Red      => [0xF4, 0x43, 0x36, 0xFF],
        TrayState::Inactive => [0x60, 0x60, 0x60, 0xFF],
    };
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..(side * side) {
        rgba.extend_from_slice(&color);
    }
    tray_icon::Icon::from_rgba(rgba, side, side).expect("Failed to create tray icon")
}

/// Spawns the tray icon background thread and returns a command sender and
/// a shared reference to the current menu IDs.
///
/// The tray thread owns the `TrayIcon` and runs a Win32 `PeekMessage` loop.
/// The iced main thread polls `MenuEvent::receiver()` on every tick to handle
/// menu clicks, and sends `TrayCommand`s when state changes require a menu rebuild.
pub fn spawn_tray_thread(
    gender: Gender,
    input_devices: Vec<String>,
    output_devices: Vec<String>,
    selected_input: String,
    selected_output: String,
) -> (std::sync::mpsc::Sender<TrayCommand>, Arc<Mutex<TrayMenuIds>>) {
    // ids_shared is populated by the thread once it builds the menu.
    // We pre-fill with a placeholder so the Arc exists before the thread starts.
    let placeholder_ids = TrayMenuIds {
        gender_toggle: MenuId::new("__placeholder__"),
        open_config: MenuId::new("__placeholder__"),
        patreon: MenuId::new("__placeholder__"),
        quit: MenuId::new("__placeholder__"),
        input_devices: Vec::new(),
        output_devices: Vec::new(),
    };
    let ids_shared = Arc::new(Mutex::new(placeholder_ids));
    let ids_for_thread = Arc::clone(&ids_shared);

    let (tx, rx) = std::sync::mpsc::channel::<TrayCommand>();

    std::thread::spawn(move || {
        // Build menu inside the thread — muda::Menu is !Send due to Rc internals.
        let (initial_menu, initial_ids) = build_tray_menu(
            gender,
            &input_devices,
            &output_devices,
            &selected_input,
            &selected_output,
        );
        let tooltip = format!("PitchBrick - Target: {}", gender);

        // Publish the real IDs now that we're on the tray thread.
        if let Ok(mut ids) = ids_for_thread.lock() {
            *ids = initial_ids;
        }

        let icon = create_icon(TrayState::Inactive);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(initial_menu))
            .with_tooltip(&tooltip)
            .with_icon(icon)
            .build()
            .expect("Failed to create tray icon");

        // Win32 polling message loop.
        #[cfg(windows)]
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
            };

            let mut msg = MSG::default();
            'outer: loop {
                // Drain all pending Win32 messages.
                unsafe {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        if msg.message == 0x0012 {
                            // WM_QUIT
                            break 'outer;
                        }
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                // Handle commands from the iced thread.
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        TrayCommand::Rebuild {
                            gender,
                            input_devices,
                            output_devices,
                            selected_input,
                            selected_output,
                        } => {
                            let (new_menu, new_ids) = build_tray_menu(
                                gender,
                                &input_devices,
                                &output_devices,
                                &selected_input,
                                &selected_output,
                            );
                            tray.set_menu(Some(Box::new(new_menu)));
                            let tooltip = format!("PitchBrick - Target: {}", gender);
                            tray.set_tooltip(Some(&tooltip)).ok();
                            if let Ok(mut ids) = ids_for_thread.lock() {
                                *ids = new_ids;
                            }
                        }
                        TrayCommand::SetState(state) => {
                            tray.set_icon(Some(create_icon(state))).ok();
                        }
                        TrayCommand::Quit => {
                            unsafe {
                                windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
                            }
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(50));
            }
        }

        // Non-Windows stub (keeps the thread alive but does nothing).
        #[cfg(not(windows))]
        {
            loop {
                match rx.recv() {
                    Ok(TrayCommand::Quit) | Err(_) => break,
                    Ok(TrayCommand::SetState(state)) => {
                        tray.set_icon(Some(create_icon(state))).ok();
                    }
                    _ => {}
                }
            }
        }

        drop(tray);
    });

    (tx, ids_shared)
}

/// Returns the global `MenuEvent` receiver so the iced main thread can poll it.
pub fn menu_event_receiver() -> &'static tray_icon::menu::MenuEventReceiver {
    MenuEvent::receiver()
}
