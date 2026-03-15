/// Windows autostart (Run registry key) management.
///
/// Syncs `HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\PitchBrick`
/// on every launch to match the user's `config.autostart` preference.
/// Uses HKCU so no admin rights are required.

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "PitchBrick";

/// Ensures the Windows autostart registry entry matches `enabled`.
///
/// If `enabled` is true and the entry is missing or stale, it is written.
/// If `enabled` is false and the entry exists, it is removed.
/// Errors are logged but never propagated — autostart is best-effort.
pub fn sync_autostart(enabled: bool) {
    #[cfg(windows)]
    {
        if enabled {
            set_autostart();
        } else {
            remove_autostart();
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
    }
}

#[cfg(windows)]
fn set_autostart() {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("autostart: could not get current exe path: {e}");
            return;
        }
    };

    let exe_str = exe.to_string_lossy();
    tracing::debug!("autostart: setting Run key to {exe_str}");

    let mut key = HKEY::default();
    let subkey = HSTRING::from(RUN_KEY);
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status.is_err() {
        tracing::warn!("autostart: could not open Run key: {:?}", status);
        return;
    }

    // Encode as null-terminated UTF-16 for REG_SZ.
    let value_name = HSTRING::from(VALUE_NAME);
    let mut data: Vec<u16> = OsStr::new(exe_str.as_ref())
        .encode_wide()
        .collect();
    data.push(0);
    let byte_len = (data.len() * 2) as u32;

    let status = unsafe {
        RegSetValueExW(
            key,
            &value_name,
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                data.as_ptr().cast(),
                byte_len as usize,
            )),
        )
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
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &subkey,
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if status.is_err() {
        // Key doesn't exist — nothing to remove.
        return;
    }

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
