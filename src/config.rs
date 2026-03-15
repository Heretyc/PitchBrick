/// Configuration management for PitchBrick.
///
/// Handles loading, saving, validating, and hot-reloading the TOML
/// configuration file stored in the user's home directory.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Minimum gap in Hz that must always separate male_freq_high and female_freq_low.
/// Based on the perceptual boundary zone (155–165 Hz) identified in the literature
/// (Gelfer & Schofield 2000; Mount & Salmon 1988): voices above ~165 Hz are reliably
/// perceived as female; voices below ~155 Hz are reliably perceived as male.
const MIN_GENDER_GAP_HZ: f32 = 10.0;

/// The user's selected target gender for vocal training.
///
/// Determines which frequency range is considered "in target" (green)
/// and which is "out of target" (red).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    /// Target feminine voice range (default 165-255 Hz).
    Female,
    /// Target masculine voice range (default 85-155 Hz).
    Male,
}

impl Gender {
    /// Returns the opposite gender.
    pub fn toggle(self) -> Gender {
        match self {
            Gender::Female => Gender::Male,
            Gender::Male => Gender::Female,
        }
    }
}

impl std::fmt::Display for Gender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Gender::Female => write!(f, "Female"),
            Gender::Male => write!(f, "Male"),
        }
    }
}

/// Enforces validity constraints on frequency ranges.
///
/// 1. Each range's low bound is clamped below its high bound (min 1 Hz apart).
/// 2. `male_high` and `female_low` are always separated by at least
///    `MIN_GENDER_GAP_HZ` (10 Hz). When too close, the non-target boundary moves.
fn fix_freq_overlap(
    target_gender: Gender,
    male_low: &mut f32,
    male_high: &mut f32,
    female_low: &mut f32,
    female_high: &mut f32,
) {
    if *male_low >= *male_high {
        *male_low = (*male_high - 1.0).max(1.0);
    }
    if *female_low >= *female_high {
        *female_low = (*female_high - 1.0).max(1.0);
    }

    if *male_high >= *female_low - MIN_GENDER_GAP_HZ {
        match target_gender {
            Gender::Female => {
                *male_high = *female_low - MIN_GENDER_GAP_HZ;
                if *male_high <= *male_low {
                    *male_high = *male_low + 1.0;
                }
            }
            Gender::Male => {
                *female_low = *male_high + MIN_GENDER_GAP_HZ;
                if *female_low >= *female_high {
                    *female_low = *female_high - 1.0;
                }
            }
        }
    }
}

/// VR-specific configuration overrides.
///
/// When VR mode is active, these values are used instead of the desktop
/// equivalents for frequency ranges, reminder settings, audio devices,
/// and overlay position/size.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VrConfig {
    pub target_gender: Gender,
    pub female_freq_low: f32,
    pub female_freq_high: f32,
    pub male_freq_low: f32,
    pub male_freq_high: f32,
    pub red_duration_seconds: f32,
    pub reminder_tone_freq: f32,
    pub reminder_tone_volume: f32,
    pub vr_x: Option<i32>,
    pub vr_y: Option<i32>,
    pub vr_width: Option<f32>,
    pub vr_height: Option<f32>,
    pub input_device_name: String,
    pub output_device_name: String,
}

impl Default for VrConfig {
    fn default() -> Self {
        Self {
            target_gender: Gender::Female,
            female_freq_low: 165.0,
            female_freq_high: 255.0,
            male_freq_low: 85.0,
            male_freq_high: 155.0,
            red_duration_seconds: 0.5,
            reminder_tone_freq: 165.0,
            reminder_tone_volume: 0.5,
            vr_x: None,
            vr_y: None,
            vr_width: None,
            vr_height: None,
            input_device_name: String::new(),
            output_device_name: String::new(),
        }
    }
}

impl VrConfig {
    /// Creates a VR config by copying relevant fields from the desktop config.
    pub fn from_desktop(config: &Config) -> Self {
        Self {
            target_gender: config.target_gender,
            female_freq_low: config.female_freq_low,
            female_freq_high: config.female_freq_high,
            male_freq_low: config.male_freq_low,
            male_freq_high: config.male_freq_high,
            red_duration_seconds: config.red_duration_seconds,
            reminder_tone_freq: config.reminder_tone_freq,
            reminder_tone_volume: config.reminder_tone_volume,
            vr_x: config.window_x,
            vr_y: config.window_y,
            vr_width: config.window_width,
            vr_height: config.window_height,
            input_device_name: config.input_device_name.clone(),
            output_device_name: config.output_device_name.clone(),
        }
    }

    /// Enforces validity constraints on the VR frequency ranges.
    pub fn fix_overlap(&mut self) {
        fix_freq_overlap(
            self.target_gender,
            &mut self.male_freq_low,
            &mut self.male_freq_high,
            &mut self.female_freq_low,
            &mut self.female_freq_high,
        );
    }

    /// Returns the frequency range for the VR target gender as (low, high) in Hz.
    pub fn target_range(&self) -> (f32, f32) {
        match self.target_gender {
            Gender::Female => (self.female_freq_low, self.female_freq_high),
            Gender::Male => (self.male_freq_low, self.male_freq_high),
        }
    }
}

/// Application configuration persisted as TOML in the user's home directory.
///
/// All fields have sensible defaults derived from academic research on
/// voice fundamental frequency (F0) ranges. Default frequency ranges:
/// - Male:   85-155 Hz (Titze 1989; Gelfer & Schofield 2000; ASHA guidelines)
/// - Female: 165-255 Hz (same sources)
///
/// A mandatory 10 Hz gap between male_freq_high and female_freq_low is enforced
/// at all times, reflecting the perceptual boundary zone (155-165 Hz) where
/// listeners cannot reliably assign a gender to the voice.
///
/// # Example
///
/// ```no_run
/// let config = pitchbrick::config::Config::default();
/// assert_eq!(config.target_gender, pitchbrick::config::Gender::Female);
/// assert_eq!(config.female_freq_low, 165.0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The user's selected target gender for vocal training.
    pub target_gender: Gender,
    /// Lower bound of the female voice frequency range in Hz.
    pub female_freq_low: f32,
    /// Upper bound of the female voice frequency range in Hz.
    pub female_freq_high: f32,
    /// Lower bound of the male voice frequency range in Hz.
    pub male_freq_low: f32,
    /// Upper bound of the male voice frequency range in Hz.
    pub male_freq_high: f32,
    /// Seconds the display must remain RED before the reminder tone starts.
    pub red_duration_seconds: f32,
    /// Frequency of the reminder tone in Hz (range: 100-4000 Hz).
    pub reminder_tone_freq: f32,
    /// Volume of the reminder tone (range: 0.0-1.0).
    pub reminder_tone_volume: f32,
    /// Saved window X position in screen pixels.
    pub window_x: Option<i32>,
    /// Saved window Y position in screen pixels.
    pub window_y: Option<i32>,
    /// Saved window width in pixels.
    pub window_width: Option<f32>,
    /// Saved window height in pixels.
    pub window_height: Option<f32>,
    /// Name of the selected audio input device, or empty for system default.
    pub input_device_name: String,
    /// Name of the selected audio output device, or empty for system default.
    pub output_device_name: String,
    /// Whether the SteamVR overlay is enabled (requires vr-overlay feature at compile time).
    pub vr_overlay_enabled: bool,
    /// Whether VR-specific settings override desktop settings when the overlay is active.
    pub vr_specific_settings: bool,
    /// VR-specific configuration overrides. Created on first toggle of vr_specific_settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vr: Option<VrConfig>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_gender: Gender::Female,
            female_freq_low: 165.0,
            female_freq_high: 255.0,
            male_freq_low: 85.0,
            male_freq_high: 155.0,
            red_duration_seconds: 0.5,
            reminder_tone_freq: 165.0,
            reminder_tone_volume: 0.5,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            input_device_name: String::new(),
            output_device_name: String::new(),
            vr_overlay_enabled: true,
            vr_specific_settings: false,
            vr: None,
        }
    }
}

impl Config {
    /// Returns the filesystem path to the configuration file.
    ///
    /// The config file is stored at `~/pitchbrick.toml`.
    ///
    /// # Panics
    ///
    /// Panics if the user's home directory cannot be determined.
    pub fn path() -> PathBuf {
        dirs::home_dir()
            .expect("Could not determine home directory")
            .join("pitchbrick.toml")
    }

    /// Loads the configuration from disk, creating a default file if none exists.
    ///
    /// After loading, validates and fixes any overlapping frequency ranges.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TOML configuration file.
    ///
    /// # Returns
    ///
    /// The loaded (and possibly corrected) configuration, or a default config
    /// if the file does not exist or cannot be parsed.
    pub fn load(path: &Path) -> Config {
        if !path.exists() {
            let config = Config::default();
            config.save(path);
            return config;
        }

        match std::fs::read_to_string(path) {
            Ok(contents) => {
                let mut config: Config = toml::from_str(&contents).unwrap_or_else(|e| {
                    tracing::warn!("Failed to parse config, using defaults: {}", e);
                    Config::default()
                });
                config.fix_overlap();
                if let Some(ref mut vr) = config.vr {
                    vr.fix_overlap();
                }
                config
            }
            Err(e) => {
                tracing::warn!("Failed to read config file, using defaults: {}", e);
                Config::default()
            }
        }
    }

    /// Saves the configuration to disk as TOML.
    ///
    /// Writes to a temporary file first, then renames for atomicity.
    ///
    /// # Arguments
    ///
    /// * `path` - Destination path for the TOML file.
    pub fn save(&self, path: &Path) {
        let toml_str = toml::to_string_pretty(self).unwrap_or_else(|e| {
            tracing::error!("Failed to serialize config: {}", e);
            String::new()
        });

        if toml_str.is_empty() {
            return;
        }

        let tmp_path = path.with_extension("toml.tmp");
        if let Err(e) = std::fs::write(&tmp_path, &toml_str) {
            tracing::error!("Failed to write temp config: {}", e);
            return;
        }

        if let Err(e) = std::fs::rename(&tmp_path, path) {
            tracing::error!("Failed to rename temp config: {}", e);
            let _ = std::fs::remove_file(&tmp_path);
        }
    }

    /// Enforces validity constraints on all frequency ranges:
    ///
    /// 1. Each range's low bound is clamped below its high bound (min 1 Hz apart).
    /// 2. `male_freq_high` and `female_freq_low` are always separated by at least
    ///    `MIN_GENDER_GAP_HZ` (10 Hz).  When they are too close, the boundary of
    ///    the **non-target** gender is moved to restore the gap, preserving the
    ///    range the user is actively training toward.
    ///
    /// This is called on every config load and save so invalid states can never
    /// persist to disk.
    pub fn fix_overlap(&mut self) {
        fix_freq_overlap(
            self.target_gender,
            &mut self.male_freq_low,
            &mut self.male_freq_high,
            &mut self.female_freq_low,
            &mut self.female_freq_high,
        );
    }

    /// Returns the frequency range for the user's target gender as (low, high) in Hz.
    pub fn target_range(&self) -> (f32, f32) {
        match self.target_gender {
            Gender::Female => (self.female_freq_low, self.female_freq_high),
            Gender::Male => (self.male_freq_low, self.male_freq_high),
        }
    }

    /// Returns true when VR mode is active: overlay enabled, VR-specific settings
    /// toggled on, and VR config exists.
    pub fn is_vr_mode(&self) -> bool {
        self.vr_overlay_enabled && self.vr_specific_settings && self.vr.is_some()
    }

    /// Returns the target gender for the active mode (VR or desktop).
    pub fn effective_target_gender(&self) -> Gender {
        if self.is_vr_mode() {
            self.vr.as_ref().unwrap().target_gender
        } else {
            self.target_gender
        }
    }

    /// Returns the target frequency range for the active mode.
    pub fn effective_target_range(&self) -> (f32, f32) {
        if self.is_vr_mode() {
            self.vr.as_ref().unwrap().target_range()
        } else {
            self.target_range()
        }
    }

    /// Returns the red duration for the active mode.
    pub fn effective_red_duration(&self) -> f32 {
        if self.is_vr_mode() {
            self.vr.as_ref().unwrap().red_duration_seconds
        } else {
            self.red_duration_seconds
        }
    }

    /// Returns the reminder tone frequency for the active mode.
    pub fn effective_reminder_tone_freq(&self) -> f32 {
        if self.is_vr_mode() {
            self.vr.as_ref().unwrap().reminder_tone_freq
        } else {
            self.reminder_tone_freq
        }
    }

    /// Returns the reminder tone volume for the active mode.
    pub fn effective_reminder_tone_volume(&self) -> f32 {
        if self.is_vr_mode() {
            self.vr.as_ref().unwrap().reminder_tone_volume
        } else {
            self.reminder_tone_volume
        }
    }

    /// Returns the input device name for the active mode.
    pub fn effective_input_device(&self) -> &str {
        if self.is_vr_mode() {
            &self.vr.as_ref().unwrap().input_device_name
        } else {
            &self.input_device_name
        }
    }

    /// Returns the output device name for the active mode.
    pub fn effective_output_device(&self) -> &str {
        if self.is_vr_mode() {
            &self.vr.as_ref().unwrap().output_device_name
        } else {
            &self.output_device_name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = Config::default();
        assert_eq!(config.target_gender, Gender::Female);
        assert_eq!(config.female_freq_low, 165.0);
        assert_eq!(config.female_freq_high, 255.0);
        assert_eq!(config.male_freq_low, 85.0);
        assert_eq!(config.male_freq_high, 155.0);
        assert_eq!(config.red_duration_seconds, 0.5);
        assert_eq!(config.reminder_tone_freq, 165.0);
        assert!(!config.vr_specific_settings);
        assert!(config.vr.is_none());
    }

    #[test]
    fn test_toml_round_trip() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config.target_gender, deserialized.target_gender);
        assert_eq!(config.female_freq_low, deserialized.female_freq_low);
        assert_eq!(config.male_freq_high, deserialized.male_freq_high);
    }

    #[test]
    fn test_fix_overlap_female_target() {
        // male_freq_high too close to female_freq_low: male ceiling should be pushed down.
        let mut config = Config {
            target_gender: Gender::Female,
            female_freq_low: 165.0,
            male_freq_high: 160.0, // only 5 Hz gap — below the 10 Hz minimum
            ..Config::default()
        };
        config.fix_overlap();
        assert!(config.male_freq_high < config.female_freq_low);
        assert_eq!(config.male_freq_high, 155.0); // pushed to female_low - 10
    }

    #[test]
    fn test_fix_overlap_male_target() {
        // female_freq_low too close to male_freq_high: female floor should be pushed up.
        let mut config = Config {
            target_gender: Gender::Male,
            male_freq_high: 155.0,
            female_freq_low: 160.0, // only 5 Hz gap — below the 10 Hz minimum
            ..Config::default()
        };
        config.fix_overlap();
        assert!(config.female_freq_low > config.male_freq_high);
        assert_eq!(config.female_freq_low, 165.0); // pushed to male_high + 10
    }

    #[test]
    fn test_fix_overlap_no_change_needed() {
        // Default config already has a valid gap — nothing should move.
        let mut config = Config::default();
        let high_before = config.male_freq_high;
        let low_before = config.female_freq_low;
        config.fix_overlap();
        assert_eq!(config.male_freq_high, high_before);
        assert_eq!(config.female_freq_low, low_before);
    }

    #[test]
    fn test_gender_toggle() {
        assert_eq!(Gender::Female.toggle(), Gender::Male);
        assert_eq!(Gender::Male.toggle(), Gender::Female);
    }

    #[test]
    fn test_target_range() {
        let mut config = Config::default();
        assert_eq!(config.target_range(), (165.0, 255.0));
        config.target_gender = Gender::Male;
        assert_eq!(config.target_range(), (85.0, 155.0));
    }

    #[test]
    fn test_vr_config_from_desktop() {
        let config = Config::default();
        let vr = VrConfig::from_desktop(&config);
        assert_eq!(vr.target_gender, config.target_gender);
        assert_eq!(vr.female_freq_low, config.female_freq_low);
        assert_eq!(vr.red_duration_seconds, config.red_duration_seconds);
    }

    #[test]
    fn test_is_vr_mode() {
        let mut config = Config::default();
        assert!(!config.is_vr_mode());

        config.vr_overlay_enabled = true;
        config.vr_specific_settings = true;
        assert!(!config.is_vr_mode()); // vr is None

        config.vr = Some(VrConfig::default());
        assert!(config.is_vr_mode());

        config.vr_overlay_enabled = false;
        assert!(!config.is_vr_mode());
    }

    #[test]
    fn test_effective_settings_desktop() {
        let config = Config::default();
        assert_eq!(config.effective_target_gender(), Gender::Female);
        assert_eq!(config.effective_target_range(), (165.0, 255.0));
        assert_eq!(config.effective_red_duration(), 0.5);
    }

    #[test]
    fn test_effective_settings_vr_mode() {
        let mut config = Config::default();
        config.vr_overlay_enabled = true;
        config.vr_specific_settings = true;
        config.vr = Some(VrConfig {
            target_gender: Gender::Male,
            red_duration_seconds: 2.0,
            ..VrConfig::default()
        });
        assert_eq!(config.effective_target_gender(), Gender::Male);
        assert_eq!(config.effective_target_range(), (85.0, 155.0));
        assert_eq!(config.effective_red_duration(), 2.0);
    }

    #[test]
    fn test_vr_config_fix_overlap() {
        let mut vr = VrConfig {
            target_gender: Gender::Female,
            female_freq_low: 165.0,
            male_freq_high: 160.0,
            ..VrConfig::default()
        };
        vr.fix_overlap();
        assert_eq!(vr.male_freq_high, 155.0);
    }

    #[test]
    fn test_toml_round_trip_with_vr() {
        let mut config = Config::default();
        config.vr_specific_settings = true;
        config.vr = Some(VrConfig {
            target_gender: Gender::Male,
            ..VrConfig::default()
        });
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert!(deserialized.vr.is_some());
        assert_eq!(deserialized.vr.unwrap().target_gender, Gender::Male);
    }

    #[test]
    fn test_toml_no_vr_section_when_none() {
        let config = Config::default();
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(!serialized.contains("[vr]"));
    }
}
