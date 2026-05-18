# Windows Integration Prompt Part 4

Status: archived implementation prompt reference.

This file is a split continuation of `PROMPT_WINDOWS_INTEGRATION.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.

   ```
</requirements>

### Example: Tray Menu Build Function

```rust
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    TrayIconBuilder,
};

// CUSTOMIZE: Add fields for any additional menu items your app needs.
pub struct TrayMenuIds {
    pub autostart_toggle: MenuId,
    pub quit: MenuId,
}

fn build_tray_menu(autostart: bool) -> (Menu, TrayMenuIds) {
    let autostart_label = if autostart {
        "✓ Start with Windows"
    } else {
        "  Start with Windows"
    };
    let autostart_item = MenuItem::new(autostart_label, true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let ids = TrayMenuIds {
        autostart_toggle: autostart_item.id().clone(),
        quit: quit_item.id().clone(),
    };

    let menu = Menu::new();
    menu.append(&autostart_item).ok();
    menu.append(&PredefinedMenuItem::separator()).ok();
    menu.append(&quit_item).ok();

    (menu, ids)
}
```

### Example: Icon Generation

```rust
fn create_icon() -> tray_icon::Icon {
    let side = 32u32;
    // CUSTOMIZE: Choose your app's tray icon color (RGBA).
    let color: [u8; 4] = [0x60, 0x60, 0x60, 0xFF]; // Gray
    let mut rgba = Vec::with_capacity((side * side * 4) as usize);
    for _ in 0..(side * side) {
        rgba.extend_from_slice(&color);
    }
    tray_icon::Icon::from_rgba(rgba, side, side).expect("Failed to create tray icon")
}
```

### Example: Win32 Message Loop (Tray Thread)

```rust
#[cfg(windows)]
{
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };

    let mut msg = MSG::default();
    'outer: loop {
        // Drain Win32 messages.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == 0x0012 { break 'outer; } // WM_QUIT
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Handle commands from the main thread.
        while let Ok(cmd) = rx.try_recv() {
            match cmd {
                TrayCommand::Rebuild { autostart } => {
                    let (new_menu, new_ids) = build_tray_menu(autostart);
                    tray.set_menu(Some(Box::new(new_menu)));
                    // CUSTOMIZE: Update tooltip with current app state.
                    tray.set_tooltip(Some("MyApp")).ok();
                    if let Ok(mut ids) = ids_arc.lock() { *ids = new_ids; }
                }
                TrayCommand::Quit => {
                    unsafe { windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0); }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    drop(tray);
}
```

### Example: Spawn Function Skeleton

```rust
use std::sync::{mpsc, Arc, Mutex};

pub fn spawn_tray_thread(
    autostart: bool,
) -> (mpsc::Sender<TrayCommand>, Arc<Mutex<TrayMenuIds>>) {
    let placeholder_ids = TrayMenuIds {
        autostart_toggle: MenuId::new("__placeholder__"),
        quit: MenuId::new("__placeholder__"),
    };
    let ids_shared = Arc::new(Mutex::new(placeholder_ids));
    let ids_for_thread = Arc::clone(&ids_shared);

    let (tx, rx) = mpsc::channel::<TrayCommand>();

    std::thread::spawn(move || {
        // Build menu on tray thread (Menu is !Send).
        let (initial_menu, initial_ids) = build_tray_menu(autostart);

        // Publish real IDs.
        if let Ok(mut ids) = ids_for_thread.lock() {
            *ids = initial_ids;
        }

        let icon = create_icon();
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(initial_menu))
            .with_tooltip("MyApp") // CUSTOMIZE: Your app name/tooltip
            .with_icon(icon)
            .build()
            .expect("Failed to create tray icon");

        // ... Win32 message loop (see example above) ...

        drop(tray);
    });

    (tx, ids_shared)
}
```

---

## Integration Checklist (App Initialization)

Execute these steps during application startup, in this order:

```
1. Load config from the existing config file (existing mechanism).
2. Call autostart::sync_autostart(config.autostart).
3. Call tray::spawn_tray_thread(config.autostart) → store the Sender and Arc<Mutex<TrayMenuIds>>.
4. Call shortcut::check_and_create_shortcut():
   - If Created → log info "Start Menu shortcut created".
   - If AlreadyCorrect → do nothing.
   - If Mismatched(old_path) and !config.start_menu_shortcut_declined → show dialog.
   - If Failed(e) → log warning.
5. Enter the main event loop.
6. On every tick/frame, poll tray::menu_event_receiver().try_recv():
   - Lock TrayMenuIds; skip if IDs are still placeholders.
   - Match MenuId to autostart_toggle → flip config.autostart, call sync_autostart, save config, send TrayCommand::Rebuild.
