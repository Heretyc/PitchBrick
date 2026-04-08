//! First-time VR Settings explanation dialog.

use crate::app::Message;
use iced::widget::{button, column, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

/// Renders the VR settings explanation dialog shown on first activation.
pub fn view<'a>() -> Element<'a, Message> {
    let heading = text("Allow VR Specific Settings").size(18);

    let body = text(
        "This allows separate frequency ranges, microphone \
         sensitivity, and audio devices for VR sessions.\n\n\
         \u{2022} VR settings only apply when the SteamVR overlay is active\n\
         \u{2022} Desktop settings remain unchanged and are used outside VR\n\
         \u{2022} You can configure each independently in the Settings window\n\n\
         When SteamVR is running, PitchBrick automatically switches \
         to your VR configuration.",
    )
    .size(13);

    let ok_btn = button(text("I Understand").size(14))
        .on_press(Message::AcknowledgeVrDialog)
        .style(button::primary);

    let content = column![heading, body, ok_btn]
        .spacing(12)
        .padding(Padding::from(20))
        .width(Length::Fill)
        .align_x(Alignment::Center);

    scrollable(content)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
