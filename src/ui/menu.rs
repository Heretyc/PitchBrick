//! Settings menu bar for PitchBrick.
//!
//! Builds a dropdown "Settings" menu with controls for gender toggle,
//! config file opening, reminder tone sliders, and audio device selection.

use crate::app::Message;
use crate::config::Config;
use iced::widget::{button, pick_list, row, slider, text};
use iced::{Element, Length};
use iced_aw::menu::{Item, Menu, MenuBar};

/// Builds the settings menu bar for the application.
///
/// Creates a single "Settings" dropdown containing all user controls:
/// gender toggle, config file opener, tone frequency/volume sliders,
/// and input/output device pick lists.
///
/// # Arguments
///
/// * `config` - Current application configuration (for display values).
/// * `input_devices` - Names of available microphone devices.
/// * `output_devices` - Names of available speaker/headphone devices.
pub fn build_menu_bar<'a>(
    config: &Config,
    input_devices: &[String],
    output_devices: &[String],
) -> Element<'a, Message> {
    let gender_label = format!("Target: {}", config.target_gender);

    let selected_input = input_devices
        .iter()
        .find(|d| *d == &config.input_device_name)
        .cloned();
    let selected_output = output_devices
        .iter()
        .find(|d| *d == &config.output_device_name)
        .cloned();

    let items = vec![
        Item::new(
            button(text(gender_label))
                .on_press(Message::ToggleGender)
                .width(Length::Fill),
        ),
        Item::new(
            button(text("Open Config File"))
                .on_press(Message::OpenSettings)
                .width(Length::Fill),
        ),
        Item::new(
            row![
                text("Freq: "),
                slider(
                    100.0..=4000.0,
                    config.reminder_tone_freq,
                    Message::SetReminderFreq
                ),
                text(format!(" {:.0} Hz", config.reminder_tone_freq)),
            ]
            .spacing(5),
        )
        .close_on_click(false),
        Item::new(
            row![
                text("Vol: "),
                slider(
                    0.0..=1.0,
                    config.reminder_tone_volume,
                    Message::SetReminderVolume
                )
                .step(0.01),
                text(format!(" {:.0}%", config.reminder_tone_volume * 100.0)),
            ]
            .spacing(5),
        )
        .close_on_click(false),
        Item::new(
            pick_list(
                input_devices.to_vec(),
                selected_input,
                Message::SelectInputDevice,
            )
            .placeholder("Input Device")
            .width(Length::Fill),
        ),
        Item::new(
            pick_list(
                output_devices.to_vec(),
                selected_output,
                Message::SelectOutputDevice,
            )
            .placeholder("Output Device")
            .width(Length::Fill),
        ),
    ];

    let settings_menu = Menu::new(items).max_width(250.0);

    let root = Item::with_menu(
        button(text("Settings")).on_press(Message::Noop),
        settings_menu,
    );

    MenuBar::new(vec![root]).width(Length::Fill).into()
}
