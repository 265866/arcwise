//! Colors and type, shared by every part of the window.

use gpui::{Hsla, Rgba, rgb};

pub fn bg() -> Rgba {
    rgb(0x0b0e13)
}

pub fn panel() -> Rgba {
    rgb(0x10141b)
}

pub fn raised() -> Rgba {
    rgb(0x171c25)
}

pub fn hover() -> Rgba {
    rgb(0x1d2430)
}

pub fn line() -> Rgba {
    rgb(0x222a36)
}

/// A border under the pointer.
pub fn line_hover() -> Rgba {
    rgb(0x303a49)
}

pub fn text() -> Rgba {
    rgb(0xece8e1)
}

pub fn muted() -> Rgba {
    rgb(0x8d96a6)
}

pub fn accent() -> Rgba {
    rgb(0xff4655)
}

pub fn attack() -> Rgba {
    rgb(0xff7a85)
}

/// `color` at `alpha` opacity.
pub fn alpha(color: impl Into<Hsla>, alpha: f32) -> Hsla {
    let mut color = color.into();
    color.a = alpha;
    color
}

pub const FONT: &str = "Segoe UI Variable Text";
pub const DISPLAY_FONT: &str = "Segoe UI Variable Display";
