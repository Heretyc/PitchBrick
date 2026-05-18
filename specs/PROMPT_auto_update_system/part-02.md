# Auto-Update System Prompt Part 2

Status: implementation prompt reference; safety-scope question flow is normative.

This file is a split continuation of `PROMPT_auto_update_system.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.


| State | Label | Clickable | Duration |
|---|---|---|---|
| `Ready` | "Check for updates" | Yes | Permanent (default) |
| `Checking` | "Checking..." | No (disabled) | Until check completes |
| `NoUpdates` | "No new updates." | Yes | 30s then → Ready |
| `Available(v)` | "Update available (v{x})" | Yes (opens window) | Until user acts |
| `NetworkError` | "Problem with internet!" | Yes | 15s then → Ready |

Add `SetUpdateMenuState(UpdateMenuState)` variant to `{{TRAY_COMMAND_ENUM}}`.
Add `check_for_updates: MenuId` to `{{TRAY_MENU_IDS_STRUCT}}`.
Place the menu item above the attribution/quit section.

### Step 6: App State Integration

New state fields on `{{APP_STRUCT}}`:
```rust
update_check_rx: Option<mpsc::Receiver<UpdateCheckResult>>,
update_window_id: Option<window::Id>,
update_available_version: Option<String>,
config_was_newly_created: bool,
last_update_check_time: Option<Instant>,  // 5s rate limit
update_menu_state: UpdateMenuState,
no_updates_timer: Option<Instant>,        // revert timer
```

New `{{MESSAGE_ENUM}}` variants:
```rust
AcceptUpdate,
DeclineUpdate,
UpdateWindowOpened(window::Id),
CheckForUpdates,
OpenCratesPage,
```

**Constructor (`new`):**
- Accept `config_is_new: bool` parameter
- If `config_is_new || config.is_update_check_due()` → spawn background check, set menu to `Checking`
- Otherwise → set menu to `Ready`

**Tick handler additions:**
1. Poll `update_check_rx` for results:
   - `Available(v)` → open update window, set menu, save date+version to config
   - `UpToDate` → set menu to `NoUpdates`, start 30s timer, save date
   - `Failed` → set menu to `NetworkError`, start 15s timer (don't save date)
2. Check `no_updates_timer`: revert menu to `Ready` after timeout

**Message handlers:**
- `CheckForUpdates`: rate-limit 5s, close existing update window, spawn fresh check
- `AcceptUpdate`: save config, call `spawn_update_and_exit()`
- `DeclineUpdate`: save version+date, close window, revert menu
- `OpenCratesPage`: open browser to crates.io page
- `UpdateWindowOpened`: store window ID

**View routing:** route update window ID → update_window::view()
**Title routing:** "{{CRATE_NAME}} - Update" for update window

### Step 7: Entry Point (`main.rs`)

- Add `mod update;`
- Change `Config::load()` to `let (config, config_is_new) = Config::load(...)`
- Pass `config_is_new` to app constructor

---

## Verification Checklist

1. `cargo build` / `cargo test` / `cargo clippy` — all pass
2. Delete config file, run → immediate check, config created with update fields
3. Existing config without update fields → check triggered, fields written
4. Recent check date → no automatic check on startup
5. Newer version on crates.io → dialog appears with correct version info
6. "Not Now" → dialog closes, version+date recorded, menu reverts to Ready
7. "Update Now" → visible console window runs cargo install, app exits, new version launches
8. Tray "Check for updates" → Checking → NoUpdates (30s) → Ready
9. Network failure → NetworkError (15s) → Ready
10. Rapid clicks on menu item → silently ignored within 5s window

---

## What NOT To Do

- Do NOT add `chrono` for date math — the Hinnant algorithm in ~20 lines handles it.
- Do NOT poll crates.io more than once per session automatically.
- Do NOT block the GUI thread — all network I/O is on a background thread.
- Do NOT show the update window on every launch if the user already declined this version.
- Do NOT add update-related UI to the main window — it goes in a separate small window.
- Do NOT hardcode version strings — always use `env!("CARGO_PKG_VERSION")`.

---

## Output Format

Produce the implementation as a series of file edits and new files:
1. `Cargo.toml` diff (added dependencies)
2. `src/config.rs` diff (new fields, helpers, load() signature change)
3. `src/update.rs` (complete new file)
4. `src/ui/update_window.rs` (complete new file)
5. `src/ui/mod.rs` diff (add pub mod)
6. `src/tray.rs` diff (UpdateMenuState, SetUpdateMenuState, menu item)
7. `src/main.rs` diff (mod update, config_is_new)
8. `src/app.rs` diff (new state, messages, handlers, view/title routing)

Each file should compile independently after all edits are applied. Run `cargo test` after implementation to verify all existing and new tests pass.
