//! Semantic colors and the circular surface shared by drawing and native
//! background effects. Geometry is in logical surface coordinates.

use super::WINDOW_SIZE;
use gpui::{Bounds, Hsla, Pixels, Point, Size, point, px};
use openlogi_ui::color;

#[derive(Clone, Copy)]
pub(crate) struct RingPalette {
    pub panel: Hsla,
    pub button: Hsla,
    pub cancel: Hsla,
    pub text: Hsla,
    pub button_text: Hsla,
    pub muted_text: Hsla,
    pub selected: Hsla,
    pub selected_text: Hsla,
    pub focus: Hsla,
}

impl Default for RingPalette {
    fn default() -> Self {
        Self {
            panel: neutral(0.06, 0.82),
            button: neutral(0.16, 0.98),
            cancel: neutral(0.20, 0.98),
            text: neutral(0.94, 1.0),
            button_text: neutral(0.98, 1.0),
            muted_text: neutral(0.82, 1.0),
            selected: color::accent_at_lightness(0.48),
            selected_text: neutral(0.98, 1.0),
            focus: color::accent_at_lightness(0.78),
        }
    }
}

const fn neutral(lightness: f32, alpha: f32) -> Hsla {
    Hsla {
        h: 0.0,
        s: 0.0,
        l: lightness,
        a: alpha,
    }
}

/// The visible disk, excluding the transparent margin reserved for shadows.
pub(crate) fn panel_bounds(origin: Point<Pixels>) -> Bounds<Pixels> {
    let inset = px(18.0);
    Bounds::new(
        origin + point(inset, inset),
        Size::new(px(WINDOW_SIZE) - inset * 2.0, px(WINDOW_SIZE) - inset * 2.0),
    )
}
