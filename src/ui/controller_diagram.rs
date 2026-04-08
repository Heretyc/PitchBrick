//! Interactive Xbox Series X controller diagram with embedded PNG.
//!
//! Uses a 3D render of the controller as background (via iced image widget),
//! overlaid with an interactive canvas for button selection highlights.

use crate::app::Message;
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Stroke};
use iced::widget::{image, stack, Canvas};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme};
use std::sync::LazyLock;

// ── Embedded controller PNG, decoded once ───────────────────────────────

static CONTROLLER_PNG: &[u8] = include_bytes!("../images/controller.png");

static CONTROLLER_HANDLE: LazyLock<image::Handle> = LazyLock::new(|| {
    let decoded = ::image::load_from_memory(CONTROLLER_PNG)
        .expect("embedded controller PNG is valid")
        .resize_exact(CW as u32, CH as u32, ::image::imageops::FilterType::Lanczos3)
        .into_rgba8();
    let (w, h) = decoded.dimensions();
    image::Handle::from_rgba(w, h, decoded.into_raw())
});

// Canvas dimensions — matched to the image aspect ratio (972:668 ≈ 1.455).
const CW: f32 = 280.0;
const CH: f32 = 192.0;

// ── Palette ─────────────────────────────────────────────────────────────

const SEL: Color = Color { r: 0.35, g: 0.65, b: 1.0, a: 1.0 };
const GLOW: Color = Color { r: 0.35, g: 0.65, b: 1.0, a: 0.30 };

// ── Hit regions (positioned to overlay the 3D render at 280×192) ────────
// Coordinates mapped from the 972×668 source image scaled to 280×192.

struct Btn { name: &'static str, cx: f32, cy: f32, r: f32 }

const BUTTONS: &[Btn] = &[
    // Face buttons (right side, diamond pattern)
    Btn { name: "Y",  cx: 192.0, cy: 68.0,  r: 8.0 },
    Btn { name: "X",  cx: 176.0, cy: 82.0,  r: 8.0 },
    Btn { name: "B",  cx: 208.0, cy: 82.0,  r: 8.0 },
    Btn { name: "A",  cx: 192.0, cy: 96.0,  r: 8.0 },
    // Bumpers (along top shoulders)
    Btn { name: "LB", cx: 82.0,  cy: 18.0,  r: 20.0 },
    Btn { name: "RB", cx: 198.0, cy: 18.0,  r: 20.0 },
    // View / Menu (center)
    Btn { name: "BACK",  cx: 112.0, cy: 68.0, r: 7.0 },
    Btn { name: "START", cx: 168.0, cy: 68.0, r: 7.0 },
    // Thumbsticks
    Btn { name: "LTHUMB", cx: 92.0,  cy: 68.0,  r: 14.0 },
    Btn { name: "RTHUMB", cx: 172.0, cy: 110.0, r: 14.0 },
    // D-pad
    Btn { name: "UP",    cx: 92.0,  cy: 98.0,  r: 7.0 },
    Btn { name: "DOWN",  cx: 92.0,  cy: 122.0, r: 7.0 },
    Btn { name: "LEFT",  cx: 80.0,  cy: 110.0, r: 7.0 },
    Btn { name: "RIGHT", cx: 104.0, cy: 110.0, r: 7.0 },
];

fn hit_test(x: f32, y: f32) -> Option<&'static str> {
    for b in BUTTONS {
        let dx = x - b.cx;
        let dy = y - b.cy;
        let hr = b.r + 4.0;
        if dx * dx + dy * dy <= hr * hr { return Some(b.name); }
    }
    None
}

// ── Public view function ────────────────────────────────────────────────

pub fn view<'a>(selected_button: &str) -> Element<'a, Message> {
    let bg = image::Image::new(CONTROLLER_HANDLE.clone())
        .width(Length::Fixed(CW))
        .height(Length::Fixed(CH));

    let overlay = Canvas::new(OverlayCanvas {
        selected_button: selected_button.to_string(),
    })
    .width(Length::Fixed(CW))
    .height(Length::Fixed(CH));

    stack![bg, overlay]
        .width(Length::Fixed(CW))
        .height(Length::Fixed(CH))
        .into()
}

// ── Transparent overlay canvas ──────────────────────────────────────────

struct OverlayCanvas { selected_button: String }

#[derive(Debug, Default)]
pub struct OverlayState;

impl canvas::Program<Message> for OverlayCanvas {
    type State = OverlayState;

    fn update(
        &self, _s: &mut Self::State, event: &canvas::Event,
        bounds: Rectangle, cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event {
            if let Some(pos) = cursor.position_in(bounds) {
                if let Some(n) = hit_test(pos.x, pos.y) {
                    return Some(canvas::Action::publish(
                        Message::SettingsGamepadButtonChanged(n.to_string()),
                    ));
                }
                return Some(canvas::Action::capture());
            }
        }
        None
    }

    fn draw(
        &self, _s: &Self::State, renderer: &Renderer, _theme: &Theme,
        bounds: Rectangle, _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());

        // Draw selection highlight on the selected button.
        for btn in BUTTONS {
            if btn.name != self.selected_button { continue; }
            fill_circle(&mut f, btn.cx, btn.cy, btn.r + 5.0, GLOW);
            stroke_circle(&mut f, btn.cx, btn.cy, btn.r + 3.0, SEL, 2.0);

            // Label
            let lx = btn.cx - (btn.name.len() as f32 * 2.8);
            let ly = btn.cy + btn.r + 6.0;
            f.fill_text(canvas::Text {
                content: btn.name.to_string(),
                position: Point::new(lx, ly),
                color: SEL,
                size: iced::Pixels(8.0),
                ..canvas::Text::default()
            });
        }

        // Selected label at bottom
        f.fill_text(canvas::Text {
            content: format!("Selected: {}", self.selected_button),
            position: Point::new(100.0, 180.0),
            color: Color::from_rgb(0.7, 0.7, 0.75),
            size: iced::Pixels(10.0),
            ..canvas::Text::default()
        });

        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self, _s: &Self::State, bounds: Rectangle, cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(pos) = cursor.position_in(bounds) {
            if hit_test(pos.x, pos.y).is_some() { return mouse::Interaction::Pointer; }
        }
        mouse::Interaction::default()
    }
}

fn fill_circle(f: &mut Frame, cx: f32, cy: f32, r: f32, c: Color) {
    let mut b = canvas::path::Builder::new();
    b.circle(Point::new(cx, cy), r);
    f.fill(&b.build(), c);
}

fn stroke_circle(f: &mut Frame, cx: f32, cy: f32, r: f32, c: Color, w: f32) {
    let mut b = canvas::path::Builder::new();
    b.circle(Point::new(cx, cy), r);
    f.stroke(&b.build(), Stroke::default().with_color(c).with_width(w));
}
