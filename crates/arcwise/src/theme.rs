//! Colors and type, shared by every part of the window.

use gpui::{Rgba, rgb};

pub fn bg() -> Rgba {
    rgb(0x0b0e13)
}

pub fn panel() -> Rgba {
    rgb(0x10141b)
}

pub fn line() -> Rgba {
    rgb(0x222a36)
}

pub fn text() -> Rgba {
    rgb(0xece8e1)
}

pub fn accent() -> Rgba {
    rgb(0xff4655)
}

pub const FONT: &str = "Segoe UI Variable Text";
pub const DISPLAY_FONT: &str = "Segoe UI Variable Display";
