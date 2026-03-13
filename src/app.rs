/// Iced application state and event handling for PitchBrick.
///
/// Contains the main application struct, message enum, and the update/view/subscription
/// functions that drive the GUI event loop.
use crate::audio;
use crate::config::Config;
use crate::ui::display::{lerp_color, DisplayCanvas, DisplayState};
use iced::widget::canvas::Canvas;
use iced::widget::column;
use iced::window;
use iced::{Color, Element, Length, Subscription, Task, Theme};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// All possible messages (events) in the PitchBrick application.
#[derive(Debug, Clone)]
pub enum Message {
    /// A new frequency analysis result from the audio worker.
    FrequencyDetected(Option<f32>),
    /// An error occurred in the audio subsystem.
    AudioError(String),
    /// Animation and timing tick (~60fps).
    Tick(Instant),
    /// Configuration was changed externally (file watcher).
    ConfigChanged(Config),
    /// User toggled the target gender.
    ToggleGender,
    /// User requested to open the config file in an editor.
    OpenSettings,
    /// User adjusted the reminder tone frequency slider.
    SetReminderFreq(f32),
    /// User adjusted the reminder tone volume slider.
    SetReminderVolume(f32),
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
    /// Debounced config save triggered.
    SaveConfig,
    /// No-operation message for menu root buttons.
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
    /// FFT frequency analyzer for processing captured audio.
    pub analyzer: audio::analysis::FrequencyAnalyzer,
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
    /// The main window ID, captured from the first window event.
    pub window_id: Option<window::Id>,
    /// Last known modification time of the config file (for change detection).
    pub config_last_modified: Option<std::time::SystemTime>,
    /// When we last checked the config file for external changes.
    pub last_config_check: Instant,
}

impl PitchBrick {
    /// Creates the initial application state.
    ///
    /// Enumerates audio devices, creates capture and playback streams,
    /// initializes the frequency analyzer, and returns the state paired
    /// with an initial no-op task.
    pub fn new(config: Config) -> (Self, Task<Message>) {
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
        let analyzer = audio::analysis::FrequencyAnalyzer::new(sample_rate);

        // Snapshot the config file's current mtime to avoid false reload on startup
        let config_last_modified = std::fs::metadata(Config::path())
            .ok()
            .and_then(|m| m.modified().ok());

        let state = PitchBrick {
            config,
            audio_capture,
            reminder_tone,
            audio_buffer,
            analyzer,
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
            window_id: None,
            config_last_modified,
            last_config_check: now,
        };

        (state, Task::none())
    }

    /// Handles all application messages and returns side-effect tasks.
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick(now) => {
                // --- Audio analysis ---
                if let Ok(mut buf) = self.audio_buffer.try_lock() {
                    if !buf.is_empty() {
                        let samples: Vec<f32> = buf.drain(..).collect();
                        self.analyzer.push_samples(&samples);
                    }
                }

                let results = self.analyzer.analyze();
                if let Some(&last) = results.last() {
                    self.detected_freq = last;

                    let new_state = match last {
                        Some(freq) if (65.0..=300.0).contains(&freq) => {
                            let (low, high) = self.config.target_range();
                            if freq >= low && freq <= high {
                                DisplayState::Green
                            } else {
                                DisplayState::Red
                            }
                        }
                        _ => DisplayState::Black,
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

                // --- Color interpolation (1-second linear fade) ---
                let t = now
                    .duration_since(self.color_transition_start)
                    .as_secs_f32()
                    .min(1.0);
                self.current_color = lerp_color(self.from_color, self.target_color, t);

                // --- Debounced config save (500ms after last window/settings change) ---
                if let Some(save_time) = self.save_timer {
                    if now.duration_since(save_time).as_millis() >= 500 {
                        self.save_timer = None;
                        let config_path = Config::path();
                        self.config.save(&config_path);
                        // Update mtime to prevent false reload detection
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
                                // Update mtime after corrective save
                                self.config_last_modified = std::fs::metadata(&config_path)
                                    .ok()
                                    .and_then(|m| m.modified().ok());
                                // Apply new config
                                self.config = new_config;
                                if let Some(ref tone) = self.reminder_tone {
                                    tone.set_frequency(self.config.reminder_tone_freq);
                                    tone.set_volume(self.config.reminder_tone_volume);
                                }
                                tracing::info!("Config hot-reloaded from disk");
                            }
                        }
                    }
                }

                Task::none()
            }
            Message::DragWindow => {
                if let Some(id) = self.window_id {
                    window::drag(id)
                } else {
                    Task::none()
                }
            }
            Message::FrequencyDetected(_freq) => Task::none(),
            Message::AudioError(e) => {
                tracing::error!("Audio error: {}", e);
                Task::none()
            }
            Message::ConfigChanged(new_config) => {
                self.config = new_config;
                if let Some(ref tone) = self.reminder_tone {
                    tone.set_frequency(self.config.reminder_tone_freq);
                    tone.set_volume(self.config.reminder_tone_volume);
                }
                Task::none()
            }
            Message::ToggleGender => {
                self.config.target_gender = self.config.target_gender.toggle();
                self.config.fix_overlap();
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                tracing::info!("Gender toggled to {}", self.config.target_gender);
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
            Message::SetReminderFreq(freq) => {
                self.config.reminder_tone_freq = freq;
                if let Some(ref tone) = self.reminder_tone {
                    tone.set_frequency(freq);
                }
                tracing::debug!("Reminder frequency set to {:.0} Hz", freq);
                self.save_timer = Some(Instant::now());
                Task::none()
            }
            Message::SetReminderVolume(vol) => {
                self.config.reminder_tone_volume = vol;
                if let Some(ref tone) = self.reminder_tone {
                    tone.set_volume(vol);
                }
                tracing::debug!("Reminder volume set to {:.0}%", vol * 100.0);
                self.save_timer = Some(Instant::now());
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
                            self.analyzer = audio::analysis::FrequencyAnalyzer::new(self.sample_rate);
                            tracing::info!("Input device changed to: {}", name);
                        }
                        Err(e) => {
                            tracing::error!("Failed to create audio capture on '{}': {}", name, e);
                        }
                    }
                }
                self.save_timer = Some(Instant::now());
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
                            tracing::error!("Failed to create reminder tone on '{}': {}", name, e);
                        }
                    }
                }
                self.save_timer = Some(Instant::now());
                Task::none()
            }
            Message::WindowMoved(id, point) => {
                self.window_id = Some(id);
                self.config.window_x = Some(point.x as i32);
                self.config.window_y = Some(point.y as i32);
                self.save_timer = Some(Instant::now());
                Task::none()
            }
            Message::WindowResized(id, size) => {
                self.window_id = Some(id);
                self.config.window_width = Some(size.width);
                self.config.window_height = Some(size.height);
                self.save_timer = Some(Instant::now());
                Task::none()
            }
            Message::SaveConfig => {
                self.config.save(&Config::path());
                self.config_last_modified = std::fs::metadata(Config::path())
                    .ok()
                    .and_then(|m| m.modified().ok());
                Task::none()
            }
            Message::Noop => Task::none(),
        }
    }

    /// Renders the application view.
    ///
    /// Composes the menu bar at the top and the color indicator canvas
    /// filling the remaining space. Clicking the canvas triggers window drag.
    pub fn view(&self) -> Element<'_, Message> {
        let menu = crate::ui::menu::build_menu_bar(
            &self.config,
            &self.input_devices,
            &self.output_devices,
        );

        let display = DisplayCanvas {
            color: self.current_color,
        };
        let canvas_widget = Canvas::new(display)
            .width(Length::Fill)
            .height(Length::Fill);

        column![menu, canvas_widget].into()
    }

    /// Returns active subscriptions (event sources).
    ///
    /// Subscribes to a 16ms tick for animations, audio analysis, and config
    /// polling. Also subscribes to window move/resize events for position
    /// persistence and window ID capture (needed for drag support).
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

    /// Returns the application theme.
    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}
