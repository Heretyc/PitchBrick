/// Audio device enumeration and selection.
///
/// Provides functions to list available input (microphone) and output
/// (speaker) devices and to resolve a device by name with fallback
/// to the system default.
use cpal::traits::{DeviceTrait, HostTrait};

/// A named audio device with its cpal device handle.
///
/// # Fields
///
/// * `name` - Human-readable device name from the OS (e.g., "Microphone (Realtek Audio)").
/// * `device` - The underlying cpal device handle.
pub struct NamedDevice {
    pub name: String,
    pub device: cpal::Device,
}

/// Enumerates all available audio input (microphone) devices.
///
/// # Returns
///
/// A vector of `NamedDevice` entries for each input device the system reports.
/// Returns an empty vector if enumeration fails.
pub fn enumerate_input_devices() -> Vec<NamedDevice> {
    let host = cpal::default_host();
    let devices = match host.input_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to enumerate input devices: {}", e);
            return Vec::new();
        }
    };

    devices
        .filter_map(|d| {
            let name = d.description().ok()?.name().to_string();
            Some(NamedDevice { name, device: d })
        })
        .collect()
}

/// Enumerates all available audio output (speaker/headphone) devices.
///
/// # Returns
///
/// A vector of `NamedDevice` entries for each output device the system reports.
/// Returns an empty vector if enumeration fails.
pub fn enumerate_output_devices() -> Vec<NamedDevice> {
    let host = cpal::default_host();
    let devices = match host.output_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to enumerate output devices: {}", e);
            return Vec::new();
        }
    };

    devices
        .filter_map(|d| {
            let name = d.description().ok()?.name().to_string();
            Some(NamedDevice { name, device: d })
        })
        .collect()
}

/// Finds an input device by name, falling back to the system default.
///
/// # Arguments
///
/// * `name` - The device name to search for. If empty, returns the default device.
///
/// # Returns
///
/// The matching cpal device, or `None` if no input device is available.
pub fn find_input_device(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if !name.is_empty() {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if let Ok(desc) = device.description() {
                    if desc.name() == name {
                        return Some(device);
                    }
                }
            }
        }
        tracing::warn!("Input device '{}' not found, using default", name);
    }

    host.default_input_device()
}

/// Finds an output device by name, falling back to the system default.
///
/// # Arguments
///
/// * `name` - The device name to search for. If empty, returns the default device.
///
/// # Returns
///
/// The matching cpal device, or `None` if no output device is available.
pub fn find_output_device(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if !name.is_empty() {
        if let Ok(devices) = host.output_devices() {
            for device in devices {
                if let Ok(desc) = device.description() {
                    if desc.name() == name {
                        return Some(device);
                    }
                }
            }
        }
        tracing::warn!("Output device '{}' not found, using default", name);
    }

    host.default_output_device()
}
