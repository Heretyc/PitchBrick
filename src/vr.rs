//! SteamVR overlay thread for PitchBrick.
//!
//! Displays a small color indicator in the user's VR headset that mirrors
//! the desktop pitch display. Uses runtime loading of `openvr_api.dll` so
//! the app degrades gracefully when SteamVR is not installed.
//!
//! The thread retries VR initialisation every 10 seconds so it works with
//! setups where SteamVR starts later (e.g. Virtual Desktop Streamer).
//!
//! Gated behind `#[cfg(feature = "vr-overlay")]` at the module level.

use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// OpenVR C-API type definitions (minimal subset)
// ---------------------------------------------------------------------------

type VROverlayHandle = u64;
type TrackedDeviceIndex = u32;

#[repr(C)]
#[derive(Clone, Copy)]
struct HmdMatrix34 {
    m: [[f32; 4]; 3],
}

// Calling convention notes:
// - Function-table entries use `extern "system"` (stdcall on Win32, C on x64).
// - DLL-exported free functions use `extern "C"` (cdecl).
type VrInitInternal2Fn =
    unsafe extern "C" fn(error: *mut i32, app_type: i32, startup_info: *const u8) -> u32;
type VrShutdownInternalFn = unsafe extern "C" fn();
type VrGetGenericInterfaceFn =
    unsafe extern "C" fn(iface_version: *const u8, error: *mut i32) -> *mut c_void;

// -- IVRApplications function-table entry types --
type AddApplicationManifestFn =
    unsafe extern "system" fn(path: *const u8, temporary: bool) -> i32;
type SetApplicationAutoLaunchFn =
    unsafe extern "system" fn(app_key: *const u8, auto_launch: bool) -> i32;

// -- IVROverlay function-table entry types --
type CreateOverlayFn = unsafe extern "system" fn(
    key: *const u8,
    name: *const u8,
    handle: *mut VROverlayHandle,
) -> i32;
type DestroyOverlayFn = unsafe extern "system" fn(handle: VROverlayHandle) -> i32;
type SetOverlayWidthFn =
    unsafe extern "system" fn(handle: VROverlayHandle, width: f32) -> i32;
type SetOverlayTransformDeviceRelativeFn = unsafe extern "system" fn(
    handle: VROverlayHandle,
    device: TrackedDeviceIndex,
    transform: *const HmdMatrix34,
) -> i32;
type ShowOverlayFn = unsafe extern "system" fn(handle: VROverlayHandle) -> i32;
type SetOverlayRawFn = unsafe extern "system" fn(
    handle: VROverlayHandle,
    buffer: *mut c_void,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
) -> i32;

/// Pointers into the `VR_IVROverlay_FnTable` that we actually use.
struct OverlayFns {
    create: CreateOverlayFn,
    destroy: DestroyOverlayFn,
    set_width: SetOverlayWidthFn,
    set_transform_device_relative: SetOverlayTransformDeviceRelativeFn,
    show: ShowOverlayFn,
    set_raw: SetOverlayRawFn,
}

// IVROverlay_028 function table indices (from openvr_capi.h)
const IDX_CREATE_OVERLAY: usize = 1;
const IDX_DESTROY_OVERLAY: usize = 3;
const IDX_SET_OVERLAY_WIDTH: usize = 22;
const IDX_SET_TRANSFORM_DEVICE_RELATIVE: usize = 35;
const IDX_SHOW_OVERLAY: usize = 43;
const IDX_SET_OVERLAY_RAW: usize = 62;
const FN_TABLE_LEN: usize = 82;

const OVERLAY_INTERFACE: &[u8] = b"FnTable:IVROverlay_028\0";
const APPLICATIONS_INTERFACE: &[u8] = b"FnTable:IVRApplications_007\0";
const APP_TYPE_OVERLAY: i32 = 2;

const IDX_ADD_APPLICATION_MANIFEST: usize = 0;
const IDX_SET_APPLICATION_AUTO_LAUNCH: usize = 17;

const APP_KEY: &str = "com.pitchbrick.overlay";
const K_TRACKED_DEVICE_HMD: TrackedDeviceIndex = 0;

/// How often to retry VR initialisation when SteamVR isn't running yet.
const VR_RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// Optional VR overlay position/size configuration passed from config.
#[allow(dead_code)]
struct VrOverlayConfig {
    vr_x: Option<i32>,
    vr_y: Option<i32>,
    vr_width: Option<f32>,
    vr_height: Option<f32>,
}

// ---------------------------------------------------------------------------
// SteamVR process detection
// ---------------------------------------------------------------------------

/// Returns true if `vrserver.exe` is currently running.
///
/// Prevents `VR_InitInternal2` from launching SteamVR when it isn't already
/// running. Uses Win32 process snapshot APIs.
fn is_steamvr_running() -> bool {
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snapshot = match snapshot {
        Ok(h) => h,
        Err(_) => return false,
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    if ok.is_err() {
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
        return false;
    }

    loop {
        let name = String::from_utf16_lossy(
            &entry.szExeFile[..entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len())],
        );
        if name.eq_ignore_ascii_case("vrserver.exe") {
            let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
            return true;
        }
        if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
            break;
        }
    }

    let _ = unsafe { windows::Win32::Foundation::CloseHandle(snapshot) };
    false
}

// ---------------------------------------------------------------------------
// DLL discovery
// ---------------------------------------------------------------------------

fn find_openvr_dll() -> Option<PathBuf> {
    for subkey in [
        "SOFTWARE\\WOW6432Node\\Valve\\OpenVR",
        "SOFTWARE\\Valve\\OpenVR",
    ] {
        if let Some(path) = read_registry_runtime_path(subkey) {
            let dll = path.join("bin").join("win64").join("openvr_api.dll");
            tracing::debug!("VR: checking registry path: {}", dll.display());
            if dll.exists() {
                tracing::debug!("VR: found openvr_api.dll via registry");
                return Some(dll);
            }
        }
    }
    tracing::debug!("VR: registry lookup did not find OpenVR runtime");

    let candidates = [
        "C:\\Program Files (x86)\\Steam\\steamapps\\common\\SteamVR\\bin\\win64\\openvr_api.dll",
        "C:\\Program Files\\Steam\\steamapps\\common\\SteamVR\\bin\\win64\\openvr_api.dll",
        "D:\\Steam\\steamapps\\common\\SteamVR\\bin\\win64\\openvr_api.dll",
        "D:\\SteamLibrary\\steamapps\\common\\SteamVR\\bin\\win64\\openvr_api.dll",
    ];
    for path in candidates {
        let dll = PathBuf::from(path);
        if dll.exists() {
            tracing::debug!("VR: found openvr_api.dll at {}", dll.display());
            return Some(dll);
        }
    }
    tracing::debug!("VR: common Steam paths did not contain openvr_api.dll");
    None
}

fn read_registry_runtime_path(subkey: &str) -> Option<PathBuf> {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;

    let mut key_handle = HKEY::default();
    let subkey_w = HSTRING::from(subkey);

    let status = unsafe {
        RegOpenKeyExW(HKEY_LOCAL_MACHINE, &subkey_w, 0, KEY_READ, &mut key_handle)
    };
    if status.is_err() {
        return None;
    }

    let value_name = HSTRING::from("RuntimePath");
    let mut buf_size: u32 = 512;
    let mut buf = vec![0u16; buf_size as usize / 2];

    let status = unsafe {
        RegQueryValueExW(
            key_handle,
            &value_name,
            None,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut buf_size),
        )
    };
    let _ = unsafe { RegCloseKey(key_handle) };

    if status.is_err() {
        return None;
    }

    let len = (buf_size as usize / 2).saturating_sub(1);
    let path_str = String::from_utf16_lossy(&buf[..len]);
    let path = PathBuf::from(path_str.trim_end_matches('\0'));
    tracing::debug!("VR: registry RuntimePath = {}", path.display());
    Some(path)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Commands sent from the main thread to the VR overlay thread.
pub enum VrOverlayCommand {
    SetColor([u8; 4]),
    Quit,
}

fn generate_vrmanifest() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let manifest_path = exe.with_file_name("pitchbrick.vrmanifest");
    let exe_path = exe.to_string_lossy().replace('\\', "/");
    let content = format!(
        r#"{{
  "source": "builtin",
  "applications": [
    {{
      "app_key": "{}",
      "launch_type": "binary",
      "binary_path_windows": "{}",
      "is_dashboard_overlay": true,
      "strings": {{
        "en_us": {{
          "name": "PitchBrick VR Overlay",
          "description": "Transgender vocal training pitch indicator overlay."
        }}
      }}
    }}
  ]
}}"#,
        APP_KEY, exe_path
    );
    match std::fs::write(&manifest_path, &content) {
        Ok(()) => {
            tracing::debug!("VR: wrote vrmanifest to {}", manifest_path.display());
            Some(manifest_path)
        }
        Err(e) => {
            tracing::warn!("VR: failed to write vrmanifest: {}", e);
            None
        }
    }
}

/// Spawns the VR overlay background thread.
///
/// Returns `Some(sender)` immediately. The thread retries VR initialisation
/// every 10 seconds until SteamVR becomes available (or `Quit` is received).
/// Returns `None` only if `openvr_api.dll` cannot be found at all (SteamVR
/// not installed).
pub fn spawn_vr_overlay_thread(
    vr_x: Option<i32>,
    vr_y: Option<i32>,
    vr_width: Option<f32>,
    vr_height: Option<f32>,
) -> Option<mpsc::Sender<VrOverlayCommand>> {
    tracing::debug!("VR: looking for openvr_api.dll");

    // Locate the DLL — this is the only hard failure (SteamVR not installed).
    let dll_path = if let Some(p) = find_openvr_dll() {
        p
    } else {
        // Try bare name as last resort.
        match unsafe { libloading::Library::new("openvr_api.dll") } {
            Ok(_) => {
                // It loaded — use bare name. Drop the test load; re-load in thread.
                PathBuf::from("openvr_api.dll")
            }
            Err(e) => {
                tracing::warn!("VR: openvr_api.dll not found, overlay disabled: {}", e);
                return None;
            }
        }
    };
    tracing::debug!("VR: will use {}", dll_path.display());

    let (tx, rx) = mpsc::channel::<VrOverlayCommand>();

    let overlay_config = VrOverlayConfig {
        vr_x,
        vr_y,
        vr_width,
        vr_height,
    };

    std::thread::spawn(move || {
        vr_thread_main(dll_path, rx, overlay_config);
    });

    Some(tx)
}

/// Main loop for the VR thread. Retries VR init until SteamVR is available.
fn vr_thread_main(
    dll_path: PathBuf,
    rx: mpsc::Receiver<VrOverlayCommand>,
    overlay_config: VrOverlayConfig,
) {
    // Load the DLL (persists for the lifetime of this thread).
    let lib = match unsafe { libloading::Library::new(&dll_path) } {
        Ok(lib) => lib,
        Err(e) => {
            tracing::warn!("VR: failed to load {}: {}", dll_path.display(), e);
            return;
        }
    };
    tracing::debug!("VR: openvr_api.dll loaded");

    // Resolve DLL exports once.
    let vr_init: VrInitInternal2Fn = unsafe {
        match lib.get::<VrInitInternal2Fn>(b"VR_InitInternal2\0") {
            Ok(sym) => *sym,
            Err(e) => {
                tracing::warn!("VR: symbol VR_InitInternal2 not found: {}", e);
                return;
            }
        }
    };
    let vr_shutdown: VrShutdownInternalFn = unsafe {
        match lib.get::<VrShutdownInternalFn>(b"VR_ShutdownInternal\0") {
            Ok(sym) => *sym,
            Err(e) => {
                tracing::warn!("VR: symbol VR_ShutdownInternal not found: {}", e);
                return;
            }
        }
    };
    let vr_get_iface: VrGetGenericInterfaceFn = unsafe {
        match lib.get::<VrGetGenericInterfaceFn>(b"VR_GetGenericInterface\0") {
            Ok(sym) => *sym,
            Err(e) => {
                tracing::warn!("VR: symbol VR_GetGenericInterface not found: {}", e);
                return;
            }
        }
    };
    tracing::debug!("VR: DLL exports resolved");

    // Retry loop: wait for SteamVR to be running, then connect.
    loop {
        // Check for Quit before each attempt.
        match rx.try_recv() {
            Ok(VrOverlayCommand::Quit) | Err(mpsc::TryRecvError::Disconnected) => {
                tracing::debug!("VR: quit received during retry loop");
                return;
            }
            _ => {}
        }

        // Only attempt VR init if SteamVR is already running.
        // Calling VR_InitInternal2 when SteamVR isn't running can launch it.
        if !is_steamvr_running() {
            tracing::debug!(
                "VR: SteamVR not running, waiting {}s",
                VR_RETRY_INTERVAL.as_secs()
            );
            for _ in 0..(VR_RETRY_INTERVAL.as_millis() / 200) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(VrOverlayCommand::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        tracing::debug!("VR: quit received while waiting for SteamVR");
                        return;
                    }
                    _ => {}
                }
            }
            continue;
        }

        tracing::debug!("VR: attempting VR_InitInternal2 (overlay mode)");
        let mut error: i32 = 0;
        unsafe {
            vr_init(&mut error, APP_TYPE_OVERLAY, c"".as_ptr().cast());
        }
        if error != 0 {
            tracing::debug!(
                "VR: VR_InitInternal2 failed (error {}), retrying in {}s",
                error,
                VR_RETRY_INTERVAL.as_secs()
            );
            // Sleep in small increments so we can respond to Quit promptly.
            for _ in 0..(VR_RETRY_INTERVAL.as_millis() / 200) {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(VrOverlayCommand::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                        tracing::debug!("VR: quit received while waiting to retry");
                        return;
                    }
                    _ => {}
                }
            }
            continue;
        }

        tracing::info!("VR: connected to SteamVR");

        // Get overlay function table.
        let overlay_fns = unsafe {
            let table_ptr = vr_get_iface(OVERLAY_INTERFACE.as_ptr(), &mut error);
            if table_ptr.is_null() || error != 0 {
                tracing::warn!("VR: IVROverlay_028 not available (error {})", error);
                vr_shutdown();
                return;
            }
            tracing::debug!("VR: obtained IVROverlay_028 function table");
            let fns = table_ptr as *const *const c_void;
            if FN_TABLE_LEN <= IDX_SET_OVERLAY_RAW {
                tracing::warn!("VR: function table too small");
                vr_shutdown();
                return;
            }
            OverlayFns {
                create: std::mem::transmute::<*const c_void, CreateOverlayFn>(
                    *fns.add(IDX_CREATE_OVERLAY),
                ),
                destroy: std::mem::transmute::<*const c_void, DestroyOverlayFn>(
                    *fns.add(IDX_DESTROY_OVERLAY),
                ),
                set_width: std::mem::transmute::<*const c_void, SetOverlayWidthFn>(
                    *fns.add(IDX_SET_OVERLAY_WIDTH),
                ),
                set_transform_device_relative: std::mem::transmute::<
                    *const c_void,
                    SetOverlayTransformDeviceRelativeFn,
                >(
                    *fns.add(IDX_SET_TRANSFORM_DEVICE_RELATIVE)
                ),
                show: std::mem::transmute::<*const c_void, ShowOverlayFn>(
                    *fns.add(IDX_SHOW_OVERLAY),
                ),
                set_raw: std::mem::transmute::<*const c_void, SetOverlayRawFn>(
                    *fns.add(IDX_SET_OVERLAY_RAW),
                ),
            }
        };

        // Register vrmanifest + auto-launch.
        register_startup_app(vr_get_iface, &mut error);

        // Create and run the overlay, blocking until Quit or disconnect.
        run_overlay(&overlay_fns, &rx, &overlay_config);

        // Cleanup.
        unsafe { vr_shutdown() };
        tracing::debug!("VR: VR_Shutdown called");
        return;
    }
}

/// Registers the vrmanifest with SteamVR and enables auto-launch.
fn register_startup_app(
    vr_get_iface: VrGetGenericInterfaceFn,
    error: &mut i32,
) {
    let manifest_path = match generate_vrmanifest() {
        Some(p) => p,
        None => return,
    };

    unsafe {
        let apps_table = vr_get_iface(APPLICATIONS_INTERFACE.as_ptr(), error);
        if apps_table.is_null() || *error != 0 {
            tracing::warn!(
                "VR: IVRApplications_007 not available (error {}), skipping registration",
                *error
            );
            return;
        }
        tracing::debug!("VR: obtained IVRApplications_007 function table");

        let fns = apps_table as *const *const c_void;
        let add_manifest = std::mem::transmute::<*const c_void, AddApplicationManifestFn>(
            *fns.add(IDX_ADD_APPLICATION_MANIFEST),
        );
        let set_auto_launch = std::mem::transmute::<*const c_void, SetApplicationAutoLaunchFn>(
            *fns.add(IDX_SET_APPLICATION_AUTO_LAUNCH),
        );

        let manifest_str = CString::new(manifest_path.to_string_lossy().as_ref()).unwrap();
        tracing::debug!("VR: AddApplicationManifest(\"{}\")", manifest_path.display());
        let err = add_manifest(manifest_str.as_ptr().cast(), false);
        if err != 0 {
            tracing::warn!("VR: AddApplicationManifest failed (error {})", err);
            return;
        }
        tracing::debug!("VR: AddApplicationManifest succeeded");

        let app_key_c = CString::new(APP_KEY).unwrap();
        tracing::debug!("VR: SetApplicationAutoLaunch(\"{}\", true)", APP_KEY);
        let err = set_auto_launch(app_key_c.as_ptr().cast(), true);
        if err != 0 {
            tracing::warn!("VR: SetApplicationAutoLaunch failed (error {})", err);
        } else {
            tracing::info!("VR: registered as SteamVR startup overlay app");
        }
    }
}

/// Creates the overlay, processes color commands, and returns on Quit/disconnect.
fn run_overlay(
    fns: &OverlayFns,
    rx: &mpsc::Receiver<VrOverlayCommand>,
    overlay_config: &VrOverlayConfig,
) {
    let key = CString::new("pitchbrick.indicator").unwrap();
    let name = CString::new("PitchBrick").unwrap();
    let mut handle: VROverlayHandle = 0;

    let err = unsafe {
        (fns.create)(key.as_ptr().cast(), name.as_ptr().cast(), &mut handle)
    };
    if err != 0 {
        tracing::warn!("VR: CreateOverlay failed (error {})", err);
        return;
    }
    tracing::debug!("VR: CreateOverlay succeeded (handle={})", handle);

    // Size: use config width if provided, else default 0.045m.
    let width = overlay_config
        .vr_width
        .map(|w| w / 1920.0)
        .unwrap_or(0.045_f32);
    let err = unsafe { (fns.set_width)(handle, width) };
    tracing::debug!("VR: SetOverlayWidthInMeters({}) = {}", width, err);

    // Position: use config values if provided, else default position.
    let (x, y, z) = if let (Some(vr_x), Some(vr_y)) = (overlay_config.vr_x, overlay_config.vr_y) {
        let x = vr_x as f32 / 1920.0;
        let y = -(vr_y as f32 / 1080.0); // negate: screen Y is down, VR Y is up
        let z = -1.0_f32;
        (x, y, z)
    } else {
        // Default: ~30deg right, ~20deg up, 1m in front of HMD,
        // shifted 4 units toward center on a 45deg diagonal.
        let angle_right = 30.0_f32.to_radians();
        let angle_up = 20.0_f32.to_radians();
        let distance = 1.0_f32;
        let shift = 4.0 * 0.015 / 2.0_f32.sqrt();
        let x = distance * angle_right.sin() - shift;
        let y = distance * angle_up.sin() - shift;
        let z = -distance * angle_right.cos() * angle_up.cos();
        (x, y, z)
    };

    let transform = HmdMatrix34 {
        m: [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, y],
            [0.0, 0.0, 1.0, z],
        ],
    };
    let err = unsafe {
        (fns.set_transform_device_relative)(handle, K_TRACKED_DEVICE_HMD, &transform)
    };
    tracing::debug!(
        "VR: SetOverlayTransformTrackedDeviceRelative(HMD, [{:.3}, {:.3}, {:.3}]) = {}",
        x, y, z, err
    );

    // Initial green texture (overlay stays green unless explicitly Red).
    set_overlay_color(fns, handle, [0x4C, 0xAF, 0x50, 0xFF]);

    let err = unsafe { (fns.show)(handle) };
    tracing::debug!("VR: ShowOverlay = {}", err);
    tracing::info!("VR: overlay visible");

    // Command loop with drain for smooth color fading.
    loop {
        match rx.recv_timeout(Duration::from_millis(16)) {
            Ok(VrOverlayCommand::SetColor(rgba)) => {
                let mut latest_color = rgba;
                // Drain: keep reading until empty, apply only the latest color.
                while let Ok(next) = rx.try_recv() {
                    match next {
                        VrOverlayCommand::SetColor(c) => latest_color = c,
                        VrOverlayCommand::Quit => {
                            tracing::info!("VR: shutting down overlay");
                            set_overlay_color(fns, handle, latest_color);
                            unsafe { (fns.destroy)(handle) };
                            tracing::debug!("VR: overlay destroyed");
                            return;
                        }
                    }
                }
                set_overlay_color(fns, handle, latest_color);
            }
            Ok(VrOverlayCommand::Quit) => {
                tracing::info!("VR: shutting down overlay");
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    unsafe { (fns.destroy)(handle) };
    tracing::debug!("VR: overlay destroyed");
}

fn set_overlay_color(fns: &OverlayFns, handle: VROverlayHandle, rgba: [u8; 4]) {
    let mut pixels = [0u8; 4 * 4 * 4];
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.copy_from_slice(&rgba);
    }
    unsafe {
        (fns.set_raw)(handle, pixels.as_mut_ptr().cast(), 4, 4, 4);
    }
}
