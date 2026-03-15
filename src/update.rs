//! Auto-update checker for PitchBrick.
//!
//! Queries the crates.io API in a background thread and compares the latest
//! published version against the currently running binary. Provides a
//! self-update mechanism via `cargo install pitchbrick --force`.

use std::sync::mpsc;

/// Result of a background update check.
#[derive(Debug, Clone)]
pub enum UpdateCheckResult {
    /// A newer version is available on crates.io.
    Available(String),
    /// The running version is already the latest.
    UpToDate,
    /// The network request or parse failed.
    Failed,
}

/// Returns the version of the currently running binary.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Fetches the latest published version of `pitchbrick` from crates.io.
fn fetch_latest_version() -> Result<String, String> {
    let resp = ureq::get("https://crates.io/api/v1/crates/pitchbrick")
        .set("User-Agent", &format!("pitchbrick/{} (update-check)", current_version()))
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let body_str = resp
        .into_string()
        .map_err(|e| format!("Body read failed: {}", e))?;

    let body: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    body["crate"]["newest_version"]
        .as_str()
        .map(|s: &str| s.to_string())
        .ok_or_else(|| "Missing newest_version field".to_string())
}

/// Determines whether the user should be prompted to update.
///
/// Returns true when the latest version from crates.io is strictly newer than
/// the currently installed version, and either:
/// - No previous check has been recorded (`last_observed` is None), or
/// - The config was freshly created (first run), or
/// - The latest version is different from what was previously observed.
pub fn should_prompt(latest: &str, last_observed: Option<&str>, config_is_new: bool) -> bool {
    let Ok(latest_ver) = semver::Version::parse(latest) else {
        return false;
    };
    let Ok(current_ver) = semver::Version::parse(current_version()) else {
        return false;
    };

    if latest_ver <= current_ver {
        return false;
    }

    // Newer version exists on crates.io.
    if config_is_new {
        return true;
    }

    match last_observed {
        None => true,
        Some(observed) => observed != latest,
    }
}

/// Spawns a background thread that checks crates.io and sends the result
/// back via the returned channel receiver.
pub fn spawn_update_check(
    last_observed: Option<String>,
    config_is_new: bool,
) -> mpsc::Receiver<UpdateCheckResult> {
    let (tx, rx) = mpsc::channel();

    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let result = match fetch_latest_version() {
                Ok(latest) => {
                    if should_prompt(&latest, last_observed.as_deref(), config_is_new) {
                        UpdateCheckResult::Available(latest)
                    } else {
                        UpdateCheckResult::UpToDate
                    }
                }
                Err(e) => {
                    tracing::warn!("Update check failed: {}", e);
                    UpdateCheckResult::Failed
                }
            };
            let _ = tx.send(result);
        })
        .ok();

    rx
}

/// Launches a detached process that waits 2 seconds, runs `cargo install
/// pitchbrick --force`, then relaunches PitchBrick. The current process
/// exits immediately so the install can replace the binary.
pub fn spawn_update_and_exit() -> ! {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd")
        .args([
            "/C",
            "timeout /t 2 && cargo install pitchbrick --force && pitchbrick",
        ])
        .creation_flags(0x00000010) // CREATE_NEW_CONSOLE — visible window
        .spawn();

    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_prompt_newer_version_no_observed() {
        // Simulate: latest is newer than current, no previous observation.
        // We can't easily fake current_version(), so we test the logic directly.
        let current = current_version();
        let major: u64 = current.split('.').next().unwrap().parse().unwrap();
        let fake_latest = format!("{}.99.99", major + 1);
        assert!(should_prompt(&fake_latest, None, false));
    }

    #[test]
    fn test_should_prompt_same_version() {
        let current = current_version();
        assert!(!should_prompt(current, None, false));
    }

    #[test]
    fn test_should_prompt_older_version() {
        assert!(!should_prompt("0.0.1", None, false));
    }

    #[test]
    fn test_should_prompt_already_observed() {
        let current = current_version();
        let major: u64 = current.split('.').next().unwrap().parse().unwrap();
        let fake_latest = format!("{}.99.99", major + 1);
        // Already observed this version — don't prompt again.
        assert!(!should_prompt(&fake_latest, Some(&fake_latest), false));
    }

    #[test]
    fn test_should_prompt_new_config() {
        let current = current_version();
        let major: u64 = current.split('.').next().unwrap().parse().unwrap();
        let fake_latest = format!("{}.99.99", major + 1);
        assert!(should_prompt(&fake_latest, Some(&fake_latest), true));
    }

    #[test]
    fn test_should_prompt_different_observed() {
        let current = current_version();
        let major: u64 = current.split('.').next().unwrap().parse().unwrap();
        let fake_latest = format!("{}.99.99", major + 1);
        let old_observed = format!("{}.99.98", major + 1);
        assert!(should_prompt(&fake_latest, Some(&old_observed), false));
    }

    #[test]
    fn test_should_prompt_invalid_semver() {
        assert!(!should_prompt("not-a-version", None, false));
    }
}
