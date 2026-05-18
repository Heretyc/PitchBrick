# Windows Integration Prompt Part 5

Status: archived implementation prompt reference.

This file is a split continuation of `PROMPT_WINDOWS_INTEGRATION.md`.
Current repository policy in `AGENTS.md` supersedes this reference if instructions conflict.

   - Match MenuId to quit → initiate app shutdown, send TrayCommand::Quit.
```

---

## What NOT to Do

<negative_constraints>
- Do NOT use `HKLM` (local machine) registry keys. Everything is per-user (`HKCU`).
- Do NOT use `Task Scheduler` for autostart. The Run key is simpler and sufficient.
- Do NOT bundle or require external `.ico` files for the tray icon. Generate the icon bytes in code.
- Do NOT make the tray icon or shortcut features conditional behind feature flags. They are always compiled on Windows.
- Do NOT use `unwrap()` or `expect()` on registry/COM operations. Log and gracefully degrade.
- Do NOT create the `Menu` on the main thread and send it to the tray thread. `Menu` contains `Rc` and is `!Send`. Build it on the tray thread.
- Do NOT use `std::sync::mpsc::Receiver` in a blocking `recv()` call on the tray thread. Use `try_recv()` inside the `PeekMessageW` polling loop so Win32 messages are also processed.
- Do NOT add a GUI settings window for these features. The tray menu IS the settings interface for autostart. The shortcut dialog is a one-time launch-time check.
- Do NOT add any AI/agent attribution (Co-Authored-By, etc.) to commits.
- Do NOT use `CoInitialize` (the non-Ex variant). Always use `CoInitializeEx` with `COINIT_APARTMENTTHREADED`.
- Do NOT forget to call `CoUninitialize()` on every exit path from COM functions. Use the closure pattern shown in the examples.
- Do NOT use `windows` crate API patterns from versions prior to v0.50. The v0.58 API uses `HSTRING`, `Interface` trait for casting, and returns `WIN32_ERROR` from registry operations.
</negative_constraints>

---

## Output Format

<output_format>
- Produce each module as a **complete, self-contained Rust source file** with all necessary `use` imports at the top of each function or module, ready to be placed in `src/` and registered in `main.rs` via `mod autostart;`, `mod shortcut;`, `mod tray;`.
- For integration points (app initialization and event loop dispatch), provide a clearly commented Rust code block showing the initialization sequence and event loop dispatch logic. Mark adaptation points with `// CUSTOMIZE:` comments explaining what the implementer must change for their specific app.
- Use `// CUSTOMIZE:` as the comment prefix for all app-specific adaptation points (app name, icon color, tooltip text, description strings, additional menu items, dialog mechanism).
- Implementation order: `autostart.rs` first, then `shortcut.rs`, then `tray.rs`, then integration code.
</output_format>

---

## Summary of Config Changes

Add these two fields to your existing config struct with the specified defaults. Both are persisted to the existing configured config file:

| Field | Type | Default | Purpose |
|---|---|---|---|
| `autostart` | `bool` | `true` | Controls the Windows Run registry key |
| `start_menu_shortcut_declined` | `bool` | `false` | Suppresses the shortcut mismatch dialog permanently |

---

## Cargo.toml Dependency Additions

```toml
[dependencies]
dirs = "6"

[target.'cfg(windows)'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_Registry",
    "Win32_UI_Shell",
    "Win32_System_Com",
    "Win32_Storage_FileSystem",
    "Win32_Foundation",
] }
tray-icon = "0.21"
```

If your project already uses the `windows` crate, merge the feature flags into the existing entry — in particular, ensure `Win32_Storage_FileSystem` is present (required for `IPersistFile`). If you already use `tray-icon`, verify the version is compatible (0.21.x uses `MenuEvent::receiver()` as a static method returning `&'static MenuEventReceiver`). The `dirs` crate is only used by the shortcut module to resolve `%APPDATA%` portably, but if your existing config system already resolves that path differently, you may use your existing approach instead.
