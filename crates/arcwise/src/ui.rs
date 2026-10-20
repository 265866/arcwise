//! Small pieces of UI used across the window.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use arcwise_core::Agent;
use gpui::{
    Bounds, Div, ElementId, FontWeight, Hsla, ImageSource, ObjectFit, Pixels, RenderImage,
    SharedString, Stateful, Transformation, canvas, div, img, prelude::*, px, svg,
};

use crate::theme;

/// The header bar's height.
pub const BAR_HEIGHT: Pixels = px(60.);

/// The header bar along the top of the window.
pub fn bar() -> Div {
    div()
        .h(BAR_HEIGHT)
        .flex_none()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(16.))
        .border_b_1()
        .border_color(theme::line())
        .bg(theme::panel())
        .child(brand())
}

fn brand() -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mr(px(14.))
        .font_family(theme::DISPLAY_FONT)
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(px(13.))
        .child(diamond(px(14.), theme::accent()))
        .child("ARCWISE")
}

/// The brand mark: a square stood on its corner.
pub fn diamond(size: Pixels, color: impl Into<Hsla>) -> impl IntoElement {
    svg().data(DIAMOND).size(size).text_color(color.into())
}

// The fixed shapes are SVGs, which GPUI draws as masks in its sprite atlas, tinted with the text
// color. Paths would cost a window-sized multisampled texture, about 27 MB of GPU memory, once
// any one was drawn.
const DIAMOND: &[u8] =
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 2"><path d="M1 0 2 1 1 2 0 1Z"/></svg>"#;
const CHEVRON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2 4 6 8 10 4" fill="none" stroke="#000" stroke-width="2"/></svg>"##;
const PLAY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1 -1 2 2"><path d="M-0.6-1 1 0-0.6 1Z"/></svg>"#;
const ARROW_HEAD: &[u8] =
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 6 10"><path d="M0 0 6 5 0 10Z"/></svg>"#;
const RING: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 34 34"><circle cx="17" cy="17" r="15.5" fill="none" stroke="#000" stroke-width="3"/></svg>"##;
/// A quarter of [`RING`], from three o'clock to six.
const ARC: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 34 34"><path d="M32.5 17A15.5 15.5 0 0 1 17 32.5" fill="none" stroke="#000" stroke-width="3"/></svg>"##;

/// A bordered button with a text label.
pub fn button(id: impl Into<ElementId>, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(36.))
        .px(px(16.))
        .rounded(px(8.))
        .border_1()
        .border_color(theme::line_hover())
        .bg(theme::raised())
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).border_color(theme::muted()))
        .child(label)
}

/// An image filling its container, cropped to cover it.
pub fn cover(image: Arc<RenderImage>) -> gpui::Img {
    img(ImageSource::Render(image))
        .size_full()
        .object_fit(ObjectFit::Cover)
}

/// An agent's portrait over their color and initial, which is what shows until it loads.
pub fn face(
    agent: &Agent,
    portrait: Option<Arc<RenderImage>>,
    grayscale: bool,
    size: Pixels,
    radius: Pixels,
) -> Div {
    let color = theme::agent(agent.color);
    let initial: String = agent.name.chars().take(1).collect();
    div()
        .relative()
        .flex_none()
        .size(size)
        .rounded(radius)
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme::mix(color, theme::raised(), 0.32))
        .font_family(theme::DISPLAY_FONT)
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(size * 0.38)
        .text_color(theme::mix(color, gpui::white(), 0.7))
        .child(initial)
        .children(portrait.map(|p| {
            div()
                .absolute()
                .inset_0()
                .child(cover(p).grayscale(grayscale))
        }))
}

/// A small uppercase caption, such as the label over a picker's value.
pub fn label(text: &str) -> Div {
    div()
        .text_size(px(10.))
        .line_height(px(12.))
        .text_color(theme::faint())
        .child(text.to_uppercase())
}

/// A count, in the monospaced face.
pub fn num(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(11.))
        .text_color(theme::faint())
        .child(text.into())
}

/// A small downward chevron.
pub fn chevron(color: impl Into<Hsla>) -> impl IntoElement {
    svg()
        .data(CHEVRON)
        .size(px(12.))
        .ml(px(2.))
        .text_color(color.into())
}

/// Records its parent's bounds each frame, for sizing or placing something by it. Layout that
/// reads the bounds sees the previous frame's, so a change asks for one more frame.
pub fn measure(into: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(
        move |bounds, window, _| {
            if into.replace(bounds) != bounds {
                // Draws the measuring view again; a refresh during a frame does nothing.
                window.request_animation_frame();
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// A key cap, as in the keyboard hints.
pub fn kbd(key: impl Into<SharedString>, lit: bool) -> Div {
    div()
        .min_w(px(18.))
        .px(px(4.))
        .py(px(1.))
        .flex()
        .justify_center()
        .rounded(px(4.))
        .border_1()
        .border_b_2()
        .border_color(theme::line())
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(10.))
        .line_height(px(12.))
        .text_color(if lit { theme::text() } else { theme::muted() })
        .child(key.into())
}

/// A play triangle centered in its parent.
pub fn play_glyph(size: Pixels) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(svg().data(PLAY).size(size).text_color(theme::text()))
}

/// A short arrow fading from `from` to `to`, pointing right.
pub fn arrow(from: impl Into<Hsla>, to: impl Into<Hsla>) -> Div {
    let (from, to) = (from.into(), to.into());
    div()
        .relative()
        .w(px(34.))
        .h(px(10.))
        .child(
            div()
                .absolute()
                .left_0()
                .right(px(4.))
                .top(px(4.))
                .h(px(2.))
                .bg(gpui::linear_gradient(
                    90.,
                    gpui::linear_color_stop(from, 0.),
                    gpui::linear_color_stop(to, 1.),
                )),
        )
        .child(
            svg()
                .data(ARROW_HEAD)
                .absolute()
                .right_0()
                .top_0()
                .w(px(6.))
                .h(px(10.))
                .text_color(to),
        )
}

/// A turning ring, centered in its parent, for something on its way.
pub fn spinner(size: Pixels) -> impl IntoElement {
    // Turns once every 800 ms, measured from a fixed point so redraws do not restart it.
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let t = EPOCH
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f32();
    let turn = (t / 0.8).fract() * std::f32::consts::TAU;
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .relative()
                .size(size)
                .child(
                    svg()
                        .data(RING)
                        .absolute()
                        .inset_0()
                        .text_color(theme::alpha(gpui::white(), 0.19)),
                )
                .child(
                    svg()
                        .data(ARC)
                        .absolute()
                        .inset_0()
                        .text_color(theme::text())
                        .with_transformation(Transformation::rotate(gpui::radians(turn))),
                )
                // Draws the next turn on the next frame.
                .child(canvas(
                    |_, _, _| {},
                    |_, (), window, _| window.request_animation_frame(),
                )),
        )
}
