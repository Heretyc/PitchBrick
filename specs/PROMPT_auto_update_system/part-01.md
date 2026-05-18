# Auto-Update System Prompt Part 1

Status: implementation prompt reference; safety-scope question flow is normative.

This file is a split continuation of `PROMPT_auto_update_system.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.

# Auto-Update Checker for Rust GUI Applications (crates.io)

<role>
You are a senior Rust systems engineer implementing a production-ready auto-update system for a Rust GUI application distributed via `cargo install`. You follow Rust idioms, minimize dependencies, and handle all edge cases (network failure, invalid semver, concurrent checks, rate limiting). You do NOT over-engineer — you build exactly what the spec requires.
</role>

<context>
This prompt adds a periodic crates.io version check + user-facing update notification to any Rust application that:
1. Is published on crates.io and installed via `cargo install <crate_name>`
2. Has a TOML config file with serde (de)serialization
3. Has a system tray menu (via `tray-icon` crate on Windows)
4. Uses the Iced GUI framework (0.14+) in daemon/multi-window mode

If your target project differs (no tray, different GUI framework, Linux-only), read the full implementation, and follow `docs/spec/safety-scope.md` structured question flow to clarify, then adapt the integration points accordingly — the core update logic (`update.rs`) is framework-agnostic.
</context>

<constraints>
- Do NOT add more than 3 new dependencies: `ureq` (sync HTTP), `serde_json` (parse API), `semver` (version compare). If the project already has an async HTTP client (e.g. `reqwest`), prefer that over adding `ureq`.
- Do NOT store credentials, tokens, or user-identifiable data. The only outbound request is a GET to the public crates.io API.
- Do NOT auto-update silently. Always show a user-facing prompt with version info and explicit "Update Now" / "Not Now" buttons.
- The update install MUST use a visible console window so the user can see progress.
- Rate limit manual checks to 5 seconds minimum between requests.
- Automatic checks run at most once per 30 days (tracked via config date field).
- Network failures are non-fatal: log a warning, show a transient tray state, retry next launch.
</constraints>

---

## Required Information (fill in before running)

Replace these placeholders with your project's values:

| Placeholder | Description | Example |
|---|---|---|
| `{{CRATE_NAME}}` | Your crate name on crates.io | `pitchbrick` |
| `{{CONFIG_STRUCT}}` | Your existing config struct name | `Config` |
| `{{CONFIG_PATH_FN}}` | Function returning config file path | `Config::path()` |
| `{{CONFIG_SAVE_FN}}` | Method to save config to disk | `self.config.save(&Config::path())` |
| `{{APP_STRUCT}}` | Your main app/state struct | `PitchBrick` |
| `{{MESSAGE_ENUM}}` | Your app's message/event enum | `Message` |
| `{{TRAY_COMMAND_ENUM}}` | Your tray command enum (if applicable) | `TrayCommand` |
| `{{TRAY_MENU_IDS_STRUCT}}` | Your tray menu IDs struct (if applicable) | `TrayMenuIds` |

---

## Implementation Steps

### Step 1: Add Dependencies (`Cargo.toml`)

Add to `[dependencies]`:
```toml
ureq = "2"
serde_json = "1"
semver = "1"
```

### Step 2: Extend Config Struct

Add two optional fields to `{{CONFIG_STRUCT}}`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub update_last_checked_version: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub update_last_checked_date: Option<String>,
```

Both default to `None`. Add to `Default` impl.

Add these helper methods:

```rust
impl {{CONFIG_STRUCT}} {
    /// True if no check date recorded or >30 days ago.
    pub fn is_update_check_due(&self) -> bool {
        match &self.update_last_checked_date {
            None => true,
            Some(date_str) => {
                let today = days_since_epoch();
                let checked = parse_iso_to_days(date_str).unwrap_or(0);
                today.saturating_sub(checked) > 30
            }
        }
    }

    pub fn today_iso() -> String {
        // days-since-epoch → YYYY-MM-DD using Hinnant's algorithm
        // (see reference implementation below)
    }
}
```

Change `load()` return type from `Config` to `(Config, bool)` where bool = config was freshly created (file did not exist). This drives first-run behavior.

**Date helpers** (module-private free functions): `days_since_epoch()`, `parse_iso_to_days(&str)`, `ymd_to_days(y,m,d)`, `days_to_ymd(days)` — all use the Howard Hinnant civil date algorithm. No chrono dependency needed.

### Step 3: Create `src/update.rs` (Core Logic — Framework-Agnostic)

```rust
pub enum UpdateCheckResult {
    Available(String),  // newer version string
    UpToDate,
    Failed,
}

pub fn current_version() -> &'static str { env!("CARGO_PKG_VERSION") }

fn fetch_latest_version() -> Result<String, String> {
    // GET https://crates.io/api/v1/crates/{{CRATE_NAME}}
    // User-Agent: "{{CRATE_NAME}}/{version} (update-check)"
    // Parse JSON → .crate.newest_version
}

pub fn should_prompt(latest: &str, last_observed: Option<&str>, config_is_new: bool) -> bool {
    // Parse both as semver::Version
    // Return false if latest <= current
    // Return true if config_is_new
    // Return true if last_observed is None
    // Return true if last_observed != latest (new version since last check)
    // Return false otherwise (user already saw this version)
}

pub fn spawn_update_check(
    last_observed: Option<String>,
    config_is_new: bool,
) -> mpsc::Receiver<UpdateCheckResult> {
    // Spawn named thread "update-check"
    // Call fetch_latest_version + should_prompt
    // Send result on channel
}

pub fn spawn_update_and_exit() -> ! {
    // Windows: cmd /C "timeout /t 2 && cargo install {{CRATE_NAME}} --force && {{CRATE_NAME}}"
    //   with CREATE_NEW_CONSOLE flag (0x00000010) for visible window
    // Unix: sh -c "sleep 2 && cargo install {{CRATE_NAME}} --force && {{CRATE_NAME}} &"
    // Then process::exit(0)
}
```

**Unit tests for `should_prompt`:**
- Newer version, no observed → true
- Same version → false
- Older version → false
- Already observed same → false
- New config + newer → true
- Different observed + newer → true
- Invalid semver → false

### Step 4: Create Update Notification Window (UI)

A small (340x160) decorated window with:
- Heading: "Update Available"
- Body: "{{CRATE_NAME}} v{new} is available (you have v{current})."
- "View changes" button → opens `https://crates.io/crates/{{CRATE_NAME}}` in default browser
- "Update Now" button → triggers `spawn_update_and_exit()`
- "Not Now" button → closes window, records version+date so user isn't re-prompted

### Step 5: Tray Menu Integration (if applicable)

Add an `UpdateMenuState` enum with 5 states:
