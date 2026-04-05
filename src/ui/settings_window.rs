//! Settings window view and custom canvas widgets.
//!
//! Provides a two-column settings layout (Desktop / VR) with interactive
//! frequency sliders, device pickers, and a VR FOV position editor.

use crate::app::Message;
use crate::config::{Config, Gender};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::widget::{button, checkbox, column, container, pick_list, row, slider, text, Canvas};
use iced::{Alignment, Color, Element, Length, Padding, Rectangle, Renderer, Theme};
use std::time::Instant;

/// Identifies which handle on the frequency slider is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreqHandle {
    MaleLow,
    MaleHigh,
    FemaleLow,
    FemaleHigh,
}

/// Transient UI state for the settings window (not persisted).
pub struct SettingsState {
    /// Whether any config value has changed since the last save.
    pub dirty: bool,
    /// When the last change occurred (for 5-second debounce).
    pub last_change: Option<Instant>,
    /// Whether the reminder tone was already playing when a preview started.
    pub tone_was_already_playing: bool,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            dirty: false,
            last_change: None,
            tone_was_already_playing: false,
        }
    }
}

// ── Frequency Slider Canvas ─────────────────────────────────────────────

/// Internal state for the frequency slider drag interaction.
#[derive(Debug, Default)]
pub struct FreqSliderState {
    pub dragging: Option<FreqHandle>,
}

/// A custom canvas widget that draws a horizontal frequency range slider
/// with 4 draggable handles (male low/high, female low/high).
pub struct FrequencySliderCanvas {
    pub male_low: f32,
    pub male_high: f32,
    pub female_low: f32,
    pub female_high: f32,
    pub target_gender: Gender,
    pub disabled: bool,
    pub is_vr: bool,
}

const FREQ_MIN: f32 = 70.0;
const FREQ_MAX: f32 = 300.0;
const SLIDER_PAD: f32 = 20.0;
const HANDLE_RADIUS: f32 = 6.0;

impl FrequencySliderCanvas {
    fn freq_to_x(&self, freq: f32, width: f32) -> f32 {
        let usable = width - 2.0 * SLIDER_PAD;
        SLIDER_PAD + (freq - FREQ_MIN) / (FREQ_MAX - FREQ_MIN) * usable
    }

    fn x_to_freq(&self, x: f32, width: f32) -> f32 {
        let usable = width - 2.0 * SLIDER_PAD;
        let t = (x - SLIDER_PAD) / usable;
        FREQ_MIN + t.clamp(0.0, 1.0) * (FREQ_MAX - FREQ_MIN)
    }

    fn closest_handle(&self, x: f32, width: f32) -> Option<FreqHandle> {
        let handles = [
            (FreqHandle::MaleLow, self.male_low),
            (FreqHandle::MaleHigh, self.male_high),
            (FreqHandle::FemaleLow, self.female_low),
            (FreqHandle::FemaleHigh, self.female_high),
        ];
        let mut best = None;
        let mut best_dist = 12.0_f32; // max pixel distance to grab
        for (handle, freq) in &handles {
            let hx = self.freq_to_x(*freq, width);
            let dist = (x - hx).abs();
            if dist < best_dist {
                best_dist = dist;
                best = Some(*handle);
            }
        }
        best
    }
}

impl canvas::Program<Message> for FrequencySliderCanvas {
    type State = FreqSliderState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.disabled {
            return None;
        }

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if let Some(handle) = self.closest_handle(pos.x, bounds.width) {
                        state.dragging = Some(handle);
                        return Some(canvas::Action::capture());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(handle) = state.dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let raw = self.x_to_freq(pos.x, bounds.width);
                        let value = (raw * 10.0).round() / 10.0;
                        let msg = if self.is_vr {
                            Message::SettingsVrFreqChanged { handle, value }
                        } else {
                            Message::SettingsFreqChanged { handle, value }
                        };
                        return Some(canvas::Action::publish(msg));
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging.is_some() {
                    state.dragging = None;
                    return Some(canvas::Action::capture());
                }
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let h = bounds.height;
        let w = bounds.width;
        let mid_y = h / 2.0;

        // Track background
        let track_y = mid_y - 2.0;
        frame.fill_rectangle(
            iced::Point::new(SLIDER_PAD, track_y),
            iced::Size::new(w - 2.0 * SLIDER_PAD, 4.0),
            Color::from_rgb(0.3, 0.3, 0.3),
        );

        // Male range fill
        let ml_x = self.freq_to_x(self.male_low, w);
        let mh_x = self.freq_to_x(self.male_high, w);
        let male_color = if self.target_gender == Gender::Male {
            Color::from_rgba(0.0, 0.7, 0.0, 0.4)
        } else {
            Color::from_rgba(0.7, 0.0, 0.0, 0.4)
        };
        frame.fill_rectangle(
            iced::Point::new(ml_x, track_y),
            iced::Size::new(mh_x - ml_x, 4.0),
            male_color,
        );

        // Female range fill
        let fl_x = self.freq_to_x(self.female_low, w);
        let fh_x = self.freq_to_x(self.female_high, w);
        let female_color = if self.target_gender == Gender::Female {
            Color::from_rgba(0.0, 0.7, 0.0, 0.4)
        } else {
            Color::from_rgba(0.7, 0.0, 0.0, 0.4)
        };
        frame.fill_rectangle(
            iced::Point::new(fl_x, track_y),
            iced::Size::new(fh_x - fl_x, 4.0),
            female_color,
        );

        // Draw handles
        let blue = Color::from_rgb(0.3, 0.5, 1.0);
        let pink = Color::from_rgb(1.0, 0.4, 0.7);

        for (freq, color) in [
            (self.male_low, blue),
            (self.male_high, blue),
            (self.female_low, pink),
            (self.female_high, pink),
        ] {
            let x = self.freq_to_x(freq, w);
            frame.fill_rectangle(
                iced::Point::new(x - HANDLE_RADIUS, mid_y - HANDLE_RADIUS),
                iced::Size::new(HANDLE_RADIUS * 2.0, HANDLE_RADIUS * 2.0),
                color,
            );
        }

        // Labels at bottom
        let male_label = format!("M:{:.0}-{:.0}", self.male_low, self.male_high);
        let female_label = format!("F:{:.0}-{:.0}", self.female_low, self.female_high);

        frame.fill_text(canvas::Text {
            content: male_label,
            position: iced::Point::new(SLIDER_PAD, h - 16.0),
            color: blue,
            size: iced::Pixels(12.0),
            ..canvas::Text::default()
        });

        frame.fill_text(canvas::Text {
            content: female_label,
            position: iced::Point::new(w - SLIDER_PAD - 80.0, h - 16.0),
            color: pink,
            size: iced::Pixels(12.0),
            ..canvas::Text::default()
        });

        // Range labels
        frame.fill_text(canvas::Text {
            content: format!("{:.0}", FREQ_MIN),
            position: iced::Point::new(2.0, mid_y - 6.0),
            color: Color::from_rgb(0.6, 0.6, 0.6),
            size: iced::Pixels(11.0),
            ..canvas::Text::default()
        });
        frame.fill_text(canvas::Text {
            content: format!("{:.0}", FREQ_MAX),
            position: iced::Point::new(w - 18.0, mid_y - 6.0),
            color: Color::from_rgb(0.6, 0.6, 0.6),
            size: iced::Pixels(11.0),
            ..canvas::Text::default()
        });

        // Grey overlay if disabled
        if self.disabled {
            frame.fill_rectangle(
                iced::Point::ORIGIN,
                bounds.size(),
                Color::from_rgba(0.2, 0.2, 0.2, 0.6),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.disabled {
            return mouse::Interaction::default();
        }
        if state.dragging.is_some() {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            if self.closest_handle(pos.x, bounds.width).is_some() {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::default()
    }
}

// ── VR FOV Canvas ───────────────────────────────────────────────────────

/// Internal state for the VR FOV position drag interaction.
#[derive(Debug, Default)]
pub struct VrFovState {
    pub dragging: bool,
    pub drag_offset: (f32, f32),
    /// Live overlay position during drag (canvas-space coordinates).
    /// Used to render immediately without waiting for the config round-trip.
    pub live_pos: Option<(f32, f32)>,
}

/// A custom canvas widget showing a 16:9 dark rectangle with a draggable
/// red square representing the VR overlay position.
pub struct VrFovCanvas {
    pub vr_x: i32,
    pub vr_y: i32,
    pub vr_width: f32,
    pub vr_height: f32,
    pub disabled: bool,
}

/// Canvas display size for the FOV preview.
const FOV_W: f32 = 240.0;
const FOV_H: f32 = 135.0;
/// Virtual screen coordinate range for mapping.
const SCREEN_W: f32 = 1920.0;
const SCREEN_H: f32 = 1080.0;

impl VrFovCanvas {
    fn overlay_rect(&self) -> (f32, f32, f32, f32) {
        let sx = self.vr_x as f32 / SCREEN_W * FOV_W;
        let sy = self.vr_y as f32 / SCREEN_H * FOV_H;
        let sw = self.vr_width / SCREEN_W * FOV_W;
        let sh = self.vr_height / SCREEN_H * FOV_H;
        (sx.max(0.0), sy.max(0.0), sw.max(8.0), sh.max(8.0))
    }
}

impl canvas::Program<Message> for VrFovCanvas {
    type State = VrFovState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if self.disabled {
            return None;
        }

        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let (ox, oy, ow, oh) = self.overlay_rect();
                    if pos.x >= ox && pos.x <= ox + ow && pos.y >= oy && pos.y <= oy + oh {
                        state.dragging = true;
                        state.drag_offset = (pos.x - ox, pos.y - oy);
                        return Some(canvas::Action::capture());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.dragging {
                    if let Some(pos) = cursor.position_in(bounds) {
                        let new_sx = pos.x - state.drag_offset.0;
                        let new_sy = pos.y - state.drag_offset.1;
                        // Only update the canvas state for immediate visual feedback;
                        // config is committed on release to avoid flooding the event
                        // loop and causing UI freezes.
                        state.live_pos = Some((new_sx, new_sy));
                        // request_redraw() ensures the canvas repaints this frame.
                        // Do NOT capture — iced needs CursorMoved to propagate for
                        // internal cursor tracking; capturing it freezes the UI.
                        return Some(canvas::Action::request_redraw());
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.dragging {
                    // Commit final position to config.
                    let msg = if let Some((sx, sy)) = state.live_pos {
                        let x = (sx / FOV_W * SCREEN_W) as i32;
                        let y = (sy / FOV_H * SCREEN_H) as i32;
                        Some(Message::SettingsVrFovDragged { x, y })
                    } else {
                        None
                    };
                    state.dragging = false;
                    state.live_pos = None;
                    return if let Some(msg) = msg {
                        Some(canvas::Action::publish(msg))
                    } else {
                        Some(canvas::Action::capture())
                    };
                }
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        // Dark blue background (simulated FOV)
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(FOV_W, FOV_H),
            Color::from_rgb(0.1, 0.1, 0.25),
        );

        // Red overlay square — use live drag position when available
        // so the overlay tracks the cursor without waiting for the
        // config round-trip.
        let (ox, oy, ow, oh) = if let Some((lx, ly)) = state.live_pos {
            let (_, _, ow, oh) = self.overlay_rect();
            (lx.max(0.0), ly.max(0.0), ow, oh)
        } else {
            self.overlay_rect()
        };
        frame.fill_rectangle(
            iced::Point::new(ox, oy),
            iced::Size::new(ow, oh),
            Color::from_rgba(0.8, 0.2, 0.2, 0.7),
        );

        // Grey overlay if disabled
        if self.disabled {
            frame.fill_rectangle(
                iced::Point::ORIGIN,
                bounds.size(),
                Color::from_rgba(0.2, 0.2, 0.2, 0.6),
            );
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.disabled {
            return mouse::Interaction::default();
        }
        if state.dragging {
            return mouse::Interaction::Grabbing;
        }
        if let Some(pos) = cursor.position_in(bounds) {
            let (ox, oy, ow, oh) = self.overlay_rect();
            if pos.x >= ox && pos.x <= ox + ow && pos.y >= oy && pos.y <= oy + oh {
                return mouse::Interaction::Grab;
            }
        }
        mouse::Interaction::default()
    }
}

// ── Settings View ───────────────────────────────────────────────────────

/// Builds the complete settings window layout.
pub fn view<'a>(
    config: &Config,
    input_devices: &[String],
    output_devices: &[String],
    autostart: bool,
) -> Element<'a, Message> {
    // ── Top row ──
    let check_updates_btn = button(text("Check Updates").size(13))
        .on_press(Message::SettingsCheckForUpdates)
        .style(button::secondary);

    let autostart_cb = checkbox(autostart)
        .label("Start with Windows")
        .on_toggle(|_| Message::SettingsToggleAutostart)
        .size(14)
        .text_size(13);

    let pishock_btn = button(text("PiShock Settings").size(13)).style(button::secondary);
    let osc_btn = button(text("OSC Settings").size(13)).style(button::secondary);

    let top_row = row![check_updates_btn, autostart_cb, pishock_btn, osc_btn]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding(Padding::from(4));

    // ── Desktop column ──
    let desktop_col = build_column(config, input_devices, output_devices, false);

    // ── VR column ──
    let vr_col = build_vr_column(config, input_devices, output_devices);

    let bottom_row = row![desktop_col, vr_col]
        .spacing(16)
        .width(Length::Fill)
        .height(Length::Fill);

    let content = column![top_row, bottom_row]
        .spacing(8)
        .padding(Padding::from(12))
        .width(Length::Fill)
        .height(Length::Fill);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

/// Builds one settings column (shared between desktop and VR).
fn build_column<'a>(
    config: &Config,
    input_devices: &[String],
    output_devices: &[String],
    is_vr: bool,
) -> Element<'a, Message> {
    let (
        target_gender,
        male_low,
        male_high,
        female_low,
        female_high,
        reminder_freq,
        red_duration,
        reminder_volume,
        mic_sensitivity,
        input_name,
        output_name,
    ) = if is_vr {
        let vr = config.vr.as_ref().unwrap();
        (
            vr.target_gender,
            vr.male_freq_low,
            vr.male_freq_high,
            vr.female_freq_low,
            vr.female_freq_high,
            vr.reminder_tone_freq,
            vr.red_duration_seconds,
            vr.reminder_tone_volume,
            vr.mic_sensitivity,
            vr.input_device_name.clone(),
            vr.output_device_name.clone(),
        )
    } else {
        (
            config.target_gender,
            config.male_freq_low,
            config.male_freq_high,
            config.female_freq_low,
            config.female_freq_high,
            config.reminder_tone_freq,
            config.red_duration_seconds,
            config.reminder_tone_volume,
            config.mic_sensitivity,
            config.input_device_name.clone(),
            config.output_device_name.clone(),
        )
    };

    let gender_symbol = match target_gender {
        Gender::Female => "Target: \u{2640} Female",
        Gender::Male => "Target: \u{2642} Male",
    };

    let gender_btn = if is_vr {
        button(text(gender_symbol).size(13))
            .on_press(Message::SettingsVrToggleGender)
            .style(button::secondary)
    } else {
        button(text(gender_symbol).size(13))
            .on_press(Message::SettingsToggleGender)
            .style(button::secondary)
    };

    let freq_slider = Canvas::new(FrequencySliderCanvas {
        male_low,
        male_high,
        female_low,
        female_high,
        target_gender,
        disabled: false,
        is_vr,
    })
    .width(Length::Fill)
    .height(Length::Fixed(50.0));

    // Reminder Freq slider (70-300)
    let reminder_freq_row = {
        let label = text(format!("Reminder Freq: {:.0} Hz", reminder_freq)).size(12);
        let s = slider(70.0..=300.0, reminder_freq, move |v| {
            if is_vr {
                Message::SettingsVrReminderFreqChanged(v)
            } else {
                Message::SettingsReminderFreqChanged(v)
            }
        })
        .step(1.0)
        .on_release(if is_vr {
            Message::SettingsVrReminderFreqReleased
        } else {
            Message::SettingsReminderFreqReleased
        });
        row![label, s].spacing(8).align_y(Alignment::Center)
    };

    // Red Duration slider (0.0-5.0)
    let red_dur_row = {
        let label = text(format!("Red Duration: {:.1}s", red_duration)).size(12);
        let s = slider(0.0..=5.0, red_duration, move |v| {
            if is_vr {
                Message::SettingsVrRedDurationChanged(v)
            } else {
                Message::SettingsRedDurationChanged(v)
            }
        })
        .step(0.1);
        row![label, s].spacing(8).align_y(Alignment::Center)
    };

    // Reminder Volume slider (0.0-1.0)
    let reminder_vol_row = {
        let label = text(format!("Reminder Vol: {:.2}", reminder_volume)).size(12);
        let s = slider(0.0..=1.0, reminder_volume, move |v| {
            if is_vr {
                Message::SettingsVrReminderVolumeChanged(v)
            } else {
                Message::SettingsReminderVolumeChanged(v)
            }
        })
        .step(0.01)
        .on_release(if is_vr {
            Message::SettingsVrReminderVolumeReleased
        } else {
            Message::SettingsReminderVolumeReleased
        });
        row![label, s].spacing(8).align_y(Alignment::Center)
    };

    // Mic Sensitivity slider (1-100)
    let mic_sens_row = {
        let label = text(format!("Mic Sens: {:.0}", mic_sensitivity)).size(12);
        let s = slider(1.0..=100.0, mic_sensitivity, move |v| {
            if is_vr {
                Message::SettingsVrMicSensitivityChanged(v)
            } else {
                Message::SettingsMicSensitivityChanged(v)
            }
        })
        .step(1.0);
        row![label, s].spacing(8).align_y(Alignment::Center)
    };

    // Device pickers
    let mut input_options: Vec<String> = vec!["System Default".to_string()];
    input_options.extend(input_devices.iter().cloned());
    let selected_input = if input_name.is_empty() {
        Some("System Default".to_string())
    } else {
        Some(input_name)
    };

    let input_pick = {
        let label = text("Input:").size(12);
        let pick = pick_list(input_options, selected_input, move |name| {
            let device_name = if name == "System Default" {
                String::new()
            } else {
                name
            };
            if is_vr {
                Message::SettingsVrSelectInputDevice(device_name)
            } else {
                Message::SettingsSelectInputDevice(device_name)
            }
        })
        .text_size(12);
        row![label, pick].spacing(8).align_y(Alignment::Center)
    };

    let mut output_options: Vec<String> = vec!["System Default".to_string()];
    output_options.extend(output_devices.iter().cloned());
    let selected_output = if output_name.is_empty() {
        Some("System Default".to_string())
    } else {
        Some(output_name)
    };

    let output_pick = {
        let label = text("Output:").size(12);
        let pick = pick_list(output_options, selected_output, move |name| {
            let device_name = if name == "System Default" {
                String::new()
            } else {
                name
            };
            if is_vr {
                Message::SettingsVrSelectOutputDevice(device_name)
            } else {
                Message::SettingsSelectOutputDevice(device_name)
            }
        })
        .text_size(12);
        row![label, pick].spacing(8).align_y(Alignment::Center)
    };

    column![
        gender_btn,
        freq_slider,
        reminder_freq_row,
        red_dur_row,
        reminder_vol_row,
        mic_sens_row,
        input_pick,
        output_pick,
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

/// Builds the VR settings column with enable toggle and FOV editor.
fn build_vr_column<'a>(
    config: &Config,
    input_devices: &[String],
    output_devices: &[String],
) -> Element<'a, Message> {
    let vr_enabled = config.vr_specific_settings;
    let has_vr = config.vr.is_some();

    let heading = text("VR Settings").size(15);

    let enable_toggle = checkbox(vr_enabled)
        .label("Enable VR Settings")
        .on_toggle(|_| Message::SettingsToggleVrEnabled)
        .size(14)
        .text_size(13);

    if !vr_enabled || !has_vr {
        // Show greyed-out placeholder column
        let disabled_slider = Canvas::new(FrequencySliderCanvas {
            male_low: 85.0,
            male_high: 155.0,
            female_low: 165.0,
            female_high: 255.0,
            target_gender: Gender::Female,
            disabled: true,
            is_vr: true,
        })
        .width(Length::Fill)
        .height(Length::Fixed(50.0));

        let fov = Canvas::new(VrFovCanvas {
            vr_x: 0,
            vr_y: 0,
            vr_width: 200.0,
            vr_height: 200.0,
            disabled: true,
        })
        .width(Length::Fixed(FOV_W))
        .height(Length::Fixed(FOV_H));

        return column![heading, enable_toggle, disabled_slider, fov]
            .spacing(6)
            .width(Length::Fill)
            .into();
    }

    let vr = config.vr.as_ref().unwrap();

    // Active VR settings column
    let settings_col = build_column(config, input_devices, output_devices, true);

    // FOV canvas
    let fov = Canvas::new(VrFovCanvas {
        vr_x: vr.vr_x.unwrap_or(0),
        vr_y: vr.vr_y.unwrap_or(0),
        vr_width: vr.vr_width.unwrap_or(200.0),
        vr_height: vr.vr_height.unwrap_or(200.0),
        disabled: false,
    })
    .width(Length::Fixed(FOV_W))
    .height(Length::Fixed(FOV_H));

    column![heading, enable_toggle, settings_col, fov]
        .spacing(6)
        .width(Length::Fill)
        .into()
}
