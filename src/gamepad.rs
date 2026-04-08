//! Virtual Xbox 360 controller via ViGEmBus for PTT-on-Green.
//!
//! Spawns a background thread that owns a virtual Xbox 360 controller and
//! processes press/release commands from the main thread. The controller is
//! plugged in when the thread starts and unplugged when it exits.
//!
//! Requires the ViGEmBus kernel driver to be installed on the system.
//! If the driver is missing, `try_connect()` returns `DriverMissing` and
//! the caller can offer to install it via `download_and_install_vigembus()`.

use std::sync::mpsc;

/// Commands sent from the iced main thread to the gamepad thread.
pub enum GamepadCommand {
    /// Press the specified button (raw XButtons u16 value).
    Press(u16),
    /// Release all buttons.
    Release,
    /// Shut down the thread and unplug the virtual controller.
    Quit,
}

/// Result of attempting to connect to the ViGEmBus driver.
pub enum GamepadConnectResult {
    /// Successfully connected; command sender for the gamepad thread.
    Connected(mpsc::Sender<GamepadCommand>),
    /// ViGEmBus driver is not installed.
    DriverMissing,
    /// Some other error occurred.
    Error(String),
}

/// Available Xbox 360 button names for the settings picker.
#[allow(dead_code)]
pub const AVAILABLE_BUTTONS: &[&str] = &[
    "A", "B", "X", "Y", "LB", "RB", "BACK", "START", "LTHUMB", "RTHUMB",
    "UP", "DOWN", "LEFT", "RIGHT",
];

/// Maps a button display name to the raw `XButtons` u16 value.
pub fn button_name_to_xbuttons(name: &str) -> Option<u16> {
    Some(match name {
        "UP"     => 0x0001,
        "DOWN"   => 0x0002,
        "LEFT"   => 0x0004,
        "RIGHT"  => 0x0008,
        "START"  => 0x0010,
        "BACK"   => 0x0020,
        "LTHUMB" => 0x0040,
        "RTHUMB" => 0x0080,
        "LB"     => 0x0100,
        "RB"     => 0x0200,
        "A"      => 0x1000,
        "B"      => 0x2000,
        "X"      => 0x4000,
        "Y"      => 0x8000,
        _ => return None,
    })
}

/// Attempts to connect to the ViGEmBus driver and spawn a gamepad thread.
///
/// On success, plugs in a virtual Xbox 360 controller and returns a command
/// sender. On failure, returns `DriverMissing` or `Error`.
#[cfg(windows)]
pub fn try_connect() -> GamepadConnectResult {
    let client = match vigem_client::Client::connect() {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("{:?}", e);
            tracing::debug!("Gamepad: ViGEmBus connect failed: {}", msg);
            if msg.contains("BusNotFound") || msg.contains("NotFound") {
                return GamepadConnectResult::DriverMissing;
            }
            return GamepadConnectResult::Error(msg);
        }
    };

    tracing::info!("Gamepad: connected to ViGEmBus driver");

    let (tx, rx) = mpsc::channel::<GamepadCommand>();

    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            gamepad_thread_main(client, rx);
        })
        .ok();

    GamepadConnectResult::Connected(tx)
}

#[cfg(not(windows))]
pub fn try_connect() -> GamepadConnectResult {
    GamepadConnectResult::Error("ViGEmBus is only available on Windows".into())
}

/// Main loop for the gamepad background thread.
#[cfg(windows)]
fn gamepad_thread_main(client: vigem_client::Client, rx: mpsc::Receiver<GamepadCommand>) {
    let id = vigem_client::TargetId::XBOX360_WIRED;
    let mut target = vigem_client::Xbox360Wired::new(client, id);

    if let Err(e) = target.plugin() {
        tracing::error!("Gamepad: failed to plug in virtual controller: {:?}", e);
        return;
    }

    if let Err(e) = target.wait_ready() {
        tracing::error!("Gamepad: controller not ready: {:?}", e);
        return;
    }

    tracing::info!("Gamepad: virtual Xbox 360 controller plugged in and ready");

    loop {
        match rx.recv() {
            Ok(GamepadCommand::Press(buttons)) => {
                let gamepad = vigem_client::XGamepad {
                    buttons: vigem_client::XButtons(buttons),
                    ..Default::default()
                };
                match target.update(&gamepad) {
                    Ok(()) => tracing::debug!("Gamepad: button press (0x{:04X})", buttons),
                    Err(e) => tracing::error!("Gamepad: press failed: {:?}", e),
                }
            }
            Ok(GamepadCommand::Release) => {
                match target.update(&vigem_client::XGamepad::default()) {
                    Ok(()) => tracing::debug!("Gamepad: buttons released"),
                    Err(e) => tracing::error!("Gamepad: release failed: {:?}", e),
                }
            }
            Ok(GamepadCommand::Quit) | Err(_) => {
                tracing::info!("Gamepad: shutting down, unplugging controller");
                drop(target);
                return;
            }
        }
    }
}

/// Downloads and installs the ViGEmBus v1.22.0 driver with UAC elevation.
///
/// Downloads the installer from GitHub releases, saves to %TEMP%, and runs
/// it with the `runas` verb for admin elevation. Blocks until the installer
/// exits.
#[cfg(windows)]
pub fn download_and_install_vigembus() -> Result<(), String> {
    use std::io::Write;

    let url = "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe";

    tracing::info!("Gamepad: downloading ViGEmBus installer from {}", url);

    let resp = ureq::get(url)
        .set("User-Agent", "pitchbrick/vigembus-installer")
        .call()
        .map_err(|e| format!("Download failed: {}", e))?;

    let tmp_dir = std::env::temp_dir();
    let installer_path = tmp_dir.join("ViGEmBus_Setup.exe");

    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    let mut file = std::fs::File::create(&installer_path)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    file.write_all(&body)
        .map_err(|e| format!("Failed to write installer: {}", e))?;
    drop(file);

    tracing::info!(
        "Gamepad: installer saved to {}, launching with UAC elevation",
        installer_path.display()
    );

    // Run the installer with UAC elevation via ShellExecuteExW.
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::WaitForSingleObject;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};
    use windows::core::PCWSTR;

    let exe_wide: Vec<u16> = OsStr::new(installer_path.to_string_lossy().as_ref())
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Silent install with progress bar, no restart.
    let params_str = "/exenoui /qn /norestart";
    let params_wide: Vec<u16> = OsStr::new(params_str)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let verb_wide: Vec<u16> = OsStr::new("runas")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb_wide.as_ptr()),
        lpFile: PCWSTR(exe_wide.as_ptr()),
        lpParameters: PCWSTR(params_wide.as_ptr()),
        ..Default::default()
    };

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if let Err(e) = ok {
        // Clean up the downloaded file.
        let _ = std::fs::remove_file(&installer_path);
        return Err(format!("Failed to launch installer (UAC may have been denied): {}", e));
    }

    // Wait for the installer process to complete.
    if !sei.hProcess.is_invalid() && !sei.hProcess.0.is_null() {
        tracing::info!("Gamepad: waiting for installer to complete...");
        let wait_result = unsafe { WaitForSingleObject(sei.hProcess, 120_000) };
        unsafe { let _ = CloseHandle(sei.hProcess); }
        if wait_result != WAIT_OBJECT_0 {
            let _ = std::fs::remove_file(&installer_path);
            return Err("Installer timed out or failed to complete".into());
        }
    }

    // Clean up.
    let _ = std::fs::remove_file(&installer_path);
    tracing::info!("Gamepad: ViGEmBus installation completed");

    Ok(())
}

#[cfg(not(windows))]
pub fn download_and_install_vigembus() -> Result<(), String> {
    Err("ViGEmBus is only available on Windows".into())
}

use std::io::Read;
