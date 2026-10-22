//! Colors and type, shared by every part of the window.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use arcwise_core::Rgb;
use gpui::{Hsla, Rgba, Window, WindowAppearance, WindowBackgroundAppearance, rgb};
use serde::{Deserialize, Serialize};

/// Whether the window shows what is behind it, blurred, through its background.
static BLUR: AtomicBool = AtomicBool::new(false);
/// How much of the blur shows, from 0 to 100.
static STRENGTH: AtomicU8 = AtomicU8::new(DEFAULT_STRENGTH);
/// Whether the light theme is on, rather than the dark one.
static LIGHT: AtomicBool = AtomicBool::new(false);
/// Whether the theme follows Windows' light or dark mode.
static FOLLOW: AtomicBool = AtomicBool::new(false);

/// The strength that gives the window's look before strength could be chosen.
pub const DEFAULT_STRENGTH: u8 = 55;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Dark,
    Light,
}

/// One theme's colors, as `0xRRGGBB`.
pub struct Palette {
    pub bg: u32,
    pub panel: u32,
    pub raised: u32,
    pub hover: u32,
    pub line: u32,
    pub line_hover: u32,
    /// A border that marks the chosen one of several.
    pub line_strong: u32,
    pub text: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub attack: u32,
    pub defense: u32,
    /// The tags on lineups marked essential and pro.
    pub essential: u32,
    pub pro: u32,
    /// The color furthest from the window's, for a mark that must stand out, such as the
    /// update badge.
    pub bright: u32,
    /// What covers the window behind a chooser, and how opaque it is.
    pub scrim: (u32, f32),
}

pub static DARK: Palette = Palette {
    bg: 0x0b0e13,
    panel: 0x10141b,
    raised: 0x171c25,
    hover: 0x1d2430,
    line: 0x222a36,
    line_hover: 0x303a49,
    line_strong: 0x3a4556,
    text: 0xece8e1,
    muted: 0x8d96a6,
    faint: 0x576071,
    accent: 0xff4655,
    attack: 0xff7a85,
    defense: 0x5fd1bd,
    essential: 0xf5c76a,
    pro: 0xb99cff,
    bright: 0xffffff,
    scrim: (0x05070a, 0.6),
};

pub static LIGHT_PALETTE: Palette = Palette {
    bg: 0xeef0f3,
    panel: 0xf8f9fa,
    raised: 0xe4e7ec,
    hover: 0xd9dde4,
    line: 0xd2d7de,
    line_hover: 0xb8c0cb,
    line_strong: 0x98a2b1,
    text: 0x14171c,
    muted: 0x535c6b,
    faint: 0x737c8b,
    accent: 0xe5384a,
    attack: 0xb92d3f,
    defense: 0x0a705f,
    essential: 0x8a5d00,
    pro: 0x6545cf,
    bright: 0x0b0e13,
    scrim: (0x0b0e13, 0.32),
};

pub fn set_mode(mode: Mode) {
    LIGHT.store(mode == Mode::Light, Ordering::Relaxed);
}

pub fn mode() -> Mode {
    if LIGHT.load(Ordering::Relaxed) {
        Mode::Light
    } else {
        Mode::Dark
    }
}

pub fn set_follow_system(on: bool) {
    FOLLOW.store(on, Ordering::Relaxed);
}

pub fn follows_system() -> bool {
    FOLLOW.load(Ordering::Relaxed)
}

/// Takes Windows' light or dark mode, as the window sees it, while the theme follows it. Returns
/// whether the theme changed.
pub fn sync_system(window: &Window) -> bool {
    if !follows_system() {
        return false;
    }
    let system = match window.appearance() {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Mode::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Mode::Dark,
    };
    let changed = system != mode();
    set_mode(system);
    changed
}

fn palette() -> &'static Palette {
    match mode() {
        Mode::Dark => &DARK,
        Mode::Light => &LIGHT_PALETTE,
    }
}

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
/// [`DEFAULT_STRENGTH`] they are 0.78 and 0.5, and at 100 the backdrop is still opaque enough that
/// text stays readable over the opposite of the theme behind the window, such as a white page
/// behind the dark theme.
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
    rgb(palette().bg)
}

pub fn panel() -> Rgba {
    rgb(palette().panel)
}

pub fn raised() -> Rgba {
    rgb(palette().raised)
}

pub fn hover() -> Rgba {
    rgb(palette().hover)
}

pub fn line() -> Rgba {
    rgb(palette().line)
}

/// A border under the pointer.
pub fn line_hover() -> Rgba {
    rgb(palette().line_hover)
}

pub fn line_strong() -> Rgba {
    rgb(palette().line_strong)
}

pub fn text() -> Rgba {
    rgb(palette().text)
}

pub fn muted() -> Rgba {
    rgb(palette().muted)
}

pub fn faint() -> Rgba {
    rgb(palette().faint)
}

pub fn accent() -> Rgba {
    rgb(palette().accent)
}

pub fn attack() -> Rgba {
    rgb(palette().attack)
}

pub fn defense() -> Rgba {
    rgb(palette().defense)
}

pub fn essential() -> Rgba {
    rgb(palette().essential)
}

pub fn pro() -> Rgba {
    rgb(palette().pro)
}

pub fn bright() -> Rgba {
    rgb(palette().bright)
}

pub fn scrim() -> Hsla {
    let (color, opacity) = palette().scrim;
    alpha(rgb(color), opacity)
}

/// An agent's own color, which tints their markers and highlights.
pub fn agent(color: Rgb) -> Rgba {
    rgb(color.0)
}

/// `color`, such as an agent's, darkened in the light theme until it reads as text on any of the
/// theme's backgrounds. Agents' colors are bright, made for dark backgrounds.
pub fn readable(color: impl Into<Rgba>) -> Rgba {
    match mode() {
        Mode::Dark => color.into(),
        Mode::Light => darkened(color.into()),
    }
}

/// `color` mixed with the light theme's text until it reads on its raised background, the
/// darkest one text sits on there.
fn darkened(color: Rgba) -> Rgba {
    let (text, behind) = (rgb(LIGHT_PALETTE.text), rgb(LIGHT_PALETTE.raised));
    (0..=20)
        .map(|step| mix(color, text, 1. - step as f32 * 0.05))
        .find(|c| contrast(*c, behind) >= 4.5)
        .unwrap_or(text)
}

/// WCAG 2's contrast ratio between two opaque colors, from 1 to 21.
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let luminance = |c: Rgba| {
        let channel = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
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

    fn ratio(a: u32, b: u32) -> f32 {
        contrast(rgb(a), rgb(b))
    }

    #[test]
    fn contrast_runs_from_1_to_21() {
        assert!((ratio(0xffffff, 0x000000) - 21.).abs() < 0.01);
        assert!((ratio(0x777777, 0x777777) - 1.).abs() < 0.001);
    }

    #[test]
    fn agent_colors_read_as_text_in_the_light_theme() {
        let p = &LIGHT_PALETTE;
        // Bright yellow, lime and cyan, the hardest on white, and a blue that already reads.
        for color in [0xffe066, 0xb6ff59, 0x5fe3ff, 0x3d8bff, 0xffffff] {
            let text = darkened(rgb(color));
            for background in [p.bg, p.panel, p.raised] {
                assert!(
                    contrast(text, rgb(background)) >= 4.5,
                    "{color:06x} on {background:06x}"
                );
            }
        }
        // A color that already reads stays as it is.
        assert_eq!(darkened(rgb(0x14171c)), rgb(0x14171c));
    }

    #[test]
    fn text_is_readable_on_every_background_in_both_themes() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT_PALETTE)] {
            for (background, color) in [("bg", p.bg), ("panel", p.panel), ("raised", p.raised)] {
                let at = |c| ratio(c, color);
                assert!(at(p.text) >= 12., "{name}: text on {background}");
                assert!(at(p.muted) >= 4.5, "{name}: muted on {background}");
                // Small print such as paths and counts, as faint as the dark theme's.
                assert!(at(p.faint) >= 2.6, "{name}: faint on {background}");
                for (tag, c) in [
                    ("attack", p.attack),
                    ("defense", p.defense),
                    ("essential", p.essential),
                    ("pro", p.pro),
                ] {
                    assert!(at(c) >= 4.5, "{name}: {tag} on {background}");
                }
            }
            assert!(
                ratio(p.bright, p.panel) >= 15.,
                "{name}: the badge on the header"
            );
        }
    }
}
