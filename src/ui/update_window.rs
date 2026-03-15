//! Update notification window view.
//!
//! Displays the available version, current version, and action buttons
//! when a newer version of PitchBrick is published on crates.io.

use crate::app::Message;
use iced::widget::{button, column, row, text};
use iced::{Alignment, Element, Length, Padding};

/// Renders the update notification dialog.
pub fn view<'a>(new_version: &str, current_version: &str) -> Element<'a, Message> {
    let heading = text("Update Available")
        .size(20);

    let body = text(format!(
        "PitchBrick v{} is available (you have v{}).",
        new_version, current_version
    ))
    .size(14);

    let changes_btn = button(text("View changes").size(13))
        .on_press(Message::OpenCratesPage)
        .style(button::secondary);

    let update_btn = button(text("Update Now").size(14))
        .on_press(Message::AcceptUpdate)
        .style(button::primary);

    let not_now_btn = button(text("Not Now").size(14))
        .on_press(Message::DeclineUpdate)
        .style(button::secondary);

    let buttons = row![update_btn, not_now_btn]
        .spacing(10)
        .align_y(Alignment::Center);

    column![heading, body, changes_btn, buttons]
        .spacing(12)
        .padding(Padding::from(20))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}
