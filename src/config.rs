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

/// Application configuration persisted as TOML in the user's home directory.
///
/// All fields have sensible defaults derived from academic research on
/// voice fundamental frequency (F0) ranges. Default frequency ranges:
/// - Male:   85-155 Hz (Titze 1989; Gelfer & Schofield 2000; ASHA guidelines)
/// - Female: 165-255 Hz (same sources)
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_gender: Gender::Female,
            female_freq_low: 165.0,
            female_freq_high: 255.0,
            male_freq_low: 85.0,
            male_freq_high: 155.0,
            red_duration_seconds: 1.0,
            reminder_tone_freq: 165.0,
            reminder_tone_volume: 0.5,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            input_device_name: String::new(),
            output_device_name: String::new(),
            vr_overlay_enabled: true,
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
        // Step 1: ensure low < high within each range.
        if self.male_freq_low >= self.male_freq_high {
            self.male_freq_low = (self.male_freq_high - 1.0).max(1.0);
        }
        if self.female_freq_low >= self.female_freq_high {
            self.female_freq_low = (self.female_freq_high - 1.0).max(1.0);
        }

        // Step 2: enforce the inter-gender gap.
        if self.male_freq_high >= self.female_freq_low - MIN_GENDER_GAP_HZ {
            match self.target_gender {
                Gender::Female => {
                    // Protect the female range; push the male ceiling down.
                    self.male_freq_high = self.female_freq_low - MIN_GENDER_GAP_HZ;
                    // Don't let male ceiling drop below male floor.
                    if self.male_freq_high <= self.male_freq_low {
                        self.male_freq_high = self.male_freq_low + 1.0;
                    }
                }
                Gender::Male => {
                    // Protect the male range; push the female floor up.
                    self.female_freq_low = self.male_freq_high + MIN_GENDER_GAP_HZ;
                    // Don't let female floor exceed female ceiling.
                    if self.female_freq_low >= self.female_freq_high {
                        self.female_freq_low = self.female_freq_high - 1.0;
                    }
                }
            }
        }
    }

    /// Returns the frequency range for the user's target gender as (low, high) in Hz.
    pub fn target_range(&self) -> (f32, f32) {
        match self.target_gender {
            Gender::Female => (self.female_freq_low, self.female_freq_high),
            Gender::Male => (self.male_freq_low, self.male_freq_high),
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
        assert_eq!(config.red_duration_seconds, 1.0);
        assert_eq!(config.reminder_tone_freq, 165.0);
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
}
