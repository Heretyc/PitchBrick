# Prompt: Implement Windows Auto-Start, Start Menu Shortcut, and System Tray Integration

You are a senior Rust systems programmer implementing Windows desktop integration features for an existing Rust GUI application. The application already has a working config system that persists settings to a config file. You will add three tightly coupled features: **Windows auto-start via the registry**, **Start Menu shortcut management via COM**, and **system tray (notification area) with a context menu that includes an autostart toggle**.

**IMPORTANT — Placeholder Replacement:** Throughout this prompt, `"MyApp"` and similar placeholder names (e.g., in `VALUE_NAME`, shortcut filenames, tooltip text) must be replaced with your actual application name. Do not leave `"MyApp"` in production code.

**IMPORTANT — API Version:** All `windows` crate code examples target v0.58. The v0.58 API uses `HSTRING` for wide strings, returns `WIN32_ERROR` from registry functions, and uses `windows::core::Interface` for COM interface casting. Do not use patterns from `windows-rs` versions prior to v0.50.

---

## Context and Constraints

<context>
- **Language:** Rust (stable toolchain, Windows x86_64 target)
- **Platform:** Windows 10/11 only. Use `#[cfg(windows)]` guards on all platform-specific code, with no-op stubs for `#[cfg(not(windows))]` so the crate compiles cross-platform.
- **Config persistence:** The application already has an existing config struct (serde-serializable) and a save function that writes it atomically to a config file. When this prompt says "save the config," call your existing save mechanism. Do NOT create a new config system. The config changes are saved persistently to the existing configured config file.
- **Config fields to add:** `autostart: bool` (default `true`) and `start_menu_shortcut_declined: bool` (default `false`). Both are persisted to the config file.
- **Dependencies (add to `Cargo.toml` under `[target.'cfg(windows)'.dependencies]`):**
  - `windows = { version = "0.58", features = ["Win32_UI_WindowsAndMessaging", "Win32_System_Registry", "Win32_UI_Shell", "Win32_System_Com", "Win32_Storage_FileSystem", "Win32_Foundation"] }`
  - `tray-icon = "0.21"`
  - `dirs = "6"` (under `[dependencies]`, platform-independent)
- **No admin rights required.** All registry and filesystem operations use per-user locations (`HKCU`, `%APPDATA%`).
- **Error handling philosophy:** These features are best-effort. Log warnings on failure, never panic or propagate errors to callers. The app must always start even if registry access or COM initialization fails.
- **Implementation order:** Implement the three modules in this order: (1) `autostart.rs`, (2) `shortcut.rs`, (3) `tray.rs`. Then wire them together in the app initialization. This ordering matches dependency order: the tray menu depends on autostart, and the app init depends on all three.
</context>

---

## Feature 1: Windows Auto-Start (Registry Run Key)

### What It Does

Syncs the Windows `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run` registry key with the user's `autostart` config preference on every application launch, and whenever the user toggles the setting from the tray menu.

### Implementation Requirements

Create a module (e.g., `autostart.rs`) with a single public function:

```rust
/// Ensures the Windows autostart registry entry matches `enabled`.
///
/// If `enabled` is true and the entry is missing or stale, it is written.
/// If `enabled` is false and the entry exists, it is removed.
/// Errors are logged but never propagated — autostart is best-effort.
pub fn sync_autostart(enabled: bool)
```

<requirements>
1. **Registry path:** `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`
2. **Value name:** Use your application's name as the registry value name. This must be a `const &str` at module level (e.g., `const VALUE_NAME: &str = "MyApp";`). Replace `"MyApp"` with your actual app name.
3. **Value data:** The full path to the current running executable, obtained via `std::env::current_exe()`. Encoded as null-terminated UTF-16 (`REG_SZ`).
4. **When `enabled = true`:** Open the Run key with `KEY_SET_VALUE`, write the value via `RegSetValueExW`. If the key can't be opened or the write fails, log a warning and return.
5. **When `enabled = false`:** Open the Run key with `KEY_SET_VALUE`, delete the value via `RegDeleteValueW`. If the value is already absent (`ERROR_FILE_NOT_FOUND`), that's fine — log debug and return. If the key itself doesn't exist, return silently.
6. **Call `sync_autostart(config.autostart)` once during app initialization**, so the registry always reflects the config even if the user moved the binary.
7. **Call `sync_autostart(config.autostart)` again whenever the user toggles autostart** from the tray menu, after flipping and saving the config value.
8. **Non-Windows stub:** The `#[cfg(not(windows))]` branch should be `{ let _ = enabled; }` (a no-op).
</requirements>

### Example Implementation (Complete Module)

```rust
// autostart.rs
/// Windows autostart (Run registry key) management.
///
/// Syncs `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\{AppName}`
/// on every launch to match the user's `config.autostart` preference.
/// Uses HKCU so no admin rights are required.

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "MyApp"; // CUSTOMIZE: Replace with your app name

/// Ensures the Windows autostart registry entry matches `enabled`.
///
/// If `enabled` is true and the entry is missing or stale, it is written.
/// If `enabled` is false and the entry exists, it is removed.
/// Errors are logged but never propagated — autostart is best-effort.
pub fn sync_autostart(enabled: bool) {
    #[cfg(windows)]
    {
        if enabled { set_autostart(); } else { remove_autostart(); }
    }
    #[cfg(not(windows))]
    { let _ = enabled; }
}

#[cfg(windows)]
fn set_autostart() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => { tracing::warn!("autostart: could not get exe path: {e}"); return; }
    };

    let exe_str = exe.to_string_lossy();
    tracing::debug!("autostart: setting Run key to {exe_str}");

    let mut key = HKEY::default();
    let subkey = HSTRING::from(RUN_KEY);
    let status = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, 0, KEY_SET_VALUE, &mut key)
    };
    if status.is_err() {
        tracing::warn!("autostart: could not open Run key: {:?}", status);
        return;
    }

    // Encode as null-terminated UTF-16 for REG_SZ.
    let value_name = HSTRING::from(VALUE_NAME);
    let mut data: Vec<u16> = OsStr::new(exe_str.as_ref())
        .encode_wide().collect();
    data.push(0);
    let byte_len = (data.len() * 2) as u32;

    let status = unsafe {
        RegSetValueExW(key, &value_name, 0, REG_SZ,
            Some(std::slice::from_raw_parts(data.as_ptr().cast(), byte_len as usize)))
    };
    let _ = unsafe { RegCloseKey(key) };

    if status.is_err() {
        tracing::warn!("autostart: RegSetValueExW failed: {:?}", status);
    } else {
        tracing::debug!("autostart: Run key set successfully");
    }
}

#[cfg(windows)]
fn remove_autostart() {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;

    tracing::debug!("autostart: removing Run key entry");

    let mut key = HKEY::default();
    let subkey = HSTRING::from(RUN_KEY);
    let status = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, 0, KEY_SET_VALUE, &mut key)
    };
    if status.is_err() { return; } // Key doesn't exist — nothing to remove.

    let value_name = HSTRING::from(VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key, &value_name) };
    let _ = unsafe { RegCloseKey(key) };

    // ERROR_FILE_NOT_FOUND (2) means the value was already absent — fine.
    if status.is_err() && status != windows::Win32::Foundation::ERROR_FILE_NOT_FOUND.into() {
        tracing::warn!("autostart: RegDeleteValueW failed: {:?}", status);
    } else {
        tracing::debug!("autostart: Run key entry removed (or was already absent)");
    }
}
```

---

## Feature 2: Start Menu Shortcut Management (COM Shell Link)

### What It Does

On every launch, checks whether a `.lnk` shortcut exists in the user's Start Menu Programs folder. If missing, creates it silently. If it exists but points to a different binary (e.g., the user installed a new version in a different location), the app shows a dialog asking the user whether to update it. The user can accept (shortcut is overwritten) or decline with "don't ask again" (sets `start_menu_shortcut_declined = true` in config, which is then saved persistently to the existing configured config file).

### Implementation Requirements

Create a module (e.g., `shortcut.rs`) with these public items:

<requirements>
1. **Shortcut location:** `%APPDATA%\Microsoft\Windows\Start Menu\Programs\{AppName}.lnk` — resolve `%APPDATA%` via `std::env::var("APPDATA")`.
2. **COM lifecycle:** Always call `CoInitializeEx(None, COINIT_APARTMENTTHREADED)` before COM operations and `CoUninitialize()` after, using a closure-based pattern that ensures `CoUninitialize` is called on every exit path (see example below). Do NOT use `CoInitialize` (the non-Ex variant). Do NOT forget to call `CoUninitialize` on any exit path.
3. **`check_and_create_shortcut() -> ShortcutCheckResult`:** The main entry point, called once during app init. Returns an enum:
   - `Created` — shortcut didn't exist, was just created.
   - `AlreadyCorrect` — shortcut exists and points to the current binary.
   - `Mismatched(String)` — shortcut exists but points to a different path (the old path is returned).
   - `Failed(String)` — something went wrong (error message returned).
4. **Non-Windows behavior:** On non-Windows platforms, `check_and_create_shortcut()` should return `ShortcutCheckResult::AlreadyCorrect` (a no-op). Gate the entire implementation with `#[cfg(windows)]` and provide a non-Windows stub.
5. **`update_shortcut() -> Result<(), String>`:** Overwrites the existing shortcut to point to the current binary. Called when the user accepts the mismatch dialog.
6. **Shortcut properties to set:** target path (`SetPath`), description (`SetDescription` — a short app description string), and icon location (`SetIconLocation` — point to the exe itself, icon index 0).
7. **Reading existing shortcuts:** Use `IPersistFile::Load` + `IShellLinkW::GetPath` with a 1024-char wide buffer to read the current target from an existing `.lnk`.
8. **Wide string helper:** Create a private `fn to_wide(s: &str) -> Vec<u16>` that encodes as null-terminated UTF-16.
</requirements>

### COM RAII Pattern

The COM lifecycle must be safe against early returns. Use this closure pattern:

```rust
unsafe {
    CoInitializeEx(None, COINIT_APARTMENTTHREADED)
        .ok()
        .map_err(|e| format!("CoInitializeEx failed: {e}"))?;

    let result = (|| -> Result<(), String> {
        // ... all COM operations here ...
        Ok(())
    })();

    CoUninitialize(); // Always called, even if the closure returned Err
    result
}
```

### Example Implementation (Complete Module)

```rust
// shortcut.rs
//! Start Menu shortcut creation and management (Windows only).
//!
//! Uses COM (`IShellLinkW` + `IPersistFile`) to create and read `.lnk` files
//! in the user's Start Menu Programs folder.

use std::path::{Path, PathBuf};

/// Result of checking the Start Menu shortcut state at launch.
pub enum ShortcutCheckResult {
    /// Shortcut already exists and points to the current binary.
    AlreadyCorrect,
    /// Shortcut was freshly created (didn't exist before).
    Created,
    /// Shortcut exists but points to a different binary (old target path).
    Mismatched(String),
    /// Something went wrong (error description).
    Failed(String),
}

// CUSTOMIZE: Replace "MyApp" with your application name.
#[cfg(windows)]
fn shortcut_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        Path::new(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("MyApp.lnk") // CUSTOMIZE: Replace with "{YourAppName}.lnk"
    })
}

/// Encodes a Rust string as a null-terminated wide (UTF-16) vector.
#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates a `.lnk` shortcut file pointing to `target_exe`.
#[cfg(windows)]
fn create_lnk(lnk_path: &Path, target_exe: &Path) -> Result<(), String> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, IPersistFile, STGM,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| format!("CoInitializeEx failed: {e}"))?;

        let result = (|| {
            let shell_link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(IShellLinkW) failed: {e}"))?;

            let target_wide = to_wide(&target_exe.to_string_lossy());
            shell_link
                .SetPath(PCWSTR(target_wide.as_ptr()))
                .map_err(|e| format!("SetPath failed: {e}"))?;

            // CUSTOMIZE: Replace with your app's description string.
            let desc_wide = to_wide("My application description");
            shell_link
                .SetDescription(PCWSTR(desc_wide.as_ptr()))
                .map_err(|e| format!("SetDescription failed: {e}"))?;

            let icon_wide = to_wide(&target_exe.to_string_lossy());
            shell_link
                .SetIconLocation(PCWSTR(icon_wide.as_ptr()), 0)
                .map_err(|e| format!("SetIconLocation failed: {e}"))?;

            // Cast IShellLinkW to IPersistFile to save the .lnk file.
            let persist: IPersistFile = shell_link
                .cast()
                .map_err(|e| format!("QueryInterface(IPersistFile) failed: {e}"))?;

            let lnk_wide = to_wide(&lnk_path.to_string_lossy());
            persist
                .Save(PCWSTR(lnk_wide.as_ptr()), true)
                .map_err(|e| format!("IPersistFile::Save failed: {e}"))?;

            Ok(())
        })();

        CoUninitialize();
        result
    }
}

/// Reads the target path from an existing `.lnk` file.
#[cfg(windows)]
fn read_lnk_target(lnk_path: &Path) -> Option<String> {
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize,
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, IPersistFile, STGM,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok().ok()?;

        let result = (|| -> Option<String> {
            let shell_link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;

            let persist: IPersistFile = shell_link.cast().ok()?;

            let lnk_wide = to_wide(&lnk_path.to_string_lossy());
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
   - Match MenuId to quit → initiate app shutdown, send TrayCommand::Quit.
```

---

## What NOT to Do

<negative_constraints>
- Do NOT use `HKLM` (local machine) registry keys. Everything is per-user (`HKCU`).
- Do NOT use `Task Scheduler` for autostart. The Run key is simpler and sufficient.
- Do NOT bundle or require external `.ico` files for the tray icon. Generate the icon bytes in code.
- Do NOT make the tray icon or shortcut features conditional behind feature flags. They are always compiled on Windows.
- Do NOT use `unwrap()` or `expect()` on registry/COM operations. Log and gracefully degrade.
- Do NOT create the `Menu` on the main thread and send it to the tray thread. `Menu` contains `Rc` and is `!Send`. Build it on the tray thread.
- Do NOT use `std::sync::mpsc::Receiver` in a blocking `recv()` call on the tray thread. Use `try_recv()` inside the `PeekMessageW` polling loop so Win32 messages are also processed.
- Do NOT add a GUI settings window for these features. The tray menu IS the settings interface for autostart. The shortcut dialog is a one-time launch-time check.
- Do NOT add any AI/agent attribution (Co-Authored-By, etc.) to commits.
- Do NOT use `CoInitialize` (the non-Ex variant). Always use `CoInitializeEx` with `COINIT_APARTMENTTHREADED`.
- Do NOT forget to call `CoUninitialize()` on every exit path from COM functions. Use the closure pattern shown in the examples.
- Do NOT use `windows` crate API patterns from versions prior to v0.50. The v0.58 API uses `HSTRING`, `Interface` trait for casting, and returns `WIN32_ERROR` from registry operations.
</negative_constraints>

---

## Output Format

<output_format>
- Produce each module as a **complete, self-contained Rust source file** with all necessary `use` imports at the top of each function or module, ready to be placed in `src/` and registered in `main.rs` via `mod autostart;`, `mod shortcut;`, `mod tray;`.
- For integration points (app initialization and event loop dispatch), provide a clearly commented Rust code block showing the initialization sequence and event loop dispatch logic. Mark adaptation points with `// CUSTOMIZE:` comments explaining what the implementer must change for their specific app.
- Use `// CUSTOMIZE:` as the comment prefix for all app-specific adaptation points (app name, icon color, tooltip text, description strings, additional menu items, dialog mechanism).
- Implementation order: `autostart.rs` first, then `shortcut.rs`, then `tray.rs`, then integration code.
</output_format>

---

## Summary of Config Changes

Add these two fields to your existing config struct with the specified defaults. Both are persisted to the existing configured config file:

| Field | Type | Default | Purpose |
|---|---|---|---|
| `autostart` | `bool` | `true` | Controls the Windows Run registry key |
| `start_menu_shortcut_declined` | `bool` | `false` | Suppresses the shortcut mismatch dialog permanently |

---

## Cargo.toml Dependency Additions

```toml
[dependencies]
dirs = "6"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_Registry",
    "Win32_UI_Shell",
    "Win32_System_Com",
    "Win32_Storage_FileSystem",
    "Win32_Foundation",
] }
tray-icon = "0.21"
```

If your project already uses the `windows` crate, merge the feature flags into the existing entry — in particular, ensure `Win32_Storage_FileSystem` is present (required for `IPersistFile`). If you already use `tray-icon`, verify the version is compatible (0.21.x uses `MenuEvent::receiver()` as a static method returning `&'static MenuEventReceiver`). The `dirs` crate is only used by the shortcut module to resolve `%APPDATA%` portably, but if your existing config system already resolves that path differently, you may use your existing approach instead.
