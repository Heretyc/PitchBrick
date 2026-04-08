//! Factory-reset confirmation dialog with red warning styling.

use crate::app::Message;
use iced::widget::{button, column, container, text};
use iced::{Alignment, Color, Element, Length, Padding};

/// Renders the wipe-settings confirmation dialog.
///
/// Red background, black text, big scary "DELETE EVERYTHING" button.
pub fn view<'a>() -> Element<'a, Message> {
    let heading = text("FACTORY RESET")
        .size(22)
        .color(Color::BLACK);

    let body = text(
        "This will permanently delete ALL PitchBrick settings \
         and return the application to factory defaults.\n\n\
         \u{2022} Your frequency ranges will be reset\n\
         \u{2022} Your audio device selections will be cleared\n\
         \u{2022} Your VR configuration will be removed\n\
         \u{2022} Your vocal rest history will be erased\n\n\
         PitchBrick will restart automatically after the wipe.\n\n\
         This action cannot be undone.",
    )
    .size(13)
    .color(Color::BLACK);

    let delete_btn = button(
        text("DELETE EVERYTHING")
            .size(16)
            .color(Color::WHITE),
    )
    .on_press(Message::ConfirmWipeSettings)
    .style(button::danger);

    let content = column![heading, body, delete_btn]
        .spacing(14)
        .padding(Padding::from(20))
        .width(Length::Fill)
        .align_x(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.7, 0.1, 0.1))),
            ..Default::default()
        })
        .into()
}
