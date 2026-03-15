//! Start Menu shortcut mismatch dialog view.
//!
//! Shown when the existing shortcut points to a different PitchBrick binary.

use crate::app::Message;
use iced::widget::{button, column, text};
use iced::{Alignment, Element, Length, Padding};

/// Renders the shortcut mismatch dialog.
pub fn view<'a>(old_path: &str, current_path: &str) -> Element<'a, Message> {
    let heading = text("Start Menu Shortcut").size(18);

    let body = text(
        "Your Start Menu shortcut for PitchBrick points to a different location. Update it to the current version?",
    )
    .size(13);

    let old_label = text(format!("Old: {old_path}")).size(11);
    let new_label = text(format!("New: {current_path}")).size(11);

    let update_btn = button(text("Update Shortcut").size(14))
        .on_press(Message::AcceptShortcutUpdate)
        .style(button::primary);

    let decline_btn = button(text("No, don't ask again").size(14))
        .on_press(Message::DeclineShortcutUpdate)
        .style(button::secondary);

    let buttons = iced::widget::row![update_btn, decline_btn]
        .spacing(10)
        .align_y(Alignment::Center);

    column![heading, body, old_label, new_label, buttons]
        .spacing(8)
        .padding(Padding::from(16))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into()
}
