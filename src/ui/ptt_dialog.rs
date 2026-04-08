//! First-time Push-to-Talk on Green explanation dialog.

use crate::app::Message;
use iced::widget::{button, column, scrollable, text};
use iced::{Alignment, Element, Length, Padding};

/// Renders the PTT explanation dialog shown on first activation.
pub fn view<'a>() -> Element<'a, Message> {
    let heading = text("Push-to-Talk on Green").size(18);

    let body = text(
        "This feature holds your push-to-talk key only when \
         you are speaking in your target pitch range.\n\n\
         \u{2022} Prevents voice transmission outside your trained range\n\
         \u{2022} Helps avoid embarrassment and dysphoria\n\
         \u{2022} Encourages consistent target pitch practice\n\n\
         If your pitch dips out of range you get a grace period \
         before release. Configure the PTT key in Settings to \
         match your Discord keybind.",
    )
    .size(13);

    let ok_btn = button(text("I Understand").size(14))
        .on_press(Message::AcknowledgePttDialog)
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
