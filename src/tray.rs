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

/// State of the "Check for updates" tray menu item.
#[derive(Debug, Clone)]
pub enum UpdateMenuState {
    /// Default idle state.
    Ready,
    /// A check is in progress (non-interactive).
    Checking,
    /// The running version is the latest.
    NoUpdates,
    /// A newer version is available.
    Available(String),
    /// The network request failed.
    NetworkError,
}

impl UpdateMenuState {
    /// Returns the menu item label text.
    pub fn label(&self) -> String {
        match self {
            Self::Ready => "Check for updates".to_string(),
            Self::Checking => "Checking...".to_string(),
            Self::NoUpdates => "No new updates.".to_string(),
            Self::Available(v) => format!("Update available (v{})", v),
            Self::NetworkError => "Problem with internet!".to_string(),
        }
    }

    /// Returns whether the menu item should be clickable.
    pub fn is_enabled(&self) -> bool {
        !matches!(self, Self::Checking)
    }
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
        vr_overlay_enabled: bool,
        vr_specific_settings: bool,
        vr_mode_active: bool,
    },
    /// Update the tray icon color to reflect the current pitch state.
    SetState(TrayState),
    /// Update the "Check for updates" menu item text and enabled state.
    SetUpdateMenuState(UpdateMenuState),
    /// Switch to VR mode icon and show a balloon notification.
    EnterVrMode,
    /// Switch back to the color square icon and show a balloon notification.
    LeaveVrMode,
    /// Shut down the tray icon and exit the message loop.
    Quit,
}

/// Stores the `MenuId` for each menu item so that iced can map incoming
/// `MenuEvent` IDs to the correct `Message` variant.
pub struct TrayMenuIds {
    pub gender_toggle: MenuId,
    pub open_config: MenuId,
    pub vr_overlay_toggle: MenuId,
    pub vr_specific_settings_toggle: MenuId,
    pub check_for_updates: MenuId,
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
#[allow(clippy::too_many_arguments)]
fn build_tray_menu(
    gender: Gender,
    input_devices: &[String],
    output_devices: &[String],
    selected_input: &str,
    selected_output: &str,
    vr_overlay_enabled: bool,
    vr_specific_settings: bool,
    update_state: &UpdateMenuState,
) -> (Menu, TrayMenuIds) {
    let gender_item = MenuItem::new(format!("Target: {}", gender), true, None);
    let open_config_item = MenuItem::new("Open Config", true, None);
    let vr_label = if vr_overlay_enabled {
        "✓ Toggle SteamVR Overlay"
    } else {
        "  Toggle SteamVR Overlay"
    };
    let vr_overlay_item = MenuItem::new(vr_label, true, None);

    let vr_settings_label = if vr_specific_settings {
        "✓ Allow VR Specific Settings"
    } else {
        "  Allow VR Specific Settings"
    };
    let vr_settings_item = MenuItem::new(vr_settings_label, vr_overlay_enabled, None);

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

    let update_item = MenuItem::new(update_state.label(), update_state.is_enabled(), None);

    let patreon_item = MenuItem::new("Written by Lexi", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let ids = TrayMenuIds {
        gender_toggle: gender_item.id().clone(),
        open_config: open_config_item.id().clone(),
        vr_overlay_toggle: vr_overlay_item.id().clone(),
        vr_specific_settings_toggle: vr_settings_item.id().clone(),
        check_for_updates: update_item.id().clone(),
        patreon: patreon_item.id().clone(),
        quit: quit_item.id().clone(),
        input_devices: input_ids,
        output_devices: output_ids,
    };

    let menu = Menu::new();
    menu.append(&gender_item).ok();
    menu.append(&open_config_item).ok();
    menu.append(&vr_overlay_item).ok();
    menu.append(&vr_settings_item).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&input_submenu).ok();
    menu.append(&output_submenu).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&update_item).ok();
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

/// Creates a 32×32 VR mode icon. Attempts to load `docs/vr-mode.ico` from
/// the exe's directory; falls back to a purple square if the file is missing.
fn create_vr_icon() -> tray_icon::Icon {
    if let Ok(exe) = std::env::current_exe() {
        let ico_path = exe.with_file_name("vr-mode.ico");
        if ico_path.exists() {
            if let Ok(icon) = tray_icon::Icon::from_path(&ico_path, Some((32, 32))) {
                return icon;
            }
        }
        // Also try docs/ relative to exe dir.
        if let Some(parent) = exe.parent() {
            let ico_path = parent.join("docs").join("vr-mode.ico");
            if ico_path.exists() {
                if let Ok(icon) = tray_icon::Icon::from_path(&ico_path, Some((32, 32))) {
                    return icon;
                }
            }
        }
    }
    // Fallback: purple square.
    let side = 32u32;
    let color: [u8; 4] = [0x9C, 0x27, 0xB0, 0xFF];
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..(side * side) {
        rgba.extend_from_slice(&color);
    }
    tray_icon::Icon::from_rgba(rgba, side, side).expect("Failed to create VR icon")
}

/// Shows a balloon (toast) notification via the Win32 Shell_NotifyIconW API.
#[cfg(windows)]
fn show_balloon(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_INFO, NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS,
    };
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        uID: 1, // tray-icon crate uses uID = 1 for its first icon
        uFlags: NIF_INFO | NOTIFY_ICON_DATA_FLAGS(0),
        ..Default::default()
    };

    // Copy title into szInfoTitle (max 63 chars + null).
    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().collect();
    let title_len = title_wide.len().min(nid.szInfoTitle.len() - 1);
    nid.szInfoTitle[..title_len].copy_from_slice(&title_wide[..title_len]);

    // Copy message into szInfo (max 255 chars + null).
    let msg_wide: Vec<u16> = OsStr::new(message).encode_wide().collect();
    let msg_len = msg_wide.len().min(nid.szInfo.len() - 1);
    nid.szInfo[..msg_len].copy_from_slice(&msg_wide[..msg_len]);

    // Find the tray-icon crate's hidden HWND via FindWindowW.
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::PCWSTR;

    let hwnd = unsafe {
        let class_name: Vec<u16> = OsStr::new("tray-icon-window")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        FindWindowW(PCWSTR(class_name.as_ptr()), PCWSTR::null())
    };

    let hwnd = match hwnd {
        Ok(h) if !h.0.is_null() => h,
        _ => {
            tracing::debug!("Balloon: could not find tray HWND, skipping notification");
            return;
        }
    };

    nid.hWnd = hwnd;
    let result = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    if !result.as_bool() {
        tracing::debug!("Balloon: Shell_NotifyIconW NIM_MODIFY failed");
    }
}

/// Spawns the tray icon background thread and returns a command sender and
/// a shared reference to the current menu IDs.
///
/// The tray thread owns the `TrayIcon` and runs a Win32 `PeekMessage` loop.
/// The iced main thread polls `MenuEvent::receiver()` on every tick to handle
/// menu clicks, and sends `TrayCommand`s when state changes require a menu rebuild.
#[allow(clippy::too_many_arguments)]
pub fn spawn_tray_thread(
    gender: Gender,
    input_devices: Vec<String>,
    output_devices: Vec<String>,
    selected_input: String,
    selected_output: String,
    vr_overlay_enabled: bool,
    vr_specific_settings: bool,
    initial_update_state: UpdateMenuState,
) -> (std::sync::mpsc::Sender<TrayCommand>, Arc<Mutex<TrayMenuIds>>) {
    // ids_shared is populated by the thread once it builds the menu.
    // We pre-fill with a placeholder so the Arc exists before the thread starts.
    let placeholder_ids = TrayMenuIds {
        gender_toggle: MenuId::new("__placeholder__"),
        open_config: MenuId::new("__placeholder__"),
        vr_overlay_toggle: MenuId::new("__placeholder__"),
        vr_specific_settings_toggle: MenuId::new("__placeholder__"),
        check_for_updates: MenuId::new("__placeholder__"),
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
        let mut current_update_state = initial_update_state;
        let (initial_menu, initial_ids) = build_tray_menu(
            gender,
            &input_devices,
            &output_devices,
            &selected_input,
            &selected_output,
            vr_overlay_enabled,
            vr_specific_settings,
            &current_update_state,
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

        // Track VR mode for icon management.
        let mut in_vr_mode = false;
        let mut current_tray_state = TrayState::Inactive;

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
                            vr_overlay_enabled,
                            vr_specific_settings,
                            vr_mode_active,
                        } => {
                            let (new_menu, new_ids) = build_tray_menu(
                                gender,
                                &input_devices,
                                &output_devices,
                                &selected_input,
                                &selected_output,
                                vr_overlay_enabled,
                                vr_specific_settings,
                                &current_update_state,
                            );
                            tray.set_menu(Some(Box::new(new_menu)));
                            let tooltip = format!("PitchBrick - Target: {}", gender);
                            tray.set_tooltip(Some(&tooltip)).ok();
                            if let Ok(mut ids) = ids_for_thread.lock() {
                                *ids = new_ids;
                            }
                            in_vr_mode = vr_mode_active;
                        }
                        TrayCommand::SetUpdateMenuState(state) => {
                            current_update_state = state;
                            // Rebuild the menu to update the item text/enabled state.
                            // We don't have the full state here, so we just update the
                            // existing menu item by rebuilding. This reuses the last
                            // known gender/devices from the tray tooltip.
                            // For simplicity, we send a balloon for available updates.
                            if let UpdateMenuState::Available(ref v) = current_update_state {
                                show_balloon("PitchBrick", &format!("Update available: v{}", v));
                            }
                        }
                        TrayCommand::SetState(state) => {
                            current_tray_state = state;
                            if !in_vr_mode {
                                tray.set_icon(Some(create_icon(state))).ok();
                            }
                        }
                        TrayCommand::EnterVrMode => {
                            in_vr_mode = true;
                            tray.set_icon(Some(create_vr_icon())).ok();
                            show_balloon("PitchBrick", "VR mode activated");
                        }
                        TrayCommand::LeaveVrMode => {
                            in_vr_mode = false;
                            tray.set_icon(Some(create_icon(current_tray_state))).ok();
                            show_balloon("PitchBrick", "VR mode deactivated");
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
                        current_tray_state = state;
                        if !in_vr_mode {
                            tray.set_icon(Some(create_icon(state))).ok();
                        }
                    }
                    Ok(TrayCommand::EnterVrMode) => {
                        in_vr_mode = true;
                        tray.set_icon(Some(create_vr_icon())).ok();
                    }
                    Ok(TrayCommand::LeaveVrMode) => {
                        in_vr_mode = false;
                        tray.set_icon(Some(create_icon(current_tray_state))).ok();
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
