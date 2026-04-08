/// Iced application state and event handling for PitchBrick.
///
/// Contains the main application struct, message enum, and the update/view/subscription
/// functions that drive the GUI event loop.
use crate::audio;
use crate::config::{self, Config, Gender};
use crate::tray::{self, TrayCommand, TrayMenuIds, TrayState, UpdateMenuState};
use crate::ui::display::{lerp_color, DisplayCanvas, DisplayState};
use crate::ui::settings_window::{FreqHandle, SettingsState};
use crate::update::{self, UpdateCheckResult};
use iced::widget::canvas::Canvas;
use iced::window;
use iced::{Color, Element, Length, Size, Subscription, Task, Theme};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::MenuId;


/// All possible messages (events) in the PitchBrick application.
#[derive(Debug, Clone)]
pub enum Message {
    /// Animation and timing tick (~60fps).
    Tick(Instant),
    /// User toggled the target gender.
    ToggleGender,
    /// User requested to open the config file in an editor.
    OpenSettings,
    /// User selected a different input (microphone) device.
    SelectInputDevice(String),
    /// User selected a different output (speaker) device.
    SelectOutputDevice(String),
    /// The window was moved to a new position (includes window ID for drag support).
    WindowMoved(window::Id, iced::Point),
    /// The window was resized (includes window ID for drag support).
    WindowResized(window::Id, iced::Size),
    /// User clicked a window to drag it (carries the window ID so the handler
    /// can verify it's the main window and not an auxiliary one).
    DragWindow(window::Id),
    /// A tray menu item was clicked.
    TrayMenuEvent(MenuId),
    /// Application quit requested from the tray menu.
    QuitRequested,
    /// The verbose log window was opened; ID already pre-assigned but message
    /// completes the open task.
    LogWindowOpened(window::Id),
    /// User toggled the VR overlay on/off from the tray menu.
    ToggleVrOverlay,
    /// User toggled VR-specific settings on/off from the tray menu.
    ToggleVrSpecificSettings,
    /// User toggled "Start with Windows" autostart on/off from the tray menu.
    ToggleAutostart,
    /// User clicked the "Written by Lexi" footer to open the Patreon page.
    OpenPatreon,
    /// User accepted the update — run cargo install and exit.
    AcceptUpdate,
    /// User declined the update — close window, record version.
    DeclineUpdate,
    /// The update notification window was opened.
    UpdateWindowOpened(window::Id),
    /// Manual "Check for updates" triggered from tray menu.
    CheckForUpdates,
    /// Open the crates.io page for PitchBrick.
    OpenCratesPage,
    /// User accepted updating the Start Menu shortcut.
    AcceptShortcutUpdate,
    /// User declined the Start Menu shortcut update (don't ask again).
    DeclineShortcutUpdate,
    /// The shortcut mismatch dialog window was opened.
    ShortcutWindowOpened(window::Id),
    /// The settings window was opened.
    SettingsWindowOpened(window::Id),
    /// A window's close button was clicked.
    WindowCloseRequested(window::Id),

    // ── Settings: top row ──
    /// Check for updates button in settings.
    SettingsCheckForUpdates,
    /// Toggle autostart checkbox in settings.
    SettingsToggleAutostart,

    // ── Settings: desktop column ──
    /// Toggle target gender in desktop settings.
    SettingsToggleGender,
    /// A frequency handle was dragged (desktop).
    SettingsFreqChanged { handle: FreqHandle, value: f32 },
    /// Reminder frequency slider changed (desktop).
    SettingsReminderFreqChanged(f32),
    /// Reminder frequency slider released (desktop).
    SettingsReminderFreqReleased,
    /// Red duration slider changed (desktop).
    SettingsRedDurationChanged(f32),
    /// Reminder volume slider changed (desktop).
    SettingsReminderVolumeChanged(f32),
    /// Reminder volume slider released (desktop).
    SettingsReminderVolumeReleased,
    /// Mic sensitivity slider changed (desktop).
    SettingsMicSensitivityChanged(f32),
    /// Input device selected in desktop settings.
    SettingsSelectInputDevice(String),
    /// Output device selected in desktop settings.
    SettingsSelectOutputDevice(String),

    // ── Settings: VR column ──
    /// Toggle VR-specific settings enabled.
    SettingsToggleVrEnabled,
    /// Toggle target gender in VR settings.
    SettingsVrToggleGender,
    /// A frequency handle was dragged (VR).
    SettingsVrFreqChanged { handle: FreqHandle, value: f32 },
    /// VR reminder frequency slider changed.
    SettingsVrReminderFreqChanged(f32),
    /// VR reminder frequency slider released.
    SettingsVrReminderFreqReleased,
    /// VR red duration slider changed.
    SettingsVrRedDurationChanged(f32),
    /// VR reminder volume slider changed.
    SettingsVrReminderVolumeChanged(f32),
    /// VR reminder volume slider released.
    SettingsVrReminderVolumeReleased,
    /// VR mic sensitivity slider changed.
    SettingsVrMicSensitivityChanged(f32),
    /// VR input device selected.
    SettingsVrSelectInputDevice(String),
    /// VR output device selected.
    SettingsVrSelectOutputDevice(String),
    /// VR FOV overlay was dragged to a new position.
    SettingsVrFovDragged { x: i32, y: i32 },

    /// User selected a vocal rest threshold from the tray submenu.
    /// 0 means OFF, otherwise minutes per hour.
    SetVocalRestMinutes(u32),

    /// User toggled Push-to-Talk on Green from the tray menu.
    TogglePttOnGreen,
    /// User acknowledged the first-time PTT explanation dialog.
    AcknowledgePttDialog,
    /// The PTT explanation dialog window was opened.
    PttDialogWindowOpened(window::Id),
    /// User changed the PTT key in settings.
    SettingsPttKeyChanged(String),
    /// User toggled the SteamVR Overlay checkbox in settings.
    SettingsToggleVrOverlay,
    /// User acknowledged the first-time VR settings warning dialog.
    AcknowledgeVrDialog,
    /// The VR explanation dialog window was opened.
    VrDialogWindowOpened(window::Id),

    /// Discards a task result with no side effect.
    Noop,
}

/// Main application state for PitchBrick.
pub struct PitchBrick {
    /// User configuration (persisted to ~/pitchbrick.toml).
    pub config: Config,
    /// Active microphone capture stream (None if no device available).
    pub audio_capture: Option<audio::capture::AudioCapture>,
    /// Active reminder tone output stream (None if no device available).
    pub reminder_tone: Option<audio::playback::ReminderTone>,
    /// Shared ring buffer between the capture thread and analysis worker.
    pub audio_buffer: Arc<Mutex<VecDeque<f32>>>,
    /// Background thread that drains the audio buffer and runs the FFT analyzer.
    pub analysis_worker: audio::analysis::AnalysisWorker,
    /// Most recently detected fundamental frequency in Hz.
    pub detected_freq: Option<f32>,
    /// Current interpolated display color.
    pub current_color: Color,
    /// Color at the start of the current transition (for linear interpolation).
    pub from_color: Color,
    /// Target color being transitioned toward.
    pub target_color: Color,
    /// When the current color transition started.
    pub color_transition_start: Instant,
    /// Current display state (Green/Red/Black).
    pub display_state: DisplayState,
    /// When the display entered the Red state (for reminder tone timing).
    pub red_since: Option<Instant>,
    /// Names of available input (microphone) devices.
    pub input_devices: Vec<String>,
    /// Names of available output (speaker) devices.
    pub output_devices: Vec<String>,
    /// Last window change time for debounced config save (500ms).
    pub save_timer: Option<Instant>,
    /// Audio sample rate from the capture device (or 48000 fallback).
    pub sample_rate: u32,
    /// The main window ID (pre-assigned from window::open at startup).
    pub window_id: Option<window::Id>,
    /// Last known modification time of the config file (for change detection).
    pub config_last_modified: Option<std::time::SystemTime>,
    /// When we last checked the config file for external changes.
    pub last_config_check: Instant,
    /// Channel sender for sending commands to the tray icon thread.
    pub tray_command_tx: std::sync::mpsc::Sender<TrayCommand>,
    /// Shared mapping of tray menu item IDs to actions.
    pub tray_menu_ids: Arc<Mutex<TrayMenuIds>>,
    /// Accumulated log lines displayed in the log window (verbose mode only).
    pub log_lines: Vec<String>,
    /// Window ID of the verbose log window (pre-assigned from window::open).
    pub log_window_id: Option<window::Id>,
    /// Channel receiver for incoming log lines (verbose mode only).
    pub log_rx: Option<std::sync::mpsc::Receiver<String>>,
    /// Command sender for the VR overlay thread (None if VR unavailable or disabled).
    #[cfg(feature = "vr-overlay")]
    pub vr_overlay_tx: Option<std::sync::mpsc::Sender<crate::vr::VrOverlayCommand>>,
    /// Whether VR mode is currently active.
    pub vr_mode_active: bool,
    /// Receiver for a pending background update check result.
    pub update_check_rx: Option<mpsc::Receiver<UpdateCheckResult>>,
    /// Window ID of the update notification dialog.
    pub update_window_id: Option<window::Id>,
    /// The version string of an available update (set when check finds one).
    pub update_available_version: Option<String>,
    /// Whether the config file was freshly created on this launch.
    #[allow(dead_code)]
    pub config_was_newly_created: bool,
    /// When the last manual update check was initiated (5s rate limit).
    pub last_update_check_time: Option<Instant>,
    /// Current state of the tray "Check for updates" menu item.
    pub update_menu_state: UpdateMenuState,
    /// Timer for reverting transient menu states back to Ready.
    pub no_updates_timer: Option<Instant>,
    /// Window ID of the Start Menu shortcut mismatch dialog.
    pub shortcut_window_id: Option<window::Id>,
    /// Old target path when a shortcut mismatch was detected.
    pub shortcut_mismatch_old_path: Option<String>,
    /// Window ID of the settings window (None when not open).
    pub settings_window_id: Option<window::Id>,
    /// Transient UI state for the settings window (None when not open).
    pub settings_state: Option<SettingsState>,
    /// Dynamic minimum noise floor shared with the analysis worker thread.
    pub min_noise_floor: Arc<AtomicU32>,
    /// Vocal rest rolling window tracker.
    pub vocal_rest: crate::vocal_rest::VocalRestTracker,
    /// Rodio output stream handle for playing the vocal rest WAV.
    /// The `_stream` must be kept alive for the handle to remain valid.
    pub rest_sound_stream: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    /// Last time we rebuilt the tray menu for vocal rest display updates.
    pub last_vocal_rest_tray_update: Instant,
    /// Whether the raw (pre-overage) display state was Green on the previous tick.
    pub was_raw_green: bool,
    /// The most recent raw display state (before vocal rest override).
    pub raw_display_state: DisplayState,
    /// Whether the PTT key is currently being held down.
    pub ptt_held: bool,
    /// When the PTT key was first pressed (for 500ms minimum activation).
    pub ptt_press_start: Option<Instant>,
    /// When the PTT release timer started (100ms silence grace period).
    pub ptt_release_timer: Option<Instant>,
    /// Window ID of the PTT first-time explanation dialog.
    pub ptt_dialog_window_id: Option<window::Id>,
    /// Window ID of the VR first-time warning dialog.
    pub vr_dialog_window_id: Option<window::Id>,
    /// Monotonic counter incremented on every config/device change that
    /// affects the settings view. Used as the `lazy` cache key so the
    /// settings widget tree is only rebuilt when something actually changed,
    /// not on every 16 ms tick.
    pub settings_version: u64,
    /// Timestamp of the last Tick message, used to detect slow frames.
    pub last_tick: Instant,
}

impl PitchBrick {
    /// Creates the initial application state and opens the main (and optionally
    /// log) window via `window::open` tasks.
    ///
    /// In verbose mode a second 900×350 log window is opened alongside the main
    /// colour-indicator window. Both window IDs are pre-assigned synchronously
    /// from the `window::open` return tuple.
    pub fn new(
        config: Config,
        config_is_new: bool,
        verbose: bool,
        log_rx: Option<std::sync::mpsc::Receiver<String>>,
        main_size: Size,
        main_position: window::Position,
    ) -> (Self, Task<Message>) {
        let audio_buffer = Arc::new(Mutex::new(VecDeque::new()));

        let effective_input = config.effective_input_device();
        let input_device = audio::devices::find_input_device(effective_input);
        let (audio_capture, sample_rate) = match input_device {
            Some(ref dev) => {
                match audio::capture::AudioCapture::new(dev, audio_buffer.clone()) {
                    Ok(capture) => {
                        let sr = capture.sample_rate;
                        (Some(capture), sr)
                    }
                    Err(e) => {
                        tracing::error!("Failed to create audio capture: {}", e);
                        (None, 48000)
                    }
                }
            }
            None => {
                tracing::error!("No input audio device available");
                (None, 48000)
            }
        };

        let effective_output = config.effective_output_device();
        let output_device = audio::devices::find_output_device(effective_output);
        let reminder_tone = match output_device {
            Some(ref dev) => {
                match audio::playback::ReminderTone::new(
                    dev,
                    config.effective_reminder_tone_freq(),
                    config.effective_reminder_tone_volume(),
                ) {
                    Ok(tone) => Some(tone),
                    Err(e) => {
                        tracing::error!("Failed to create reminder tone: {}", e);
                        None
                    }
                }
            }
            None => {
                tracing::error!("No output audio device available");
                None
            }
        };

        let rest_sound_stream = match rodio::OutputStream::try_default() {
            Ok((stream, handle)) => Some((stream, handle)),
            Err(e) => {
                tracing::error!("Failed to create rodio output stream: {}", e);
                None
            }
        };

        let input_devices = audio::devices::enumerate_input_devices();
        let output_devices = audio::devices::enumerate_output_devices();

        tracing::info!(
            "Audio: capture={}, tone={}, inputs={}, outputs={}",
            audio_capture.is_some(),
            reminder_tone.is_some(),
            input_devices.len(),
            output_devices.len()
        );

        let now = Instant::now();
        let initial_color = DisplayState::Black.color();
        let initial_noise_floor =
            config::mic_sensitivity_to_noise_floor(config.effective_mic_sensitivity());
        let min_noise_floor = Arc::new(AtomicU32::new(initial_noise_floor.to_bits()));
        let analysis_worker = audio::analysis::AnalysisWorker::spawn(
            audio_buffer.clone(),
            sample_rate,
            min_noise_floor.clone(),
        );

        let config_last_modified = std::fs::metadata(Config::path())
            .ok()
            .and_then(|m| m.modified().ok());

        let selected_input = if config.effective_input_device().is_empty() {
            audio::devices::default_input_display_name().unwrap_or_default()
        } else {
            config.effective_input_device().to_string()
        };
        let selected_output = if config.effective_output_device().is_empty() {
            audio::devices::default_output_display_name().unwrap_or_default()
        } else {
            config.effective_output_device().to_string()
        };

        let vr_mode_active = config.is_vr_mode();

        // Determine initial update check state and spawn background check if due.
        let should_check = config_is_new || config.is_update_check_due();

        crate::autostart::sync_autostart(config.autostart);

        let vocal_rest = crate::vocal_rest::VocalRestTracker::load(
            &crate::vocal_rest::VocalRestTracker::path(),
        );

        let (tray_command_tx, tray_menu_ids) = tray::spawn_tray_thread(
            config.effective_target_gender(),
            input_devices.clone(),
            output_devices.clone(),
            selected_input,
            selected_output,
            config.vr_overlay_enabled,
            config.vr_specific_settings,
            config.vocal_rest_minutes,
            vocal_rest.accumulated_ms(now) / 1000,
            config.ptt_on_green,
        );

        let update_check_rx = if should_check {
            Some(update::spawn_update_check(
                config.update_last_checked_version.clone(),
                config_is_new,
            ))
        } else {
            None
        };

        #[cfg(feature = "vr-overlay")]
        let vr_overlay_tx = if config.vr_overlay_enabled {
            let vr = config.vr.as_ref();
            crate::vr::spawn_vr_overlay_thread(
                vr.and_then(|v| v.vr_x),
                vr.and_then(|v| v.vr_y),
                vr.and_then(|v| v.vr_width),
                vr.and_then(|v| v.vr_height),
            )
        } else {
            None
        };

        // Open the main borderless always-on-top window.
        let (main_id, open_main) = window::open(window::Settings {
            size: main_size,
            position: main_position,
            resizable: true,
            decorations: false,
            level: window::Level::AlwaysOnTop,
            ..Default::default()
        });

        // Open the verbose log window if requested.
        let (log_window_id, open_log_task) = if verbose {
            let (log_id, open_log) = window::open(window::Settings {
                size: Size::new(900.0, 350.0),
                resizable: true,
                decorations: true,
                exit_on_close_request: false,
                ..Default::default()
            });
            (Some(log_id), open_log.map(Message::LogWindowOpened))
        } else {
            (None, Task::none())
        };

        let mut state = PitchBrick {
            config,
            audio_capture,
            reminder_tone,
            audio_buffer,
            analysis_worker,
            detected_freq: None,
            current_color: initial_color,
            from_color: initial_color,
            target_color: initial_color,
            color_transition_start: now,
            display_state: DisplayState::Black,
            red_since: None,
            input_devices,
            output_devices,
            save_timer: None,
            sample_rate,
            window_id: Some(main_id),
            config_last_modified,
            last_config_check: now,
            tray_command_tx,
            tray_menu_ids,
            log_lines: Vec::new(),
            log_window_id,
            log_rx,
            #[cfg(feature = "vr-overlay")]
            vr_overlay_tx,
            vr_mode_active,
            update_check_rx,
            update_window_id: None,
            update_available_version: None,
            config_was_newly_created: config_is_new,
            last_update_check_time: if should_check { Some(now) } else { None },
            update_menu_state: if should_check {
                UpdateMenuState::Checking
            } else {
                UpdateMenuState::Ready
            },
            no_updates_timer: None,
            shortcut_window_id: None,
            shortcut_mismatch_old_path: None,
            settings_window_id: None,
            settings_state: None,
            min_noise_floor,
            vocal_rest,
            rest_sound_stream,
            last_vocal_rest_tray_update: now,
            was_raw_green: false,
            raw_display_state: DisplayState::Black,
            ptt_held: false,
            ptt_press_start: None,
            ptt_release_timer: None,
            ptt_dialog_window_id: None,
            vr_dialog_window_id: None,
            settings_version: 0,
            last_tick: Instant::now(),
        };

        // Check Start Menu shortcut and open dialog if mismatched.
        let shortcut_task = match crate::shortcut::check_and_create_shortcut() {
            crate::shortcut::ShortcutCheckResult::Created => {
                tracing::info!("Start Menu shortcut created");
                Task::none()
            }
            crate::shortcut::ShortcutCheckResult::AlreadyCorrect => Task::none(),
            crate::shortcut::ShortcutCheckResult::Mismatched(old_path) => {
                if state.config.start_menu_shortcut_declined {
                    Task::none()
                } else {
                    tracing::info!("Start Menu shortcut points to different binary: {}", old_path);
                    state.shortcut_mismatch_old_path = Some(old_path);
                    let (win_id, open_task) = window::open(window::Settings {
                        size: Size::new(460.0, 180.0),
                        resizable: false,
                        decorations: true,
                        exit_on_close_request: true,
                        ..Default::default()
                    });
                    state.shortcut_window_id = Some(win_id);
                    open_task.map(Message::ShortcutWindowOpened)
                }
            }
            crate::shortcut::ShortcutCheckResult::Failed(e) => {
                tracing::warn!("Start Menu shortcut check failed: {}", e);
                Task::none()
            }
        };

        let init_task = Task::batch([
            open_main.map(|_| Message::Noop),
            open_log_task,
            shortcut_task,
        ]);

        (state, init_task)
    }

    /// Handles all application messages and returns side-effect tasks.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        // Log all settings messages with precise timestamps for latency diagnosis.
        match &message {
            Message::SettingsToggleGender
            | Message::SettingsCheckForUpdates
            | Message::SettingsToggleAutostart => {
                tracing::debug!("[settings] {:?} received t={:?}", message, Instant::now());
            }
            Message::SettingsFreqChanged { handle, value } => {
                tracing::debug!("[settings] FreqChanged({:?}, {:.1}) t={:?}", handle, value, Instant::now());
            }
            Message::SettingsReminderFreqChanged(v) => {
                tracing::debug!("[settings] ReminderFreq={:.1} t={:?}", v, Instant::now());
            }
            Message::SettingsRedDurationChanged(v) => {
                tracing::debug!("[settings] RedDuration={:.1} t={:?}", v, Instant::now());
            }
            Message::SettingsReminderVolumeChanged(v) => {
                tracing::debug!("[settings] ReminderVol={:.2} t={:?}", v, Instant::now());
            }
            Message::SettingsMicSensitivityChanged(v) => {
                tracing::debug!("[settings] MicSens={:.0} t={:?}", v, Instant::now());
            }
            Message::SettingsSelectInputDevice(name) => {
                tracing::debug!("[settings] InputDevice={} t={:?}", name, Instant::now());
            }
            Message::SettingsSelectOutputDevice(name) => {
                tracing::debug!("[settings] OutputDevice={} t={:?}", name, Instant::now());
            }
            Message::SettingsPttKeyChanged(key) => {
                tracing::debug!("[settings] PttKey={} t={:?}", key, Instant::now());
            }
            Message::SettingsVrFreqChanged { handle, value } => {
                tracing::debug!("[settings] VrFreqChanged({:?}, {:.1}) t={:?}", handle, value, Instant::now());
            }
            Message::SettingsVrFovDragged { x, y } => {
                tracing::debug!("[settings] VrFovDragged({}, {}) t={:?}", x, y, Instant::now());
            }
            Message::SettingsVrReminderFreqChanged(v) => {
                tracing::debug!("[settings] VrReminderFreq={:.1} t={:?}", v, Instant::now());
            }
            Message::SettingsVrRedDurationChanged(v) => {
                tracing::debug!("[settings] VrRedDuration={:.1} t={:?}", v, Instant::now());
            }
            Message::SettingsVrReminderVolumeChanged(v) => {
                tracing::debug!("[settings] VrReminderVol={:.2} t={:?}", v, Instant::now());
            }
            Message::SettingsVrMicSensitivityChanged(v) => {
                tracing::debug!("[settings] VrMicSens={:.0} t={:?}", v, Instant::now());
            }
            Message::SettingsVrToggleGender
            | Message::SettingsToggleVrEnabled
            | Message::SettingsToggleVrOverlay => {
                tracing::debug!("[settings] {:?} received t={:?}", message, Instant::now());
            }
            Message::DragWindow(_) => {
                tracing::debug!("[settings] DragWindow received t={:?}", Instant::now());
            }
            _ => {}
        }
        match message {
            Message::Tick(now) => {
                // --- Slow frame detection ---
                let frame_dt = now.duration_since(self.last_tick);
                if frame_dt > Duration::from_millis(32) {
                    tracing::debug!("[perf] slow frame: {:.1}ms t={:?}", frame_dt.as_secs_f64() * 1000.0, now);
                }
                self.last_tick = now;

                // --- Tray menu event polling ---
                if let Ok(event) = tray::menu_event_receiver().try_recv() {
                    return self.update(Message::TrayMenuEvent(event.id));
                }

                // --- Audio analysis (results produced by background worker thread) ---
                if let Some(last) = self.analysis_worker.latest_result() {
                    self.detected_freq = last;
                    if last.is_some() {
                        tracing::debug!("result: {:?}", last);
                    }

                    // Classify raw display state (before vocal rest override).
                    let raw_state = match last {
                        Some(freq) if (85.0..=350.0).contains(&freq) => {
                            let (low, high) = self.config.effective_target_range();
                            if freq >= low && freq <= high {
                                DisplayState::Green
                            } else {
                                let good_direction = match self.config.effective_target_gender() {
                                    Gender::Female => freq > high,
                                    Gender::Male => freq < low,
                                };
                                if good_direction {
                                    DisplayState::Green
                                } else {
                                    DisplayState::Red
                                }
                            }
                        }
                        Some(freq) => {
                            tracing::debug!(
                                "Out of human range: {:.1} Hz (speech window 85-350 Hz)",
                                freq
                            );
                            DisplayState::Black
                        }
                        None => DisplayState::Black,
                    };
                    self.raw_display_state = raw_state;

                    // --- Vocal rest: track green enter/exit ---
                    let is_raw_green = raw_state == DisplayState::Green;
                    if is_raw_green && !self.was_raw_green {
                        self.vocal_rest.on_green_enter(now);
                    } else if !is_raw_green && self.was_raw_green {
                        self.vocal_rest.on_green_exit(now);
                    }
                    self.was_raw_green = is_raw_green;

                    // Prune spans older than 1 hour and update overage state.
                    self.vocal_rest.prune_old_spans();
                    let just_entered_overage = self
                        .vocal_rest
                        .update_overage(now, self.config.vocal_rest_minutes);

                    // Play rest sound on overage entry and every 60s while in green.
                    if just_entered_overage {
                        tracing::info!(
                            "Vocal rest: entered overage (accumulated={}ms, threshold={} min, stream={})",
                            self.vocal_rest.accumulated_ms(now),
                            self.config.vocal_rest_minutes,
                            self.rest_sound_stream.is_some()
                        );
                    }
                    if self.vocal_rest.in_overage
                        && is_raw_green
                        && self.vocal_rest.should_play_rest_sound(now)
                    {
                        if let Some((_, ref handle)) = self.rest_sound_stream {
                            let cursor = std::io::Cursor::new(crate::vocal_rest::REST_WAV);
                            match handle.play_once(cursor) {
                                Ok(_sink) => {
                                    tracing::info!(
                                        "Vocal rest: playing rest sound ({} bytes WAV)",
                                        crate::vocal_rest::REST_WAV.len()
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("Vocal rest: failed to play rest sound: {}", e);
                                }
                            }
                        } else {
                            tracing::warn!("Vocal rest: no output stream available for rest sound");
                        }
                    }

                    // Override Green → Yellow when in overage.
                    let new_state = if raw_state == DisplayState::Green
                        && self.vocal_rest.in_overage
                    {
                        DisplayState::Yellow
                    } else {
                        raw_state
                    };

                    if new_state != self.display_state {
                        tracing::debug!(
                            "State {:?} -> {:?} (freq: {:?})",
                            self.display_state,
                            new_state,
                            last
                        );
                        self.from_color = self.current_color;
                        self.display_state = new_state;
                        self.target_color = new_state.color();
                        self.color_transition_start = now;
                        let tray_state = match new_state {
                            DisplayState::Green => TrayState::Green,
                            DisplayState::Yellow => TrayState::Yellow,
                            DisplayState::Red => TrayState::Red,
                            DisplayState::Black => TrayState::Inactive,
                        };
                        let _ = self.tray_command_tx.send(TrayCommand::SetState(tray_state));

                        #[cfg(feature = "vr-overlay")]
                        if let Some(ref tx) = self.vr_overlay_tx {
                            // VR overlay resting color is green normally,
                            // yellow during vocal rest overage.
                            let rgba = match new_state {
                                DisplayState::Red => [0xF4, 0x43, 0x36, 0xFF],
                                DisplayState::Yellow => [0xFF, 0xEB, 0x3B, 0xFF],
                                _ if self.vocal_rest.in_overage => {
                                    [0xFF, 0xEB, 0x3B, 0xFF]
                                }
                                _ => [0x4C, 0xAF, 0x50, 0xFF],
                            };
                            let _ = tx.send(crate::vr::VrOverlayCommand::SetColor(rgba));
                        }
                    }
                }

                // --- Reminder tone timing ---
                match self.display_state {
                    DisplayState::Red => {
                        if self.red_since.is_none() {
                            self.red_since = Some(now);
                        }
                        if let Some(red_start) = self.red_since {
                            let elapsed = now.duration_since(red_start).as_secs_f32();
                            if elapsed >= self.config.effective_red_duration() {
                                if self.vocal_rest.in_overage {
                                    // Suppress red tone; show rest tooltip instead.
                                    if self.vocal_rest.should_show_red_tooltip(now) {
                                        let _ = self.tray_command_tx.send(TrayCommand::ShowBalloon {
                                            title: "PitchBrick".to_string(),
                                            message: "You have trained enough for this hour, time to give your voice a rest.\n\nYou can disable this setting in the menu.".to_string(),
                                        });
                                    }
                                } else if let Some(ref tone) = self.reminder_tone {
                                    if !tone.is_playing() {
                                        tone.start();
                                        tracing::info!("Reminder tone started");
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        if self.red_since.is_some() {
                            self.red_since = None;
                            if let Some(ref tone) = self.reminder_tone {
                                if tone.is_playing() {
                                    tone.stop();
                                    tracing::info!("Reminder tone stopped");
                                }
                            }
                        }
                    }
                }

                // --- Push-to-Talk on Green ---
                {
                    let ptt_enabled =
                        self.config.ptt_on_green && self.config.ptt_dialog_shown;

                    if ptt_enabled {
                        // Alert condition: Red for >= red_duration (tone fires or tooltip shown).
                        let alert_condition_met = self.display_state == DisplayState::Red
                            && self.red_since.is_some_and(|rs| {
                                now.duration_since(rs).as_secs_f32()
                                    >= self.config.effective_red_duration()
                            });

                        // Determine if PTT should be active based on raw pitch state.
                        // Green = hold, Red = grace period (hold until alert), Black = silence.
                        let want_active = match self.raw_display_state {
                            DisplayState::Green => true,
                            DisplayState::Red
                                if self.ptt_held && !alert_condition_met =>
                            {
                                true
                            }
                            _ => false,
                        };

                        if alert_condition_met && self.ptt_held {
                            // Alert fired → instant release, overrides minimum activation.
                            if let Some(vk) =
                                crate::ptt::key_name_to_vk(&self.config.ptt_key)
                            {
                                crate::ptt::release_key(vk);
                                tracing::debug!("PTT: released (alert condition met)");
                            }
                            self.ptt_held = false;
                            self.ptt_press_start = None;
                            self.ptt_release_timer = None;
                        } else if want_active {
                            if !self.ptt_held {
                                if let Some(vk) =
                                    crate::ptt::key_name_to_vk(&self.config.ptt_key)
                                {
                                    crate::ptt::press_key(vk);
                                    tracing::debug!("PTT: pressed");
                                }
                                self.ptt_held = true;
                                self.ptt_press_start = Some(now);
                            }
                            self.ptt_release_timer = None;
                        } else if self.ptt_held {
                            // Want to release: apply 100ms silence grace + 500ms minimum.
                            if self.ptt_release_timer.is_none() {
                                self.ptt_release_timer = Some(now);
                            }
                            let silence_grace_met = self.ptt_release_timer.is_some_and(
                                |t| {
                                    now.duration_since(t)
                                        >= Duration::from_millis(100)
                                },
                            );
                            let min_activation_met =
                                self.ptt_press_start.is_none_or(|t| {
                                    now.duration_since(t)
                                        >= Duration::from_millis(500)
                                });

                            if silence_grace_met && min_activation_met {
                                if let Some(vk) =
                                    crate::ptt::key_name_to_vk(&self.config.ptt_key)
                                {
                                    crate::ptt::release_key(vk);
                                    tracing::debug!("PTT: released (silence grace expired)");
                                }
                                self.ptt_held = false;
                                self.ptt_press_start = None;
                                self.ptt_release_timer = None;
                            }
                        }
                    } else if self.ptt_held {
                        // PTT was disabled while held → release immediately.
                        if let Some(vk) =
                            crate::ptt::key_name_to_vk(&self.config.ptt_key)
                        {
                            crate::ptt::release_key(vk);
                            tracing::debug!("PTT: released (feature disabled)");
                        }
                        self.ptt_held = false;
                        self.ptt_press_start = None;
                        self.ptt_release_timer = None;
                    }
                }

                // --- Color interpolation (150ms linear fade) ---
                let t = (now
                    .duration_since(self.color_transition_start)
                    .as_secs_f32()
                    / 0.15)
                    .min(1.0);
                self.current_color = lerp_color(self.from_color, self.target_color, t);

                // --- Vocal rest: periodic tooltip update (every 5s) and disk flush (every 2min) ---
                // We update the tooltip rather than rebuilding the menu, because
                // `tray.set_menu()` closes any open context menu on Windows,
                // which would make the vocal rest submenu impossible to use.
                if self.config.vocal_rest_minutes > 0
                    && now.duration_since(self.last_vocal_rest_tray_update)
                        >= Duration::from_secs(5)
                {
                    self.last_vocal_rest_tray_update = now;
                    let _ =
                        self.tray_command_tx.send(TrayCommand::UpdateTooltip {
                            vocal_rest_trained_secs: self.vocal_rest.accumulated_ms(
                                Instant::now(),
                            )
                                / 1000,
                        });
                }
                self.vocal_rest
                    .flush_if_due(&crate::vocal_rest::VocalRestTracker::path(), now);

                // --- Debounced config save (500ms after last window/settings change) ---
                if let Some(save_time) = self.save_timer {
                    if now.duration_since(save_time).as_millis() >= 500 {
                        self.save_timer = None;
                        let config_path = Config::path();
                        self.config.save(&config_path);
                        self.config_last_modified = std::fs::metadata(&config_path)
                            .ok()
                            .and_then(|m| m.modified().ok());
                    }
                }

                // --- Config file hot-reload (poll every ~500ms) ---
                if now.duration_since(self.last_config_check).as_millis() >= 500 {
                    self.last_config_check = now;
                    let config_path = Config::path();
                    if let Ok(metadata) = std::fs::metadata(&config_path) {
                        if let Ok(modified) = metadata.modified() {
                            let changed = match self.config_last_modified {
                                Some(prev) => modified != prev,
                                None => false,
                            };
                            if changed {
                                self.config_last_modified = Some(modified);
                                let (mut new_config, _) = Config::load(&config_path);
                                new_config.fix_overlap();
                                if let Some(ref mut vr) = new_config.vr {
                                    vr.fix_overlap();
                                }
                                new_config.save(&config_path);
                                self.config_last_modified = std::fs::metadata(&config_path)
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                                self.config = new_config;
                                if let Some(ref tone) = self.reminder_tone {
                                    tone.set_frequency(self.config.effective_reminder_tone_freq());
                                    tone.set_volume(self.config.effective_reminder_tone_volume());
                                }
                                self.handle_vr_mode_transition();
                                self.send_tray_rebuild();
                                tracing::info!("Config hot-reloaded from disk");
                            }
                        }
                    }
                }

                // --- Update check: poll background result ---
                let mut update_task = Task::none();
                if let Some(ref rx) = self.update_check_rx {
                    if let Ok(result) = rx.try_recv() {
                        self.update_check_rx = None;
                        match result {
                            UpdateCheckResult::Available(version) => {
                                self.update_available_version = Some(version.clone());
                                self.update_menu_state = UpdateMenuState::Available(version.clone());
                                let _ = self.tray_command_tx.send(TrayCommand::SetUpdateMenuState(
                                    UpdateMenuState::Available(version),
                                ));
                                self.send_tray_rebuild();
                                // Save check date and version.
                                self.config.update_last_checked_date = Some(Config::today_iso());
                                self.config.update_last_checked_version =
                                    self.update_available_version.clone();
                                self.config.save(&Config::path());
                                self.config_last_modified = std::fs::metadata(Config::path())
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                                // Open the update notification window.
                                let (win_id, open_task) = window::open(window::Settings {
                                    size: Size::new(340.0, 160.0),
                                    resizable: false,
                                    decorations: true,
                                    exit_on_close_request: true,
                                    ..Default::default()
                                });
                                self.update_window_id = Some(win_id);
                                update_task = open_task.map(Message::UpdateWindowOpened);
                            }
                            UpdateCheckResult::UpToDate => {
                                self.update_menu_state = UpdateMenuState::NoUpdates;
                                let _ = self.tray_command_tx.send(TrayCommand::SetUpdateMenuState(
                                    UpdateMenuState::NoUpdates,
                                ));
                                self.send_tray_rebuild();
                                self.no_updates_timer = Some(now);
                                // Save check date.
                                self.config.update_last_checked_date = Some(Config::today_iso());
                                self.config.save(&Config::path());
                                self.config_last_modified = std::fs::metadata(Config::path())
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                            }
                            UpdateCheckResult::Failed => {
                                self.update_menu_state = UpdateMenuState::NetworkError;
                                let _ = self.tray_command_tx.send(TrayCommand::SetUpdateMenuState(
                                    UpdateMenuState::NetworkError,
                                ));
                                self.send_tray_rebuild();
                                self.no_updates_timer = Some(now);
                                // Don't save date — retry next time.
                            }
                        }
                    }
                }

                // --- Update menu state revert timer ---
                if let Some(timer_start) = self.no_updates_timer {
                    let elapsed = now.duration_since(timer_start);
                    let timeout = match self.update_menu_state {
                        UpdateMenuState::NetworkError => Duration::from_secs(15),
                        _ => Duration::from_secs(30),
                    };
                    if elapsed >= timeout {
                        self.no_updates_timer = None;
                        self.update_menu_state = UpdateMenuState::Ready;
                        let _ = self
                            .tray_command_tx
                            .send(TrayCommand::SetUpdateMenuState(UpdateMenuState::Ready));
                        self.send_tray_rebuild();
                    }
                }

                // --- Log window: drain incoming lines and snap to bottom ---
                let mut log_updated = false;
                if let Some(ref rx) = self.log_rx {
                    while let Ok(line) = rx.try_recv() {
                        self.log_lines.push(line);
                        log_updated = true;
                    }
                }
                let log_task = if log_updated {
                    iced::widget::operation::snap_to(
                        crate::ui::log_window::scroll_id(),
                        iced::widget::operation::RelativeOffset { x: 0.0, y: 1.0 },
                    )
                } else {
                    Task::none()
                };

                // --- Settings window: 5-second debounced save ---
                if let Some(ref mut state) = self.settings_state {
                    if state.dirty {
                        if let Some(last) = state.last_change {
                            if now.duration_since(last) >= Duration::from_secs(5) {
                                state.dirty = false;
                                state.last_change = None;
                                self.config.save(&Config::path());
                                self.config_last_modified = std::fs::metadata(Config::path())
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                            }
                        }
                    }
                }

                Task::batch([update_task, log_task])
            }
            Message::DragWindow(id) => {
                // Only drag the main window, not settings/log/dialog windows.
                if Some(id) == self.window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::ToggleGender => {
                self.config.target_gender = self.config.target_gender.toggle();
                self.config.fix_overlap();
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("Gender toggled to {}", self.config.target_gender);
                self.send_tray_rebuild();
                Task::none()
            }
            Message::OpenSettings => {
                if let Some(id) = self.settings_window_id {
                    return window::gain_focus(id);
                }
                let (win_id, open_task) = window::open(window::Settings {
                    size: Size::new(800.0, 600.0),
                    position: window::Position::Centered,
                    resizable: false,
                    decorations: true,
                    exit_on_close_request: false,
                    ..Default::default()
                });
                self.settings_window_id = Some(win_id);
                self.settings_state = Some(SettingsState::new());
                open_task.map(Message::SettingsWindowOpened)
            }
            Message::SelectInputDevice(name) => {
                self.config.input_device_name = name.clone();
                self.audio_capture = None;
                let device = audio::devices::find_input_device(&name);
                if let Some(ref dev) = device {
                    match audio::capture::AudioCapture::new(dev, self.audio_buffer.clone()) {
                        Ok(capture) => {
                            self.sample_rate = capture.sample_rate;
                            self.audio_capture = Some(capture);
                            self.analysis_worker = audio::analysis::AnalysisWorker::spawn(
                                self.audio_buffer.clone(),
                                self.sample_rate,
                                self.min_noise_floor.clone(),
                            );
                            tracing::info!("Input device changed to: {}", name);
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to create audio capture on '{}': {}",
                                name,
                                e
                            );
                        }
                    }
                }
                self.save_timer = Some(Instant::now());
                self.send_tray_rebuild();
                Task::none()
            }
            Message::SelectOutputDevice(name) => {
                self.config.output_device_name = name.clone();
                self.reminder_tone = None;
                let device = audio::devices::find_output_device(&name);
                if let Some(ref dev) = device {
                    match audio::playback::ReminderTone::new(
                        dev,
                        self.config.effective_reminder_tone_freq(),
                        self.config.effective_reminder_tone_volume(),
                    ) {
                        Ok(tone) => {
                            self.reminder_tone = Some(tone);
                            tracing::info!("Output device changed to: {}", name);
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to create reminder tone on '{}': {}",
                                name,
                                e
                            );
                        }
                    }
                }
                self.save_timer = Some(Instant::now());
                self.send_tray_rebuild();
                Task::none()
            }
            Message::WindowMoved(id, point) => {
                if Some(id) == self.window_id {
                    self.config.window_x = Some(point.x as i32);
                    self.config.window_y = Some(point.y as i32);
                    self.save_timer = Some(Instant::now());
                }
                Task::none()
            }
            Message::WindowResized(id, size) => {
                if Some(id) == self.window_id {
                    self.config.window_width = Some(size.width);
                    self.config.window_height = Some(size.height);
                    self.save_timer = Some(Instant::now());
                }
                Task::none()
            }
            Message::TrayMenuEvent(menu_id) => {
                let ids = self.tray_menu_ids.lock().unwrap();
                if menu_id == ids.gender_toggle {
                    drop(ids);
                    return self.update(Message::ToggleGender);
                } else if menu_id == ids.open_config {
                    drop(ids);
                    return self.update(Message::OpenSettings);
                } else if menu_id == ids.vr_overlay_toggle {
                    drop(ids);
                    return self.update(Message::ToggleVrOverlay);
                } else if menu_id == ids.vr_specific_settings_toggle {
                    drop(ids);
                    return self.update(Message::ToggleVrSpecificSettings);
                } else if menu_id == ids.ptt_on_green_toggle {
                    drop(ids);
                    return self.update(Message::TogglePttOnGreen);
                } else if menu_id == ids.patreon {
                    drop(ids);
                    return self.update(Message::OpenPatreon);
                } else if menu_id == ids.quit {
                    drop(ids);
                    return self.update(Message::QuitRequested);
                } else if let Some((_, name)) =
                    ids.input_devices.iter().find(|(id, _)| *id == menu_id)
                {
                    let name = name.clone();
                    drop(ids);
                    return self.update(Message::SelectInputDevice(name));
                } else if let Some((_, name)) =
                    ids.output_devices.iter().find(|(id, _)| *id == menu_id)
                {
                    let name = name.clone();
                    drop(ids);
                    return self.update(Message::SelectOutputDevice(name));
                } else if let Some((_, minutes)) =
                    ids.vocal_rest_items.iter().find(|(id, _)| *id == menu_id)
                {
                    let minutes = *minutes;
                    drop(ids);
                    return self.update(Message::SetVocalRestMinutes(minutes));
                }
                Task::none()
            }
            Message::ToggleVrOverlay => {
                self.config.vr_overlay_enabled = !self.config.vr_overlay_enabled;
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("VR overlay toggled to {}", self.config.vr_overlay_enabled);

                #[cfg(feature = "vr-overlay")]
                {
                    if self.config.vr_overlay_enabled {
                        let vr = self.config.vr.as_ref();
                        self.vr_overlay_tx = crate::vr::spawn_vr_overlay_thread(
                            vr.and_then(|v| v.vr_x),
                            vr.and_then(|v| v.vr_y),
                            vr.and_then(|v| v.vr_width),
                            vr.and_then(|v| v.vr_height),
                        );
                        if let Some(ref tx) = self.vr_overlay_tx {
                            let rgba = match self.display_state {
                                DisplayState::Red => [0xF4, 0x43, 0x36, 0xFF],
                                DisplayState::Yellow => [0xFF, 0xEB, 0x3B, 0xFF],
                                _ if self.vocal_rest.in_overage => {
                                    [0xFF, 0xEB, 0x3B, 0xFF]
                                }
                                _ => [0x4C, 0xAF, 0x50, 0xFF],
                            };
                            let _ = tx.send(crate::vr::VrOverlayCommand::SetColor(rgba));
                        }
                    } else if let Some(tx) = self.vr_overlay_tx.take() {
                        let _ = tx.send(crate::vr::VrOverlayCommand::Quit);
                    }
                }

                self.handle_vr_mode_transition();
                self.send_tray_rebuild();
                Task::none()
            }
            Message::ToggleVrSpecificSettings => {
                self.config.vr_specific_settings = !self.config.vr_specific_settings;

                // Create VR config from desktop on first enable.
                if self.config.vr_specific_settings && self.config.vr.is_none() {
                    self.config.vr = Some(crate::config::VrConfig::from_desktop(&self.config));
                    tracing::info!("Created VR config from desktop settings");
                }

                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!(
                    "VR specific settings toggled to {}",
                    self.config.vr_specific_settings
                );
                self.handle_vr_mode_transition();
                self.send_tray_rebuild();

                if self.config.vr_specific_settings && !self.config.vr_dialog_shown {
                    let (_win_id, open_task) = window::open(window::Settings {
                        size: Size::new(440.0, 300.0),
                        resizable: false,
                        decorations: true,
                        exit_on_close_request: false,
                        ..Default::default()
                    });
                    return open_task.map(Message::VrDialogWindowOpened);
                }

                Task::none()
            }
            Message::ToggleAutostart => {
                self.config.autostart = !self.config.autostart;
                crate::autostart::sync_autostart(self.config.autostart);
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("Autostart toggled to {}", self.config.autostart);
                self.send_tray_rebuild();
                Task::none()
            }
            Message::SetVocalRestMinutes(minutes) => {
                self.config.vocal_rest_minutes = minutes;
                let config_path = Config::path();
                self.config.save(&config_path);
                self.config_last_modified = std::fs::metadata(&config_path)
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("Vocal rest minutes set to {}", minutes);
                self.send_tray_rebuild();
                Task::none()
            }
            Message::TogglePttOnGreen => {
                self.config.ptt_on_green = !self.config.ptt_on_green;

                if self.config.ptt_on_green && !self.config.ptt_dialog_shown {
                    // First time: open explanation dialog, PTT gated until acknowledged.
                    let (win_id, open_task) = window::open(window::Settings {
                        size: Size::new(440.0, 300.0),
                        resizable: false,
                        decorations: true,
                        exit_on_close_request: false,
                        ..Default::default()
                    });
                    self.ptt_dialog_window_id = Some(win_id);
                    self.config.save(&Config::path());
                    self.config_last_modified = std::fs::metadata(Config::path())
                        .ok()
                        .and_then(|m| m.modified().ok());
                    tracing::info!("PTT on Green toggled on (dialog pending)");
                    self.send_tray_rebuild();
                    return open_task.map(Message::PttDialogWindowOpened);
                }

                // Turning off: release key if held.
                if !self.config.ptt_on_green && self.ptt_held {
                    if let Some(vk) = crate::ptt::key_name_to_vk(&self.config.ptt_key) {
                        crate::ptt::release_key(vk);
                    }
                    self.ptt_held = false;
                    self.ptt_press_start = None;
                    self.ptt_release_timer = None;
                }

                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("PTT on Green toggled to {}", self.config.ptt_on_green);
                self.send_tray_rebuild();
                Task::none()
            }
            Message::AcknowledgePttDialog => {
                self.config.ptt_dialog_shown = true;
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("PTT dialog acknowledged, PTT now active");
                if let Some(id) = self.ptt_dialog_window_id.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::PttDialogWindowOpened(id) => {
                self.ptt_dialog_window_id = Some(id);
                window::gain_focus(id)
            }
            Message::AcknowledgeVrDialog => {
                self.config.vr_dialog_shown = true;
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("VR dialog acknowledged");
                if let Some(id) = self.vr_dialog_window_id.take() {
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::VrDialogWindowOpened(id) => {
                self.vr_dialog_window_id = Some(id);
                window::gain_focus(id)
            }
            Message::SettingsPttKeyChanged(key) => {
                // If PTT is currently held, release old key and press new one.
                if self.ptt_held {
                    if let Some(old_vk) = crate::ptt::key_name_to_vk(&self.config.ptt_key) {
                        crate::ptt::release_key(old_vk);
                    }
                    self.config.ptt_key = key;
                    if let Some(new_vk) = crate::ptt::key_name_to_vk(&self.config.ptt_key) {
                        crate::ptt::press_key(new_vk);
                    }
                } else {
                    self.config.ptt_key = key;
                }
                tracing::info!("PTT key changed to '{}'", self.config.ptt_key);
                self.mark_settings_dirty();
                Task::none()
            }
            Message::QuitRequested => {
                // Release PTT key before quitting.
                if self.ptt_held {
                    if let Some(vk) = crate::ptt::key_name_to_vk(&self.config.ptt_key) {
                        crate::ptt::release_key(vk);
                    }
                    self.ptt_held = false;
                }
                // Save any pending settings changes before quitting.
                if let Some(ref state) = self.settings_state {
                    if state.dirty {
                        self.config.save(&Config::path());
                    }
                }
                // Flush vocal rest data before exit.
                self.vocal_rest
                    .flush(&crate::vocal_rest::VocalRestTracker::path());
                #[cfg(feature = "vr-overlay")]
                if let Some(ref tx) = self.vr_overlay_tx {
                    let _ = tx.send(crate::vr::VrOverlayCommand::Quit);
                }
                let _ = self.tray_command_tx.send(TrayCommand::Quit);
                iced::exit()
            }
            Message::LogWindowOpened(id) => {
                // The ID was already set at construction time; this message just
                // completes the open task and can be used to confirm the ID.
                self.log_window_id = Some(id);
                Task::none()
            }
            Message::OpenPatreon => {
                let _ = std::process::Command::new("cmd")
                    .args(["/c", "start", "", "https://www.patreon.com/cw/lexi_bytes"])
                    .spawn();
                Task::none()
            }
            Message::AcceptUpdate => {
                // Save version+date before exiting.
                self.config.update_last_checked_date = Some(Config::today_iso());
                self.config.update_last_checked_version = self.update_available_version.clone();
                self.config.save(&Config::path());
                update::spawn_update_and_exit();
            }
            Message::DeclineUpdate => {
                // Save version+date so we don't re-prompt for this version.
                self.config.update_last_checked_date = Some(Config::today_iso());
                self.config.update_last_checked_version = self.update_available_version.clone();
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                // Close the update window.
                let task = if let Some(id) = self.update_window_id.take() {
                    window::close(id)
                } else {
                    Task::none()
                };
                self.update_available_version = None;
                self.update_menu_state = UpdateMenuState::Ready;
                let _ = self
                    .tray_command_tx
                    .send(TrayCommand::SetUpdateMenuState(UpdateMenuState::Ready));
                self.send_tray_rebuild();
                task
            }
            Message::UpdateWindowOpened(id) => {
                self.update_window_id = Some(id);
                Task::none()
            }
            Message::AcceptShortcutUpdate => {
                if let Err(e) = crate::shortcut::update_shortcut() {
                    tracing::error!("Failed to update Start Menu shortcut: {}", e);
                }
                let task = if let Some(id) = self.shortcut_window_id.take() {
                    window::close(id)
                } else {
                    Task::none()
                };
                self.shortcut_mismatch_old_path = None;
                task
            }
            Message::DeclineShortcutUpdate => {
                self.config.start_menu_shortcut_declined = true;
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                let task = if let Some(id) = self.shortcut_window_id.take() {
                    window::close(id)
                } else {
                    Task::none()
                };
                self.shortcut_mismatch_old_path = None;
                task
            }
            Message::ShortcutWindowOpened(id) => {
                self.shortcut_window_id = Some(id);
                Task::none()
            }
            Message::CheckForUpdates => {
                // Rate limit: ignore if a check is already in progress.
                if self.update_check_rx.is_some() {
                    return Task::none();
                }
                // Rate limit: ignore if last check was <5s ago.
                if let Some(last) = self.last_update_check_time {
                    if Instant::now().duration_since(last) < Duration::from_secs(5) {
                        return Task::none();
                    }
                }
                // Close update window if open.
                let close_task = if let Some(id) = self.update_window_id.take() {
                    self.update_available_version = None;
                    window::close(id)
                } else {
                    Task::none()
                };
                // Spawn a fresh check (no last_observed — force comparison against current).
                self.update_check_rx = Some(update::spawn_update_check(None, false));
                self.update_menu_state = UpdateMenuState::Checking;
                self.last_update_check_time = Some(Instant::now());
                self.no_updates_timer = None;
                let _ = self
                    .tray_command_tx
                    .send(TrayCommand::SetUpdateMenuState(UpdateMenuState::Checking));
                self.send_tray_rebuild();
                close_task
            }
            Message::OpenCratesPage => {
                let _ = std::process::Command::new("cmd")
                    .args([
                        "/c",
                        "start",
                        "",
                        "https://crates.io/crates/pitchbrick",
                    ])
                    .spawn();
                Task::none()
            }
            Message::SettingsWindowOpened(id) => {
                self.settings_window_id = Some(id);
                window::gain_focus(id)
            }
            Message::WindowCloseRequested(id) => {
                // Settings window close
                if Some(id) == self.settings_window_id {
                    if let Some(ref state) = self.settings_state {
                        if state.dirty {
                            self.config.save(&Config::path());
                            self.config_last_modified = std::fs::metadata(Config::path())
                                .ok()
                                .and_then(|m| m.modified().ok());
                        }
                    }
                    self.settings_window_id = None;
                    self.settings_state = None;
                    return window::close(id);
                }
                // Log window close
                if Some(id) == self.log_window_id {
                    self.log_window_id = None;
                    return window::close(id);
                }
                // VR dialog close without acknowledgment: revert toggle
                if Some(id) == self.vr_dialog_window_id {
                    if !self.config.vr_dialog_shown {
                        self.config.vr_specific_settings = false;
                        self.config.save(&Config::path());
                        self.config_last_modified = std::fs::metadata(Config::path())
                            .ok()
                            .and_then(|m| m.modified().ok());
                        self.handle_vr_mode_transition();
                        self.send_tray_rebuild();
                    }
                    self.vr_dialog_window_id = None;
                    return window::close(id);
                }
                // PTT dialog close without acknowledgment: revert toggle
                if Some(id) == self.ptt_dialog_window_id {
                    if !self.config.ptt_dialog_shown {
                        self.config.ptt_on_green = false;
                        self.config.save(&Config::path());
                        self.config_last_modified = std::fs::metadata(Config::path())
                            .ok()
                            .and_then(|m| m.modified().ok());
                        self.send_tray_rebuild();
                    }
                    self.ptt_dialog_window_id = None;
                    return window::close(id);
                }
                Task::none()
            }
            Message::SettingsCheckForUpdates => self.update(Message::CheckForUpdates),
            Message::SettingsToggleAutostart => self.update(Message::ToggleAutostart),
            Message::SettingsToggleGender => {
                self.config.target_gender = self.config.target_gender.toggle();
                self.config.fix_overlap();
                self.mark_settings_dirty();
                self.send_tray_rebuild();
                Task::none()
            }
            Message::SettingsFreqChanged { handle, value } => {
                match handle {
                    FreqHandle::MaleLow => self.config.male_freq_low = value,
                    FreqHandle::MaleHigh => self.config.male_freq_high = value,
                    FreqHandle::FemaleLow => self.config.female_freq_low = value,
                    FreqHandle::FemaleHigh => self.config.female_freq_high = value,
                }
                self.config.fix_overlap();
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsReminderFreqChanged(v) => {
                let v = (v * 10.0).round() / 10.0;
                self.config.reminder_tone_freq = v;
                if let Some(ref tone) = self.reminder_tone {
                    tone.set_frequency(v);
                    if !tone.is_playing() {
                        if let Some(ref mut s) = self.settings_state {
                            s.tone_was_already_playing = false;
                        }
                        tone.start();
                    }
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsReminderFreqReleased => {
                if let Some(ref state) = self.settings_state {
                    if !state.tone_was_already_playing {
                        if let Some(ref tone) = self.reminder_tone {
                            tone.stop();
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsRedDurationChanged(v) => {
                let v = (v * 10.0).round() / 10.0;
                self.config.red_duration_seconds = v;
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsReminderVolumeChanged(v) => {
                let v = (v * 100.0).round() / 100.0;
                self.config.reminder_tone_volume = v;
                if let Some(ref tone) = self.reminder_tone {
                    tone.set_volume(v);
                    if !tone.is_playing() {
                        if let Some(ref mut s) = self.settings_state {
                            s.tone_was_already_playing = false;
                        }
                        tone.start();
                    }
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsReminderVolumeReleased => {
                if let Some(ref state) = self.settings_state {
                    if !state.tone_was_already_playing {
                        if let Some(ref tone) = self.reminder_tone {
                            tone.stop();
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsMicSensitivityChanged(v) => {
                let v = v.round();
                self.config.mic_sensitivity = v;
                let floor = config::mic_sensitivity_to_noise_floor(v);
                self.min_noise_floor
                    .store(floor.to_bits(), Ordering::Relaxed);
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsSelectInputDevice(name) => {
                self.update(Message::SelectInputDevice(name))
            }
            Message::SettingsSelectOutputDevice(name) => {
                self.update(Message::SelectOutputDevice(name))
            }
            Message::SettingsToggleVrEnabled => self.update(Message::ToggleVrSpecificSettings),
            Message::SettingsToggleVrOverlay => self.update(Message::ToggleVrOverlay),
            Message::SettingsVrToggleGender => {
                if let Some(ref mut vr) = self.config.vr {
                    vr.target_gender = vr.target_gender.toggle();
                    vr.fix_overlap();
                }
                self.mark_settings_dirty();
                self.send_tray_rebuild();
                Task::none()
            }
            Message::SettingsVrFreqChanged { handle, value } => {
                if let Some(ref mut vr) = self.config.vr {
                    match handle {
                        FreqHandle::MaleLow => vr.male_freq_low = value,
                        FreqHandle::MaleHigh => vr.male_freq_high = value,
                        FreqHandle::FemaleLow => vr.female_freq_low = value,
                        FreqHandle::FemaleHigh => vr.female_freq_high = value,
                    }
                    vr.fix_overlap();
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrReminderFreqChanged(v) => {
                let v = (v * 10.0).round() / 10.0;
                if let Some(ref mut vr) = self.config.vr {
                    vr.reminder_tone_freq = v;
                }
                if self.config.is_vr_mode() {
                    if let Some(ref tone) = self.reminder_tone {
                        tone.set_frequency(v);
                        if !tone.is_playing() {
                            if let Some(ref mut s) = self.settings_state {
                                s.tone_was_already_playing = false;
                            }
                            tone.start();
                        }
                    }
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrReminderFreqReleased => {
                if self.config.is_vr_mode() {
                    if let Some(ref state) = self.settings_state {
                        if !state.tone_was_already_playing {
                            if let Some(ref tone) = self.reminder_tone {
                                tone.stop();
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsVrRedDurationChanged(v) => {
                let v = (v * 10.0).round() / 10.0;
                if let Some(ref mut vr) = self.config.vr {
                    vr.red_duration_seconds = v;
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrReminderVolumeChanged(v) => {
                let v = (v * 100.0).round() / 100.0;
                if let Some(ref mut vr) = self.config.vr {
                    vr.reminder_tone_volume = v;
                }
                if self.config.is_vr_mode() {
                    if let Some(ref tone) = self.reminder_tone {
                        tone.set_volume(v);
                        if !tone.is_playing() {
                            if let Some(ref mut s) = self.settings_state {
                                s.tone_was_already_playing = false;
                            }
                            tone.start();
                        }
                    }
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrReminderVolumeReleased => {
                if self.config.is_vr_mode() {
                    if let Some(ref state) = self.settings_state {
                        if !state.tone_was_already_playing {
                            if let Some(ref tone) = self.reminder_tone {
                                tone.stop();
                            }
                        }
                    }
                }
                Task::none()
            }
            Message::SettingsVrMicSensitivityChanged(v) => {
                let v = v.round();
                if let Some(ref mut vr) = self.config.vr {
                    vr.mic_sensitivity = v;
                }
                if self.config.is_vr_mode() {
                    let floor = config::mic_sensitivity_to_noise_floor(v);
                    self.min_noise_floor
                        .store(floor.to_bits(), Ordering::Relaxed);
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrSelectInputDevice(name) => {
                if let Some(ref mut vr) = self.config.vr {
                    vr.input_device_name = name.clone();
                }
                if self.config.is_vr_mode() {
                    return self.update(Message::SelectInputDevice(name));
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrSelectOutputDevice(name) => {
                if let Some(ref mut vr) = self.config.vr {
                    vr.output_device_name = name.clone();
                }
                if self.config.is_vr_mode() {
                    return self.update(Message::SelectOutputDevice(name));
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::SettingsVrFovDragged { x, y } => {
                if let Some(ref mut vr) = self.config.vr {
                    vr.vr_x = Some(x);
                    vr.vr_y = Some(y);
                }
                self.mark_settings_dirty();
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    /// Marks the settings state as dirty and records the change time.
    fn mark_settings_dirty(&mut self) {
        if let Some(ref mut s) = self.settings_state {
            s.dirty = true;
            s.last_change = Some(Instant::now());
        }
        self.settings_version = self.settings_version.wrapping_add(1);
    }

    /// Detects VR mode transitions and sends appropriate tray commands.
    ///
    /// When entering VR mode: switches tray icon, recreates audio if VR devices differ.
    /// When leaving VR mode: restores tray icon, recreates audio if desktop devices differ.
    fn handle_vr_mode_transition(&mut self) {
        let new_vr_mode = self.config.is_vr_mode();
        if new_vr_mode == self.vr_mode_active {
            return;
        }

        if new_vr_mode {
            // Entering VR mode.
            self.vr_mode_active = true;
            let _ = self.tray_command_tx.send(TrayCommand::EnterVrMode);
            tracing::info!("Entering VR mode");

            if let Some(ref vr) = self.config.vr {
                // Recreate input device if different.
                if vr.input_device_name != self.config.input_device_name {
                    self.audio_capture = None;
                    let device = audio::devices::find_input_device(&vr.input_device_name);
                    if let Some(ref dev) = device {
                        match audio::capture::AudioCapture::new(dev, self.audio_buffer.clone()) {
                            Ok(capture) => {
                                self.sample_rate = capture.sample_rate;
                                self.audio_capture = Some(capture);
                                self.analysis_worker = audio::analysis::AnalysisWorker::spawn(
                                    self.audio_buffer.clone(),
                                    self.sample_rate,
                                    self.min_noise_floor.clone(),
                                );
                            }
                            Err(e) => tracing::error!("Failed to create VR audio capture: {}", e),
                        }
                    }
                }

                // Recreate output device if different.
                if vr.output_device_name != self.config.output_device_name {
                    self.reminder_tone = None;
                    let device = audio::devices::find_output_device(&vr.output_device_name);
                    if let Some(ref dev) = device {
                        match audio::playback::ReminderTone::new(
                            dev,
                            vr.reminder_tone_freq,
                            vr.reminder_tone_volume,
                        ) {
                            Ok(tone) => self.reminder_tone = Some(tone),
                            Err(e) => tracing::error!("Failed to create VR reminder tone: {}", e),
                        }
                    }
                } else if let Some(ref tone) = self.reminder_tone {
                    tone.set_frequency(vr.reminder_tone_freq);
                    tone.set_volume(vr.reminder_tone_volume);
                }
            }
        } else {
            // Leaving VR mode.
            self.vr_mode_active = false;
            let _ = self.tray_command_tx.send(TrayCommand::LeaveVrMode);
            tracing::info!("Leaving VR mode");

            // Check if we need to switch back to desktop devices.
            let vr_input = self
                .config
                .vr
                .as_ref()
                .map(|v| v.input_device_name.as_str())
                .unwrap_or("");
            let vr_output = self
                .config
                .vr
                .as_ref()
                .map(|v| v.output_device_name.as_str())
                .unwrap_or("");

            if vr_input != self.config.input_device_name {
                self.audio_capture = None;
                let device = audio::devices::find_input_device(&self.config.input_device_name);
                if let Some(ref dev) = device {
                    match audio::capture::AudioCapture::new(dev, self.audio_buffer.clone()) {
                        Ok(capture) => {
                            self.sample_rate = capture.sample_rate;
                            self.audio_capture = Some(capture);
                            self.analysis_worker = audio::analysis::AnalysisWorker::spawn(
                                self.audio_buffer.clone(),
                                self.sample_rate,
                                self.min_noise_floor.clone(),
                            );
                        }
                        Err(e) => tracing::error!("Failed to restore desktop audio capture: {}", e),
                    }
                }
            }

            if vr_output != self.config.output_device_name {
                self.reminder_tone = None;
                let device = audio::devices::find_output_device(&self.config.output_device_name);
                if let Some(ref dev) = device {
                    match audio::playback::ReminderTone::new(
                        dev,
                        self.config.reminder_tone_freq,
                        self.config.reminder_tone_volume,
                    ) {
                        Ok(tone) => self.reminder_tone = Some(tone),
                        Err(e) => tracing::error!("Failed to restore desktop reminder tone: {}", e),
                    }
                }
            } else if let Some(ref tone) = self.reminder_tone {
                tone.set_frequency(self.config.reminder_tone_freq);
                tone.set_volume(self.config.reminder_tone_volume);
            }
        }
    }

    /// Sends a rebuild command to the tray thread with the current state.
    fn send_tray_rebuild(&mut self) {
        self.settings_version = self.settings_version.wrapping_add(1);
        let effective_input = self.config.effective_input_device();
        let selected_input = if effective_input.is_empty() {
            audio::devices::default_input_display_name().unwrap_or_default()
        } else {
            effective_input.to_string()
        };
        let effective_output = self.config.effective_output_device();
        let selected_output = if effective_output.is_empty() {
            audio::devices::default_output_display_name().unwrap_or_default()
        } else {
            effective_output.to_string()
        };

        let _ = self.tray_command_tx.send(TrayCommand::Rebuild {
            gender: self.config.effective_target_gender(),
            input_devices: self.input_devices.clone(),
            output_devices: self.output_devices.clone(),
            selected_input,
            selected_output,
            vr_overlay_enabled: self.config.vr_overlay_enabled,
            vr_specific_settings: self.config.vr_specific_settings,
            vr_mode_active: self.vr_mode_active,
            vocal_rest_minutes: self.config.vocal_rest_minutes,
            vocal_rest_trained_secs: self.vocal_rest.accumulated_ms(Instant::now()) / 1000,
            ptt_on_green: self.config.ptt_on_green,
        });
    }

    /// Returns a per-window title.
    pub fn title(&self, id: window::Id) -> String {
        if Some(id) == self.settings_window_id {
            "PitchBrick - Settings".to_string()
        } else if Some(id) == self.update_window_id {
            "PitchBrick - Update".to_string()
        } else if Some(id) == self.shortcut_window_id {
            "PitchBrick - Shortcut".to_string()
        } else if Some(id) == self.ptt_dialog_window_id {
            "PitchBrick - Push-to-Talk".to_string()
        } else if Some(id) == self.vr_dialog_window_id {
            "PitchBrick - VR Settings".to_string()
        } else if Some(id) == self.log_window_id {
            "PitchBrick - Log".to_string()
        } else {
            "PitchBrick".to_string()
        }
    }

    /// Renders the view for a given window.
    ///
    /// Main window: borderless colour-indicator canvas.
    /// Log window (verbose mode): scrollable monospace log lines.
    pub fn view(&self, id: window::Id) -> Element<'_, Message> {
        if Some(id) == self.settings_window_id {
            return crate::ui::settings_window::view(
                &self.config,
                &self.input_devices,
                &self.output_devices,
                self.config.autostart,
            );
        }
        if Some(id) == self.update_window_id {
            let new_ver = self
                .update_available_version
                .as_deref()
                .unwrap_or("?");
            return crate::ui::update_window::view(new_ver, update::current_version());
        }
        if Some(id) == self.shortcut_window_id {
            let old = self.shortcut_mismatch_old_path.as_deref().unwrap_or("?");
            let current = std::env::current_exe()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "?".into());
            return crate::ui::shortcut_window::view(old, &current);
        }
        if Some(id) == self.ptt_dialog_window_id {
            return crate::ui::ptt_dialog::view();
        }
        if Some(id) == self.vr_dialog_window_id {
            return crate::ui::vr_dialog::view();
        }
        if Some(id) == self.log_window_id {
            crate::ui::log_window::view(&self.log_lines)
        } else {
            let display = DisplayCanvas {
                color: self.current_color,
                detected_freq: self.detected_freq,
            };
            Canvas::new(display)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        }
    }

    /// Returns active subscriptions (event sources).
    pub fn subscription(&self) -> Subscription<Message> {
        let tick =
            iced::time::every(Duration::from_millis(16)).map(|_| Message::Tick(Instant::now()));

        let window_events = iced::event::listen_with(|event, status, id| match event {
            iced::Event::Window(window_event) => match window_event {
                iced::window::Event::Moved(point) => Some(Message::WindowMoved(id, point)),
                iced::window::Event::Resized(size) => Some(Message::WindowResized(id, size)),
                iced::window::Event::CloseRequested => {
                    Some(Message::WindowCloseRequested(id))
                }
                _ => None,
            },
            // Left-click → DragWindow only when no widget captured the event.
            // This prevents spurious update+view cycles when clicking buttons,
            // sliders, or pick-lists in the settings window.
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            )) if status == iced::event::Status::Ignored => {
                tracing::debug!("[input] mouse left pressed (ignored by widgets) t={:?}", Instant::now());
                Some(Message::DragWindow(id))
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Left,
            )) => {
                tracing::debug!("[input] mouse left pressed (captured by widget) t={:?}", Instant::now());
                None
            }
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )) => {
                tracing::debug!("[input] mouse left released t={:?}", Instant::now());
                None
            }
            _ => None,
        });

        Subscription::batch([tick, window_events])
    }

    /// Returns the application theme (Dark for all windows).
    pub fn theme(&self, _id: window::Id) -> Theme {
        Theme::Dark
    }
}
