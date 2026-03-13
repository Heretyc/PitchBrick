/// Iced application state and event handling for PitchBrick.
///
/// Contains the main application struct, message enum, and the update/view/subscription
/// functions that drive the GUI event loop.
use crate::audio;
use crate::config::{Config, Gender};
use crate::tray::{self, TrayCommand, TrayMenuIds, TrayState};
use crate::ui::display::{lerp_color, DisplayCanvas, DisplayState};
use iced::widget::canvas::Canvas;
use iced::window;
use iced::{Color, Element, Length, Size, Subscription, Task, Theme};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
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
    /// User clicked the canvas to drag the window.
    DragWindow,
    /// A tray menu item was clicked.
    TrayMenuEvent(MenuId),
    /// Application quit requested from the tray menu.
    QuitRequested,
    /// The verbose log window was opened; ID already pre-assigned but message
    /// completes the open task.
    LogWindowOpened(window::Id),
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
        verbose: bool,
        log_rx: Option<std::sync::mpsc::Receiver<String>>,
        main_size: Size,
        main_position: window::Position,
    ) -> (Self, Task<Message>) {
        let audio_buffer = Arc::new(Mutex::new(VecDeque::new()));

        let input_device = audio::devices::find_input_device(&config.input_device_name);
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

        let output_device = audio::devices::find_output_device(&config.output_device_name);
        let reminder_tone = match output_device {
            Some(ref dev) => {
                match audio::playback::ReminderTone::new(
                    dev,
                    config.reminder_tone_freq,
                    config.reminder_tone_volume,
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

        let input_devices: Vec<String> = audio::devices::enumerate_input_devices()
            .into_iter()
            .map(|d| d.name)
            .collect();
        let output_devices: Vec<String> = audio::devices::enumerate_output_devices()
            .into_iter()
            .map(|d| d.name)
            .collect();

        tracing::info!(
            "Audio: capture={}, tone={}, inputs={}, outputs={}",
            audio_capture.is_some(),
            reminder_tone.is_some(),
            input_devices.len(),
            output_devices.len()
        );

        let now = Instant::now();
        let initial_color = DisplayState::Black.color();
        let analysis_worker =
            audio::analysis::AnalysisWorker::spawn(audio_buffer.clone(), sample_rate);

        let config_last_modified = std::fs::metadata(Config::path())
            .ok()
            .and_then(|m| m.modified().ok());

        let (tray_command_tx, tray_menu_ids) = tray::spawn_tray_thread(
            config.target_gender,
            input_devices.clone(),
            output_devices.clone(),
            config.input_device_name.clone(),
            config.output_device_name.clone(),
        );

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

        let state = PitchBrick {
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
        };

        let init_task = Task::batch([open_main.map(|_| Message::Noop), open_log_task]);

        (state, init_task)
    }

    /// Handles all application messages and returns side-effect tasks.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick(now) => {
                // --- Tray menu event polling ---
                if let Ok(event) = tray::menu_event_receiver().try_recv() {
                    return self.update(Message::TrayMenuEvent(event.id));
                }

                // --- Audio analysis (results produced by background worker thread) ---
                if let Some(last) = self.analysis_worker.latest_result() {
                    self.detected_freq = last;
                    tracing::debug!("result: {:?}", last);

                    let new_state = match last {
                        Some(freq) if (85.0..=350.0).contains(&freq) => {
                            let (low, high) = self.config.target_range();
                            if freq >= low && freq <= high {
                                DisplayState::Green
                            } else {
                                // Only flag Red when the voice is going the wrong direction.
                                // Going further in the good direction is still Green:
                                //   Female: higher than target is fine (more feminine)
                                //   Male:   lower than target is fine (more masculine)
                                let good_direction = match self.config.target_gender {
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
                            DisplayState::Red => TrayState::Red,
                            DisplayState::Black => TrayState::Inactive,
                        };
                        let _ = self.tray_command_tx.send(TrayCommand::SetState(tray_state));
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
                            if elapsed >= self.config.red_duration_seconds {
                                if let Some(ref tone) = self.reminder_tone {
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

                // --- Color interpolation (150ms linear fade) ---
                let t = (now
                    .duration_since(self.color_transition_start)
                    .as_secs_f32()
                    / 0.15)
                    .min(1.0);
                self.current_color = lerp_color(self.from_color, self.target_color, t);

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
                                let mut new_config = Config::load(&config_path);
                                new_config.fix_overlap();
                                new_config.save(&config_path);
                                self.config_last_modified = std::fs::metadata(&config_path)
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                                self.config = new_config;
                                if let Some(ref tone) = self.reminder_tone {
                                    tone.set_frequency(self.config.reminder_tone_freq);
                                    tone.set_volume(self.config.reminder_tone_volume);
                                }
                                self.send_tray_rebuild();
                                tracing::info!("Config hot-reloaded from disk");
                            }
                        }
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
                if log_updated {
                    iced::widget::operation::snap_to(
                        crate::ui::log_window::scroll_id(),
                        iced::widget::operation::RelativeOffset { x: 0.0, y: 1.0 },
                    )
                } else {
                    Task::none()
                }
            }
            Message::DragWindow => {
                if let Some(id) = self.window_id {
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
                let path = Config::path();
                tracing::info!("Opening config file: {:?}", path);
                if let Err(e) = std::process::Command::new("notepad.exe").arg(&path).spawn() {
                    tracing::error!("Failed to open config in notepad: {}", e);
                }
                Task::none()
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
                        self.config.reminder_tone_freq,
                        self.config.reminder_tone_volume,
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
                // Only save position for the main window, not the log window.
                if Some(id) != self.log_window_id {
                    self.window_id = Some(id);
                    self.config.window_x = Some(point.x as i32);
                    self.config.window_y = Some(point.y as i32);
                    self.save_timer = Some(Instant::now());
                }
                Task::none()
            }
            Message::WindowResized(id, size) => {
                // Only save size for the main window, not the log window.
                if Some(id) != self.log_window_id {
                    self.window_id = Some(id);
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
                }
                Task::none()
            }
            Message::QuitRequested => {
                let _ = self.tray_command_tx.send(TrayCommand::Quit);
                iced::exit()
            }
            Message::LogWindowOpened(id) => {
                // The ID was already set at construction time; this message just
                // completes the open task and can be used to confirm the ID.
                self.log_window_id = Some(id);
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    /// Sends a rebuild command to the tray thread with the current state.
    fn send_tray_rebuild(&self) {
        let _ = self.tray_command_tx.send(TrayCommand::Rebuild {
            gender: self.config.target_gender,
            input_devices: self.input_devices.clone(),
            output_devices: self.output_devices.clone(),
            selected_input: self.config.input_device_name.clone(),
            selected_output: self.config.output_device_name.clone(),
        });
    }

    /// Returns a per-window title.
    pub fn title(&self, id: window::Id) -> String {
        if Some(id) == self.log_window_id {
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

        let window_events = iced::event::listen_with(|event, _status, id| match event {
            iced::Event::Window(window_event) => match window_event {
                iced::window::Event::Moved(point) => Some(Message::WindowMoved(id, point)),
                iced::window::Event::Resized(size) => Some(Message::WindowResized(id, size)),
                _ => None,
            },
            _ => None,
        });

        Subscription::batch([tick, window_events])
    }

    /// Returns the application theme (Dark for all windows).
    pub fn theme(&self, _id: window::Id) -> Theme {
        Theme::Dark
    }
}
