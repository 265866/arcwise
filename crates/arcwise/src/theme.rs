//! Colors and type, shared by every part of the window.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use arcwise_core::Rgb;
use gpui::{Hsla, Rgba, WindowBackgroundAppearance, rgb};

/// Whether the window shows what is behind it, blurred, through its background.
static BLUR: AtomicBool = AtomicBool::new(false);
/// How much of the blur shows, from 0 to 100.
static STRENGTH: AtomicU8 = AtomicU8::new(DEFAULT_STRENGTH);

/// The strength that gives the window's look before strength could be chosen.
pub const DEFAULT_STRENGTH: u8 = 55;

pub fn set_blur(on: bool) {
    BLUR.store(on, Ordering::Relaxed);
}

pub fn blur() -> bool {
    BLUR.load(Ordering::Relaxed)
}

pub fn set_strength(strength: u8) {
    STRENGTH.store(strength.min(100), Ordering::Relaxed);
}

pub fn strength() -> u8 {
    STRENGTH.load(Ordering::Relaxed)
}

/// How opaque [`backdrop`] and [`surface`] are at `strength`. At 0 the window is solid, at
/// [`DEFAULT_STRENGTH`] they are 0.78 and 0.5, and at 100 the backdrop is still dark enough that
/// text stays readable over a white page behind the window.
fn opacity(strength: u8) -> (f32, f32) {
    let s = f32::from(strength.min(100)) / f32::from(DEFAULT_STRENGTH);
    (1. - 0.22 * s, 1. - 0.5 * s)
}

/// What the window asks Windows to draw behind it.
pub fn window_background() -> WindowBackgroundAppearance {
    if blur() {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// The window's own background, see-through by [`strength`] while [`blur`] is on.
pub fn backdrop() -> Hsla {
    alpha(bg(), if blur() { opacity(strength()).0 } else { 1. })
}

/// The header and the lineup panel, laid over [`backdrop`].
pub fn surface() -> Hsla {
    alpha(panel(), if blur() { opacity(strength()).1 } else { 1. })
}

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

pub fn faint() -> Rgba {
    rgb(0x576071)
}

pub fn accent() -> Rgba {
    rgb(0xff4655)
}

pub fn attack() -> Rgba {
    rgb(0xff7a85)
}

pub fn defense() -> Rgba {
    rgb(0x5fd1bd)
}

/// An agent's own color, which tints their markers and highlights.
pub fn agent(color: Rgb) -> Rgba {
    rgb(color.0)
}

/// `color` at `alpha` opacity.
pub fn alpha(color: impl Into<Hsla>, alpha: f32) -> Hsla {
    let mut color = color.into();
    color.a = alpha;
    color
}

/// `amount` of `a` mixed into `b`, like CSS `color-mix(in srgb, a amount, b)`.
pub fn mix(a: impl Into<Rgba>, b: impl Into<Rgba>, amount: f32) -> Rgba {
    let (a, b) = (a.into(), b.into());
    let lerp = |x: f32, y: f32| x * amount + y * (1. - amount);
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

pub const FONT: &str = "Segoe UI Variable Text";
pub const DISPLAY_FONT: &str = "Segoe UI Variable Display";
pub const MONO_FONT: &str = "Cascadia Mono";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_runs_from_solid_to_readable() {
        assert_eq!(opacity(0), (1., 1.), "solid, as with blur off");
        let (backdrop, surface) = opacity(DEFAULT_STRENGTH);
        assert!((backdrop - 0.78).abs() < 0.001 && (surface - 0.5).abs() < 0.001);
        let (backdrop, surface) = opacity(100);
        assert!((backdrop - 0.6).abs() < 0.001 && (surface - 0.091).abs() < 0.001);
        assert_eq!(opacity(200), opacity(100));
    }
}
