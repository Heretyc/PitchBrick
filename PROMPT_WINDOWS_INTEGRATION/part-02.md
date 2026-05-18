# Windows Integration Prompt Part 2

Status: archived implementation prompt reference.

This file is a split continuation of `PROMPT_WINDOWS_INTEGRATION.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.


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
