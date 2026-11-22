//! Colors and type, shared by every part of the window.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use arcwise_core::Rgb;
use gpui::{Hsla, Rgba, Window, WindowAppearance, WindowBackgroundAppearance};
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
pub(crate) const DEFAULT_STRENGTH: u8 = 55;

/// The dark or the light theme, as the settings file names it: `"dark"` or `"light"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Dark,
    Light,
}

/// What covers the window behind a chooser.
#[derive(Debug)]
pub(crate) struct Scrim {
    /// Its color, as `0xRRGGBB`.
    pub(crate) color: u32,
    /// How opaque it is, from 0 to 1.
    pub(crate) opacity: f32,
}

/// One theme's colors, as `0xRRGGBB`.
#[derive(Debug)]
pub(crate) struct Palette {
    pub(crate) bg: u32,
    pub(crate) panel: u32,
    pub(crate) raised: u32,
    pub(crate) hover: u32,
    pub(crate) line: u32,
    pub(crate) line_hover: u32,
    /// A border that marks the chosen one of several.
    pub(crate) line_strong: u32,
    pub(crate) text: u32,
    pub(crate) muted: u32,
    pub(crate) faint: u32,
    pub(crate) accent: u32,
    pub(crate) attack: u32,
    pub(crate) defense: u32,
    /// The tags on lineups marked essential and pro.
    pub(crate) essential: u32,
    pub(crate) pro: u32,
    /// The color furthest from the window's, for a mark that must stand out, such as the
    /// update badge.
    pub(crate) bright: u32,
    pub(crate) scrim: Scrim,
}

/// The dark theme's colors.
pub(crate) static DARK: Palette = Palette {
    bg: 0x0b_0e_13,
    panel: 0x10_14_1b,
    raised: 0x17_1c_25,
    hover: 0x1d_24_30,
    line: 0x22_2a_36,
    line_hover: 0x30_3a_49,
    line_strong: 0x3a_45_56,
    text: 0xec_e8_e1,
    muted: 0x8d_96_a6,
    faint: 0x57_60_71,
    accent: 0xff_46_55,
    attack: 0xff_7a_85,
    defense: 0x5f_d1_bd,
    essential: 0xf5_c7_6a,
    pro: 0xb9_9c_ff,
    bright: 0xff_ff_ff,
    scrim: Scrim {
        color: 0x05_07_0a,
        opacity: 0.6,
    },
};

/// The light theme's colors.
pub(crate) static LIGHT_PALETTE: Palette = Palette {
    bg: 0xee_f0_f3,
    panel: 0xf8_f9_fa,
    raised: 0xe4_e7_ec,
    hover: 0xd9_dd_e4,
    line: 0xd2_d7_de,
    line_hover: 0xb8_c0_cb,
    line_strong: 0x98_a2_b1,
    text: 0x14_17_1c,
    muted: 0x53_5c_6b,
    faint: 0x73_7c_8b,
    accent: 0xe5_38_4a,
    attack: 0xb9_2d_3f,
    defense: 0x0a_70_5f,
    essential: 0x8a_5d_00,
    pro: 0x65_45_cf,
    bright: 0x0b_0e_13,
    scrim: Scrim {
        color: 0x0b_0e_13,
        opacity: 0.32,
    },
};

pub(crate) fn set_mode(mode: Mode) {
    LIGHT.store(mode == Mode::Light, Ordering::Relaxed);
}

#[must_use]
pub(crate) fn mode() -> Mode {
    if LIGHT.load(Ordering::Relaxed) {
        Mode::Light
    } else {
        Mode::Dark
    }
}

pub(crate) fn set_follow_system(on: bool) {
    FOLLOW.store(on, Ordering::Relaxed);
}

#[must_use]
pub(crate) fn follows_system() -> bool {
    FOLLOW.load(Ordering::Relaxed)
}

/// Takes Windows' light or dark mode, as the window sees it, while the theme follows it. Returns
/// whether the theme changed.
pub(crate) fn sync_system(window: &Window) -> bool {
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

pub(crate) fn set_blur(on: bool) {
    BLUR.store(on, Ordering::Relaxed);
}

#[must_use]
pub(crate) fn blur() -> bool {
    BLUR.load(Ordering::Relaxed)
}

pub(crate) fn set_strength(strength: u8) {
    STRENGTH.store(strength.min(100), Ordering::Relaxed);
}

#[must_use]
pub(crate) fn strength() -> u8 {
    STRENGTH.load(Ordering::Relaxed)
}

/// How opaque [`backdrop`] and [`surface`] are.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Opacity {
    backdrop: f32,
    surface: f32,
}

/// How opaque [`backdrop`] and [`surface`] are at `strength`. At 0 the window is solid, at
/// [`DEFAULT_STRENGTH`] they are 0.78 and 0.5, and at 100 the backdrop is still opaque enough that
/// text stays readable over the opposite of the theme behind the window, such as a white page
/// behind the dark theme.
fn opacity(strength: u8) -> Opacity {
    let s = f32::from(strength.min(100)) / f32::from(DEFAULT_STRENGTH);
    Opacity {
        backdrop: 1. - 0.22 * s,
        surface: 1. - 0.5 * s,
    }
}

/// What the window asks Windows to draw behind it.
#[must_use]
pub(crate) fn window_background() -> WindowBackgroundAppearance {
    if blur() {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Opaque
    }
}

/// The window's own background, see-through by [`strength`] while [`blur`] is on.
#[must_use]
pub(crate) fn backdrop() -> Hsla {
    alpha(
        bg().into(),
        if blur() {
            opacity(strength()).backdrop
        } else {
            1.
        },
    )
}

/// The header and the lineup panel, laid over [`backdrop`].
#[must_use]
pub(crate) fn surface() -> Hsla {
    alpha(
        panel().into(),
        if blur() {
            opacity(strength()).surface
        } else {
            1.
        },
    )
}

#[must_use]
pub(crate) fn bg() -> Rgba {
    gpui::rgb(palette().bg)
}

#[must_use]
pub(crate) fn panel() -> Rgba {
    gpui::rgb(palette().panel)
}

#[must_use]
pub(crate) fn raised() -> Rgba {
    gpui::rgb(palette().raised)
}

#[must_use]
pub(crate) fn hover() -> Rgba {
    gpui::rgb(palette().hover)
}

#[must_use]
pub(crate) fn line() -> Rgba {
    gpui::rgb(palette().line)
}

/// A border under the pointer.
#[must_use]
pub(crate) fn line_hover() -> Rgba {
    gpui::rgb(palette().line_hover)
}

#[must_use]
pub(crate) fn line_strong() -> Rgba {
    gpui::rgb(palette().line_strong)
}

#[must_use]
pub(crate) fn text() -> Rgba {
    gpui::rgb(palette().text)
}

#[must_use]
pub(crate) fn muted() -> Rgba {
    gpui::rgb(palette().muted)
}

#[must_use]
pub(crate) fn faint() -> Rgba {
    gpui::rgb(palette().faint)
}

#[must_use]
pub(crate) fn accent() -> Rgba {
    gpui::rgb(palette().accent)
}

#[must_use]
pub(crate) fn attack() -> Rgba {
    gpui::rgb(palette().attack)
}

#[must_use]
pub(crate) fn defense() -> Rgba {
    gpui::rgb(palette().defense)
}

#[must_use]
pub(crate) fn essential() -> Rgba {
    gpui::rgb(palette().essential)
}

#[must_use]
pub(crate) fn pro() -> Rgba {
    gpui::rgb(palette().pro)
}

#[must_use]
pub(crate) fn bright() -> Rgba {
    gpui::rgb(palette().bright)
}

#[must_use]
pub(crate) fn scrim() -> Hsla {
    let scrim = &palette().scrim;
    alpha(gpui::rgb(scrim.color).into(), scrim.opacity)
}

/// An agent's own color, which tints their markers and highlights.
#[must_use]
pub(crate) fn agent(color: Rgb) -> Rgba {
    gpui::rgb(color.0)
}

/// `color`, such as an agent's, darkened in the light theme until it reads as text on any of the
/// theme's backgrounds. Agents' colors are bright, made for dark backgrounds.
#[must_use]
pub(crate) fn readable(color: Rgba) -> Rgba {
    match mode() {
        Mode::Dark => color,
        Mode::Light => darkened(color),
    }
}

/// `color` mixed with the light theme's text until it reads on its raised background, the
/// darkest one text sits on there.
fn darkened(color: Rgba) -> Rgba {
    let text = gpui::rgb(LIGHT_PALETTE.text);
    let behind = gpui::rgb(LIGHT_PALETTE.raised);
    (0..=20_u8)
        .map(|step| mix(color, text, 1. - f32::from(step) * 0.05))
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
#[must_use]
pub(crate) fn alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}

/// `amount` of `a` mixed into `b`, like CSS `color-mix(in srgb, a amount, b)`.
#[must_use]
pub(crate) fn mix(a: Rgba, b: Rgba, amount: f32) -> Rgba {
    let lerp = |x: f32, y: f32| x * amount + y * (1. - amount);
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
}

/// The face for text.
pub(crate) const FONT: &str = "Segoe UI Variable Text";
/// The face for headings and initials.
pub(crate) const DISPLAY_FONT: &str = "Segoe UI Variable Display";
/// The face for counts and key caps.
pub(crate) const MONO_FONT: &str = "Cascadia Mono";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_runs_from_solid_to_readable() {
        assert_eq!(
            opacity(0),
            Opacity {
                backdrop: 1.,
                surface: 1.
            },
            "solid, as with blur off"
        );
        let Opacity { backdrop, surface } = opacity(DEFAULT_STRENGTH);
        assert!((backdrop - 0.78).abs() < 0.001 && (surface - 0.5).abs() < 0.001);
        let Opacity { backdrop, surface } = opacity(100);
        assert!((backdrop - 0.6).abs() < 0.001 && (surface - 0.091).abs() < 0.001);
        assert_eq!(opacity(200), opacity(100));
    }

    fn ratio(a: u32, b: u32) -> f32 {
        contrast(gpui::rgb(a), gpui::rgb(b))
    }

    #[test]
    fn contrast_runs_from_1_to_21() {
        assert!((ratio(0xff_ff_ff, 0x00_00_00) - 21.).abs() < 0.01);
        assert!((ratio(0x77_77_77, 0x77_77_77) - 1.).abs() < 0.001);
    }

    #[test]
    fn agent_colors_read_as_text_in_the_light_theme() {
        let p = &LIGHT_PALETTE;
        let hardest_on_white = [0xff_e0_66, 0xb6_ff_59, 0x5f_e3_ff];
        let already_reads = [0x3d_8b_ff, 0xff_ff_ff];
        for color in hardest_on_white.into_iter().chain(already_reads) {
            let text = darkened(gpui::rgb(color));
            for background in [p.bg, p.panel, p.raised] {
                assert!(
                    contrast(text, gpui::rgb(background)) >= 4.5,
                    "{color:06x} on {background:06x}"
                );
            }
        }
        assert_eq!(
            darkened(gpui::rgb(0x14_17_1c)),
            gpui::rgb(0x14_17_1c),
            "a color that already reads stays as it is"
        );
    }

    #[test]
    fn text_is_readable_on_every_background_in_both_themes() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT_PALETTE)] {
            for (background, color) in [("bg", p.bg), ("panel", p.panel), ("raised", p.raised)] {
                let at = |c| ratio(c, color);
                assert!(at(p.text) >= 12., "{name}: text on {background}");
                assert!(at(p.muted) >= 4.5, "{name}: muted on {background}");
                assert!(
                    at(p.faint) >= 2.6,
                    "{name}: faint small print, such as paths and counts, on {background}"
                );
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

    #[test]
    fn the_settings_file_names_the_modes_as_before() {
        assert_eq!(
            serde_json::to_string(&Mode::Dark).expect("a mode serializes"),
            r#""dark""#
        );
        assert_eq!(
            serde_json::to_string(&Mode::Light).expect("a mode serializes"),
            r#""light""#
        );
        let light: Mode = serde_json::from_str(r#""light""#).expect("a saved mode loads");
        assert_eq!(light, Mode::Light);
    }
}
