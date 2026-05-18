# Windows Integration Prompt Part 3

Status: archived implementation prompt reference.

This file is a split continuation of `PROMPT_WINDOWS_INTEGRATION.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.

            persist.Load(PCWSTR(lnk_wide.as_ptr()), STGM(0)).ok()?;

            let mut buf = [0u16; 1024];
            shell_link
                .GetPath(&mut buf, std::ptr::null_mut(), 0)
                .ok()?;

            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        })();

        CoUninitialize();
        result
    }
}

/// Checks the Start Menu shortcut and creates it if missing.
///
/// Returns the check result so the caller can decide whether to show a dialog
/// for mismatched shortcuts.
#[cfg(windows)]
pub fn check_and_create_shortcut() -> ShortcutCheckResult {
    let lnk_path = match shortcut_path() {
        Some(p) => p,
        None => return ShortcutCheckResult::Failed("Could not determine APPDATA path".into()),
    };

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return ShortcutCheckResult::Failed(format!("current_exe failed: {e}")),
    };

    if !lnk_path.exists() {
        return match create_lnk(&lnk_path, &current_exe) {
            Ok(()) => ShortcutCheckResult::Created,
            Err(e) => ShortcutCheckResult::Failed(e),
        };
    }

    // Shortcut exists — check where it points.
    match read_lnk_target(&lnk_path) {
        Some(target) => {
            let target_path = PathBuf::from(&target);
            if target_path == current_exe {
                ShortcutCheckResult::AlreadyCorrect
            } else {
                ShortcutCheckResult::Mismatched(target)
            }
        }
        None => ShortcutCheckResult::Failed("Could not read existing shortcut target".into()),
    }
}

/// Overwrites the existing Start Menu shortcut to point to the current binary.
#[cfg(windows)]
pub fn update_shortcut() -> Result<(), String> {
    let lnk_path = shortcut_path()
        .ok_or_else(|| "Could not determine APPDATA path".to_string())?;
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("current_exe failed: {e}"))?;
    create_lnk(&lnk_path, &current_exe)
}

// Non-Windows stubs — these features are Windows-only but the crate must compile cross-platform.
#[cfg(not(windows))]
pub fn check_and_create_shortcut() -> ShortcutCheckResult {
    ShortcutCheckResult::AlreadyCorrect
}

#[cfg(not(windows))]
pub fn update_shortcut() -> Result<(), String> {
    Ok(())
}
```

### App Integration for the Shortcut Dialog

During app initialization, after creating the main window:

```
match check_and_create_shortcut() {
    Created        => log info "Start Menu shortcut created"
    AlreadyCorrect => do nothing
    Mismatched(old_path) => {
        if config.start_menu_shortcut_declined { do nothing }
        else { store old_path in app state, open a dialog/prompt }
    }
    Failed(e) => log warning
}
```

**Dialog mechanism:** Implement the shortcut mismatch prompt using your GUI framework's native dialog or window system. If your framework supports multiple windows, open a small secondary window. If not, a simple Win32 `MessageBoxW` with Yes/No buttons is acceptable. The dialog must present:
- A heading or title: "Start Menu Shortcut"
- Body text: "Your Start Menu shortcut points to a different location. Update it to the current version?"
- The old path and the current exe path for context
- Two actions: "Update Shortcut" (calls `update_shortcut()`, closes dialog) and "No, don't ask again" (sets `start_menu_shortcut_declined = true` in config, saves config persistently, closes dialog)

---

## Feature 3: System Tray Icon with Context Menu

### What It Does

Spawns a background thread that owns a system tray icon (notification area icon) with a right-click context menu. The menu includes a checkmark-togglable "Start with Windows" item that controls the `autostart` config field, plus a "Quit" item. Communication between the main app thread and the tray thread uses `std::sync::mpsc` channels.

### Architecture

```
Main App Thread                          Tray Background Thread
     |                                          |
     |-- TrayCommand (mpsc::Sender) ----------->|  (rebuild menu, set icon, quit)
     |                                          |
     |<---- MenuEvent (global receiver) --------|  (user clicked a menu item)
     |                                          |
     |-- Arc<Mutex<TrayMenuIds>> (shared) ----->|  (maps MenuId -> action)
```

### Implementation Requirements

Create a module (e.g., `tray.rs`):

<requirements>
1. **Tray icon:** A solid-color 32x32 RGBA icon. Generate it programmatically — no external icon file required. Use `tray_icon::Icon::from_rgba(rgba_vec, 32, 32)`.

2. **Context menu construction** via a `build_tray_menu(autostart: bool)` function that returns `(Menu, TrayMenuIds)`. The menu includes:
   - A checkmark-toggled "Start with Windows" item: prefix with `"✓ "` when autostart is enabled, `"  "` (two spaces) when disabled. The `MenuItem` is always enabled (clickable).
   - A separator.
   - A "Quit" item.
   - Optionally, add other application-specific menu items between the autostart toggle and quit if your app needs them.

3. **`TrayMenuIds` struct:** Holds the `MenuId` for each menu item so the main thread can match incoming `MenuEvent` IDs to actions. Shared via `Arc<Mutex<TrayMenuIds>>`. The struct is initialized with placeholder `MenuId`s (e.g., `MenuId::new("__placeholder__")`) before the tray thread starts, then replaced with real IDs once the thread builds its menu.

4. **Race condition note:** The main thread must gracefully handle the case where `TrayMenuIds` still contains placeholder values (i.e., the tray thread has not yet initialized the menu). When polling `MenuEvent::receiver()`, if the locked IDs contain placeholders, skip dispatch for that tick.

5. **`TrayCommand` enum:** Sent from the main thread to the tray thread:
   - `Rebuild { autostart: bool }` — tears down and rebuilds the menu with fresh state. Add additional fields if your app has other state to reflect in the menu.
   - `Quit` — posts `WM_QUIT` to exit the message loop.

6. **`spawn_tray_thread(autostart: bool)` function:** Spawns the background thread. Returns `(mpsc::Sender<TrayCommand>, Arc<Mutex<TrayMenuIds>>)`. The thread:
   - Builds the initial menu (must happen ON the tray thread — `Menu` is `!Send` due to internal `Rc`).
   - Publishes the real `TrayMenuIds` into the shared `Arc<Mutex<>>` after building.
   - Creates the tray icon via `TrayIconBuilder::new().with_menu(...).with_tooltip(...).with_icon(...).build()`.
   - Runs a Win32 `PeekMessageW` polling loop (50ms sleep between iterations).
   - Drains `TrayCommand`s from the mpsc receiver on each loop iteration.
   - On `Rebuild`: calls `tray.set_menu(Some(Box::new(new_menu)))` and `tray.set_tooltip(...)`, then updates the shared IDs.
   - On `Quit`: calls `PostQuitMessage(0)` to exit the loop.
   - After the loop exits, drops the tray icon.

7. **Main thread polling:** The main thread polls `MenuEvent::receiver()` on every tick/frame. When a `MenuEvent` arrives:
   - Lock the `TrayMenuIds` mutex.
   - If the IDs are still placeholders, skip processing.
   - Compare the event's `MenuId` against the stored IDs.
   - If it matches the autostart toggle: flip `config.autostart`, call `sync_autostart(config.autostart)`, save the config persistently, then send a `TrayCommand::Rebuild` so the tray menu checkmark updates.
   - If it matches Quit: initiate app shutdown.

8. **Expose the global receiver** via a public function:
   ```rust
   pub fn menu_event_receiver() -> &'static tray_icon::menu::MenuEventReceiver {
       MenuEvent::receiver()
   }
