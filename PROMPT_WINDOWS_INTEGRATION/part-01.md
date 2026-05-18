# Windows Integration Prompt Part 1

Status: archived implementation prompt reference.

This file is a split continuation of `PROMPT_WINDOWS_INTEGRATION.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.

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
