//! ViGEmBus driver install prompt dialog.
//!
//! Shown when PTT-on-Green is enabled but the ViGEmBus driver is not installed.

use crate::app::Message;
use iced::widget::{button, column, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

/// Renders the ViGEmBus install prompt dialog.
pub fn view<'a>() -> Element<'a, Message> {
    let heading = text("Virtual Gamepad Driver").size(18);

    let body = text(
        "PitchBrick can create a virtual Xbox 360 controller so \
         apps like Discord, VRChat, and Steam can detect your \
         Push-to-Talk button as a gamepad input.\n\n\
         This requires the ViGEmBus driver (~6 MB download). \
         An administrator prompt will appear during installation.\n\n\
         Windows will play a controller connect/disconnect sound \
         when Push-to-Talk is toggled on and off.",
    )
    .size(13);

    let install_btn = button(text("Install ViGEmBus").size(14))
        .on_press(Message::AcceptVigemInstall)
        .style(button::primary);

    let decline_btn = button(text("No Thanks").size(14))
        .on_press(Message::DeclineVigemInstall)
        .style(button::secondary);

    let buttons = iced::widget::row![install_btn, decline_btn]
        .spacing(10)
        .align_y(Alignment::Center);

    let content = column![heading, body, buttons]
        .spacing(12)
        .padding(Padding::from(20))
        .width(Length::Fill)
        .align_x(Alignment::Center);

    scrollable(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
