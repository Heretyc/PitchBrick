/// Audio device enumeration and selection.
///
/// Provides functions to list available input (microphone) and output
/// (speaker) devices and to resolve a device by name with fallback
/// to the system default.
///
/// When multiple devices share the same raw OS name (e.g. two identical
/// USB microphones both called "Microphone"), the enumeration functions
/// append ` (1)`, ` (2)`, etc. to disambiguate them.  Unique names are
/// returned unchanged.
use cpal::traits::{DeviceTrait, HostTrait};
use std::collections::HashMap;

/// Appends occurrence numbers to duplicate names.
///
/// Names that appear only once are left unchanged.  Names that appear
/// more than once get ` (1)`, ` (2)`, … appended in the order they
/// appear in the input slice.
///
/// ```text
/// ["Mic", "Speaker", "Mic"] → ["Mic (1)", "Speaker", "Mic (2)"]
/// ```
fn disambiguate(raw: &[String]) -> Vec<String> {
    // Count how many times each name appears.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for name in raw {
        *counts.entry(name.as_str()).or_insert(0) += 1;
    }

    // Second pass: assign occurrence numbers only to duplicates.
    let mut seen: HashMap<&str, usize> = HashMap::new();
    raw.iter()
        .map(|name| {
            if counts[name.as_str()] > 1 {
                let n = seen.entry(name.as_str()).or_insert(0);
                *n += 1;
                format!("{} ({})", name, n)
            } else {
                name.clone()
            }
        })
        .collect()
}

/// Enumerates all available audio input (microphone) devices.
///
/// Duplicate OS-level names are disambiguated with occurrence suffixes.
///
/// # Returns
///
/// A vector of disambiguated display names, one per input device.
/// Returns an empty vector if enumeration fails.
pub fn enumerate_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let devices = match host.input_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to enumerate input devices: {}", e);
            return Vec::new();
        }
    };

    let raw: Vec<String> = devices
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();

    disambiguate(&raw)
}

/// Enumerates all available audio output (speaker/headphone) devices.
///
/// Duplicate OS-level names are disambiguated with occurrence suffixes.
///
/// # Returns
///
/// A vector of disambiguated display names, one per output device.
/// Returns an empty vector if enumeration fails.
pub fn enumerate_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    let devices = match host.output_devices() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to enumerate output devices: {}", e);
            return Vec::new();
        }
    };

    let raw: Vec<String> = devices
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();

    disambiguate(&raw)
}

/// Returns the disambiguated display name of the system default input device.
///
/// This lets the tray menu show which device is actually active when the
/// user hasn't explicitly chosen one (empty string in config).
pub fn default_input_display_name() -> Option<String> {
    let host = cpal::default_host();
    let default_raw = host
        .default_input_device()?
        .description()
        .ok()?
        .name()
        .to_string();

    let devices = match host.input_devices() {
        Ok(d) => d,
        Err(_) => return Some(default_raw),
    };

    let raw: Vec<String> = devices
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();
    let display = disambiguate(&raw);

    // The default device corresponds to the first occurrence of its raw name.
    raw.iter()
        .zip(display.iter())
        .find(|(r, _)| *r == &default_raw)
        .map(|(_, d)| d.clone())
}

/// Returns the disambiguated display name of the system default output device.
///
/// This lets the tray menu show which device is actually active when the
/// user hasn't explicitly chosen one (empty string in config).
pub fn default_output_display_name() -> Option<String> {
    let host = cpal::default_host();
    let default_raw = host
        .default_output_device()?
        .description()
        .ok()?
        .name()
        .to_string();

    let devices = match host.output_devices() {
        Ok(d) => d,
        Err(_) => return Some(default_raw),
    };

    let raw: Vec<String> = devices
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();
    let display = disambiguate(&raw);

    raw.iter()
        .zip(display.iter())
        .find(|(r, _)| *r == &default_raw)
        .map(|(_, d)| d.clone())
}

/// Finds an input device by its disambiguated display name, falling back
/// to the system default.
///
/// # Lookup order
///
/// 1. Match `display_name` against the disambiguated enumeration list.
/// 2. Fall back to a raw-name match (backwards compat with old configs).
/// 3. Fall back to the system default input device.
///
/// # Arguments
///
/// * `display_name` - The device display name to search for.
///   If empty, returns the default device directly.
pub fn find_input_device(display_name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if display_name.is_empty() {
        return host.default_input_device();
    }

    let devices: Vec<cpal::Device> = match host.input_devices() {
        Ok(d) => d.collect(),
        Err(_) => return host.default_input_device(),
    };

    let raw: Vec<String> = devices
        .iter()
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();
    let display = disambiguate(&raw);

    // 1. Match against disambiguated names.
    if let Some(idx) = display.iter().position(|n| n == display_name) {
        return Some(devices.into_iter().nth(idx).unwrap());
    }

    // 2. Backwards-compat: match against raw names (first occurrence).
    if let Some(idx) = raw.iter().position(|n| n == display_name) {
        tracing::info!(
            "Input device '{}' matched by raw name (legacy config)",
            display_name
        );
        return Some(devices.into_iter().nth(idx).unwrap());
    }

    tracing::warn!(
        "Input device '{}' not found, using default",
        display_name
    );
    host.default_input_device()
}

/// Finds an output device by its disambiguated display name, falling back
/// to the system default.
///
/// # Lookup order
///
/// 1. Match `display_name` against the disambiguated enumeration list.
/// 2. Fall back to a raw-name match (backwards compat with old configs).
/// 3. Fall back to the system default output device.
///
/// # Arguments
///
/// * `display_name` - The device display name to search for.
///   If empty, returns the default device directly.
pub fn find_output_device(display_name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();

    if display_name.is_empty() {
        return host.default_output_device();
    }

    let devices: Vec<cpal::Device> = match host.output_devices() {
        Ok(d) => d.collect(),
        Err(_) => return host.default_output_device(),
    };

    let raw: Vec<String> = devices
        .iter()
        .filter_map(|d| Some(d.description().ok()?.name().to_string()))
        .collect();
    let display = disambiguate(&raw);

    // 1. Match against disambiguated names.
    if let Some(idx) = display.iter().position(|n| n == display_name) {
        return Some(devices.into_iter().nth(idx).unwrap());
    }

    // 2. Backwards-compat: match against raw names (first occurrence).
    if let Some(idx) = raw.iter().position(|n| n == display_name) {
        tracing::info!(
            "Output device '{}' matched by raw name (legacy config)",
            display_name
        );
        return Some(devices.into_iter().nth(idx).unwrap());
    }

    tracing::warn!(
        "Output device '{}' not found, using default",
        display_name
    );
    host.default_output_device()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disambiguate_unique_names() {
        let raw = vec!["A".into(), "B".into(), "C".into()];
        assert_eq!(disambiguate(&raw), vec!["A", "B", "C"]);
    }

    #[test]
    fn disambiguate_duplicate_names() {
        let raw = vec!["Mic".into(), "Speaker".into(), "Mic".into()];
        assert_eq!(disambiguate(&raw), vec!["Mic (1)", "Speaker", "Mic (2)"]);
    }

    #[test]
    fn disambiguate_triple() {
        let raw = vec!["X".into(), "X".into(), "X".into()];
        assert_eq!(disambiguate(&raw), vec!["X (1)", "X (2)", "X (3)"]);
    }

    #[test]
    fn disambiguate_empty() {
        let raw: Vec<String> = vec![];
        let result: Vec<String> = vec![];
        assert_eq!(disambiguate(&raw), result);
    }
}
