//! Log window view for verbose mode.
//!
//! Renders a scrollable, monospace list of formatted log lines.
//! Auto-scroll to the latest line is driven by the caller via `scroll_id()`.

use crate::app::Message;
use iced::widget::{column, scrollable, text};
use iced::{Element, Font, Length, Padding};

/// Returns the stable widget ID used to drive snap-to-bottom tasks.
///
/// Both the scrollable widget and the `scrollable::snap_to` call must use
/// this same ID.
pub fn scroll_id() -> iced::widget::Id {
    iced::widget::Id::new("pitchbrick_log")
}

/// Renders all log lines in a vertically scrollable, monospace column.
pub fn view(lines: &[String]) -> Element<'_, Message> {
    let content = lines.iter().fold(
        column![].spacing(1).padding(Padding::from([8, 10])),
        |col, line| col.push(text(line.as_str()).size(12).font(Font::MONOSPACE)),
    );

    scrollable(content)
        .id(scroll_id())
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
