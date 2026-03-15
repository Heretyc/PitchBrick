//! Start Menu shortcut creation and management (Windows only).
//!
//! Uses COM (`IShellLinkW` + `IPersistFile`) to create and read `.lnk` files
//! in the user's Start Menu Programs folder.

use std::path::{Path, PathBuf};
use windows::core::{Interface, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    IPersistFile, STGM,
};
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

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

/// Returns the expected path: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\PitchBrick.lnk`.
fn shortcut_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        Path::new(&appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("PitchBrick.lnk")
    })
}

/// Encodes a Rust string as a null-terminated wide (UTF-16) vector.
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Creates a `.lnk` shortcut file pointing to `target_exe`.
fn create_lnk(lnk_path: &Path, target_exe: &Path) -> Result<(), String> {
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

            let desc_wide = to_wide("Transgender vocal training pitch monitor");
            shell_link
                .SetDescription(PCWSTR(desc_wide.as_ptr()))
                .map_err(|e| format!("SetDescription failed: {e}"))?;

            let icon_wide = to_wide(&target_exe.to_string_lossy());
            shell_link
                .SetIconLocation(PCWSTR(icon_wide.as_ptr()), 0)
                .map_err(|e| format!("SetIconLocation failed: {e}"))?;

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
fn read_lnk_target(lnk_path: &Path) -> Option<String> {
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
pub fn update_shortcut() -> Result<(), String> {
    let lnk_path = shortcut_path()
        .ok_or_else(|| "Could not determine APPDATA path".to_string())?;
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("current_exe failed: {e}"))?;
    create_lnk(&lnk_path, &current_exe)
}
