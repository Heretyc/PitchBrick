/// Color display canvas for the pitch indicator.
///
/// Defines the display states and color interpolation logic for the
/// visual feedback indicator that shows whether the user's voice is
/// in their target gender frequency range.
use crate::app::Message;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Color, Rectangle, Renderer, Theme};

/// The three possible display states for the pitch indicator.
///
/// Each state maps to a specific color:
/// - Green: voice is in the user's target gender range
/// - Red: voice is in speech range but not the target gender
/// - Black: no sound detected or frequency outside speech range (65-300 Hz)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DisplayState {
    /// Voice frequency is within the target gender range.
    Green,
    /// Voice is detected but outside the target gender range.
    Red,
    /// No voice detected or frequency outside human speech range.
    Black,
}

impl DisplayState {
    /// Returns the target color for this display state.
    pub fn color(&self) -> Color {
        match self {
            DisplayState::Green => Color::from_rgb(0.0, 0.8, 0.0),
            DisplayState::Red => Color::from_rgb(0.8, 0.0, 0.0),
            DisplayState::Black => Color::from_rgb(0.0, 0.0, 0.0),
        }
    }
}

/// Linearly interpolates between two colors over the range t=[0, 1].
///
/// Used for smooth 1-second color transitions between display states.
/// Values of t outside [0, 1] are clamped.
pub fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::from_rgba(
        from.r + (to.r - from.r) * t,
        from.g + (to.g - from.g) * t,
        from.b + (to.b - from.b) * t,
        from.a + (to.a - from.a) * t,
    )
}

/// A canvas program that fills its entire bounds with a single color.
///
/// Used as the main visual indicator for voice pitch state. The canvas
/// detects mouse press events and emits `Message::DragWindow` so the
/// user can drag the borderless window by clicking anywhere on it.
pub struct DisplayCanvas {
    /// The current display color (interpolated between states).
    pub color: Color,
}

impl canvas::Program<Message> for DisplayCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if cursor.position_in(bounds).is_some() {
                return Some(canvas::Action::publish(Message::DragWindow));
            }
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
        frame.fill_rectangle(iced::Point::ORIGIN, bounds.size(), self.color);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies each display state maps to the correct RGB color per spec:
    /// Green=(0,0.8,0), Red=(0.8,0,0), Black=(0,0,0). These colors were
    /// chosen for high contrast visibility during vocal training sessions.
    #[test]
    fn test_display_state_colors() {
        let green = DisplayState::Green.color();
        assert_eq!(green.r, 0.0);
        assert_eq!(green.g, 0.8);
        assert_eq!(green.b, 0.0);

        let red = DisplayState::Red.color();
        assert_eq!(red.r, 0.8);
        assert_eq!(red.g, 0.0);
        assert_eq!(red.b, 0.0);

        let black = DisplayState::Black.color();
        assert_eq!(black.r, 0.0);
        assert_eq!(black.g, 0.0);
        assert_eq!(black.b, 0.0);
    }

    /// Verifies that lerp_color returns the exact start color at t=0
    /// and the exact end color at t=1. This ensures no off-by-one errors
    /// in the color transition boundaries.
    #[test]
    fn test_lerp_color_boundaries() {
        let black = Color::from_rgb(0.0, 0.0, 0.0);
        let white = Color::from_rgb(1.0, 1.0, 1.0);

        let start = lerp_color(black, white, 0.0);
        assert_eq!(start.r, 0.0);
        assert_eq!(start.g, 0.0);
        assert_eq!(start.b, 0.0);

        let end = lerp_color(black, white, 1.0);
        assert_eq!(end.r, 1.0);
        assert_eq!(end.g, 1.0);
        assert_eq!(end.b, 1.0);
    }

    /// Verifies that the midpoint interpolation produces correct intermediate
    /// values. This is critical for the 1-second smooth color fade that
    /// provides visual feedback during vocal training.
    #[test]
    fn test_lerp_color_midpoint() {
        let black = Color::from_rgb(0.0, 0.0, 0.0);
        let green = DisplayState::Green.color();

        let mid = lerp_color(black, green, 0.5);
        assert!((mid.g - 0.4).abs() < 0.001);
        assert!((mid.r - 0.0).abs() < 0.001);
    }

    /// Verifies that t values outside [0, 1] are clamped, preventing
    /// invalid color values from timing overshoots in the animation loop.
    #[test]
    fn test_lerp_color_clamping() {
        let black = Color::from_rgb(0.0, 0.0, 0.0);
        let white = Color::from_rgb(1.0, 1.0, 1.0);

        let over = lerp_color(black, white, 2.0);
        assert_eq!(over.r, 1.0);

        let under = lerp_color(black, white, -1.0);
        assert_eq!(under.r, 0.0);
    }
}
