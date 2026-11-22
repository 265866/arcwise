//! Small pieces of UI used across the window.
//!
//! The fixed shapes are SVGs, which GPUI draws as masks in its sprite atlas, tinted with the text
//! color. Paths would cost a window-sized multisampled texture, about 27 MB of GPU memory, once
//! any one was drawn.

use std::cell::Cell;
use std::f32::consts::TAU;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use arcwise_core::Agent;
use gpui::{
    AnyView, App, Bounds, Context, Div, ElementId, FontWeight, Hsla, ImageSource, Img,
    InteractiveElement, IntoElement, ObjectFit, ParentElement, Pixels, Render, RenderImage,
    SharedString, Stateful, Styled, StyledImage, Transformation, Window,
};

use crate::theme;

/// The header bar's height: 9px above and below its 42px controls, and its 1px bottom border.
pub(crate) const BAR_HEIGHT: Pixels = gpui::px(61.);

/// The header bar along the top of the window.
#[must_use]
pub(crate) fn bar() -> Div {
    gpui::div()
        .h(BAR_HEIGHT)
        .flex_none()
        .flex()
        .items_center()
        .gap(gpui::px(10.))
        .px(gpui::px(16.))
        .border_b_1()
        .border_color(theme::line())
        .bg(theme::surface())
}

const CHEVRON: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M2 4 6 8 10 4" fill="none" stroke="#000" stroke-width="2"/></svg>"##;
const PLAY: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-1 -1 2 2"><path d="M-0.6-1 1 0-0.6 1Z"/></svg>"#;
const ARROW_HEAD: &[u8] =
    br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 6 10"><path d="M0 0 6 5 0 10Z"/></svg>"#;
const RING: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 34 34"><circle cx="17" cy="17" r="15.5" fill="none" stroke="#000" stroke-width="3"/></svg>"##;
/// A quarter of [`RING`], from three o'clock to six.
const ARC: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 34 34"><path d="M32.5 17A15.5 15.5 0 0 1 17 32.5" fill="none" stroke="#000" stroke-width="3"/></svg>"##;

/// A cog, on the buttons that open the settings window: teeth from a dashed ring, around a ring for
/// the body.
pub(crate) const GEAR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="none" stroke="#000" stroke-width="3.5" stroke-dasharray="3.534 3.534"/><circle cx="12" cy="12" r="6" fill="none" stroke="#000" stroke-width="3"/></svg>"##;

/// A small square button with an icon, such as [`GEAR`], tinted `color`.
#[must_use]
pub(crate) fn icon_button(
    id: ElementId,
    icon: &'static [u8],
    size: Pixels,
    color: Hsla,
) -> Stateful<Div> {
    icon_frame(id, size)
        .cursor_pointer()
        .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
        .child(gpui::svg().data(icon).size(size * 0.43).text_color(color))
}

/// The border of an [`icon_button`], with nothing in it and nothing to do.
#[must_use]
pub(crate) fn icon_frame(id: ElementId, size: Pixels) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .relative()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .size(size)
        .rounded(size * 0.25)
        .border_1()
        .border_color(theme::line())
}

/// A tooltip of one short line, for `.tooltip(ui::tip(..))`.
pub(crate) fn tip(text: SharedString) -> impl Fn(&mut Window, &mut App) -> AnyView {
    use gpui::AppContext as _;

    move |_, cx| cx.new(|_| Tip(text.clone())).into()
}

/// The view of a [`tip`]. GPUI draws it apart from the window's root, so it inherits nothing from
/// it and sets its own font.
struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        gpui::div()
            .px(gpui::px(8.))
            .py(gpui::px(5.))
            .rounded(gpui::px(6.))
            .border_1()
            .border_color(theme::line_hover())
            .bg(theme::panel())
            .shadow_md()
            .font_family(theme::FONT)
            .text_size(gpui::px(12.))
            .text_color(theme::text())
            .child(self.0.clone())
    }
}

/// A bordered button with a text label.
#[must_use]
pub(crate) fn button(id: ElementId, label: &'static str) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(gpui::px(36.))
        .px(gpui::px(16.))
        .rounded(gpui::px(8.))
        .border_1()
        .border_color(theme::line_hover())
        .bg(theme::raised())
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).border_color(theme::muted()))
        .child(label)
}

/// An image filling its container, cropped to cover it.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI's ImageSource::Render holds a decoded picture as an Arc<RenderImage>"
    )
)]
#[must_use]
pub(crate) fn cover(image: Arc<RenderImage>) -> Img {
    gpui::img(ImageSource::Render(image))
        .size_full()
        .object_fit(ObjectFit::Cover)
}

/// How an agent's portrait shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shade {
    /// In color.
    Color,
    /// In gray, for an agent with nothing to choose.
    Gray,
}

/// An agent's portrait over their color and initial, which is what shows until it loads.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI's ImageSource::Render holds a decoded picture as an Arc<RenderImage>"
    )
)]
#[must_use]
pub(crate) fn face(
    agent: &Agent,
    portrait: Option<Arc<RenderImage>>,
    shade: Shade,
    size: Pixels,
    radius: Pixels,
) -> Div {
    let color = theme::agent(agent.color);
    let initial: String = agent.name.chars().take(1).collect();
    gpui::div()
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
        .text_color(theme::mix(color, theme::text(), 0.7))
        .child(initial)
        .children(portrait.map(|p| {
            gpui::div()
                .absolute()
                .inset_0()
                .child(cover(p).grayscale(shade == Shade::Gray))
        }))
}

/// A small uppercase caption, such as the label over a picker's value.
#[must_use]
pub(crate) fn label(text: &str) -> Div {
    gpui::div()
        .text_size(gpui::px(10.))
        .line_height(gpui::px(12.))
        .text_color(theme::faint())
        .child(text.to_uppercase())
}

/// A count, in the monospaced face.
#[must_use]
pub(crate) fn num(text: SharedString) -> Div {
    gpui::div()
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(gpui::px(11.))
        .text_color(theme::faint())
        .child(text)
}

/// A small downward chevron.
pub(crate) fn chevron(color: Hsla) -> impl IntoElement {
    gpui::svg()
        .data(CHEVRON)
        .size(gpui::px(12.))
        .ml(gpui::px(2.))
        .text_color(color)
}

/// Records its parent's bounds each frame, for sizing or placing something by it. Layout that
/// reads the bounds sees the previous frame's, so a change asks for one more frame, which draws
/// the measuring view again; a refresh during a frame would do nothing.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "the canvas GPUI owns for one frame writes the bounds the view reads in the next"
    )
)]
pub(crate) fn measure(into: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    gpui::canvas(
        move |bounds, window, _| {
            if into.replace(bounds) != bounds {
                window.request_animation_frame();
            }
        },
        |_, (), _, _| {},
    )
    .absolute()
    .inset_0()
}

/// How a [`kbd`] key cap shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cap {
    /// Muted, as in the hints.
    Plain,
    /// In the text color, for a choice that is on.
    Lit,
}

/// A key cap, as in the keyboard hints.
#[must_use]
pub(crate) fn kbd(key: SharedString, cap: Cap) -> Div {
    gpui::div()
        .min_w(gpui::px(18.))
        .px(gpui::px(4.))
        .py(gpui::px(1.))
        .flex()
        .justify_center()
        .rounded(gpui::px(4.))
        .border_1()
        .border_b_2()
        .border_color(theme::line())
        .font_family(theme::MONO_FONT)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(gpui::px(10.))
        .line_height(gpui::px(12.))
        .text_color(match cap {
            Cap::Lit => theme::text(),
            Cap::Plain => theme::muted(),
        })
        .child(key)
}

/// A play triangle centered in its parent, over a picture or clip, which is dark in either theme.
pub(crate) fn play_glyph(size: Pixels) -> impl IntoElement {
    gpui::div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui::svg()
                .data(PLAY)
                .size(size)
                .text_color(gpui::rgb(theme::DARK.text)),
        )
}

/// A short arrow fading from `from` to `to`, pointing right.
#[must_use]
pub(crate) fn arrow(from: Hsla, to: Hsla) -> Div {
    gpui::div()
        .relative()
        .w(gpui::px(34.))
        .h(gpui::px(10.))
        .child(
            gpui::div()
                .absolute()
                .left_0()
                .right(gpui::px(4.))
                .top(gpui::px(4.))
                .h(gpui::px(2.))
                .bg(gpui::linear_gradient(
                    90.,
                    gpui::linear_color_stop(from, 0.),
                    gpui::linear_color_stop(to, 1.),
                )),
        )
        .child(
            gpui::svg()
                .data(ARROW_HEAD)
                .absolute()
                .right_0()
                .top_0()
                .w(gpui::px(6.))
                .h(gpui::px(10.))
                .text_color(to),
        )
}

/// When every spinner's turn is measured from, so redraws do not restart it.
static SPINNER_EPOCH: OnceLock<Instant> = OnceLock::new();

/// A turning ring in `color`, centered in its parent, for something on its way. It turns once
/// every 800 ms, and asks for the next frame each time it is drawn.
pub(crate) fn spinner(size: Pixels, color: Hsla) -> impl IntoElement {
    let t = SPINNER_EPOCH
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs_f32();
    let turn = (t / 0.8).fract() * TAU;
    gpui::div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui::div()
                .relative()
                .size(size)
                .child(
                    gpui::svg()
                        .data(RING)
                        .absolute()
                        .inset_0()
                        .text_color(theme::alpha(color, 0.19)),
                )
                .child(
                    gpui::svg()
                        .data(ARC)
                        .absolute()
                        .inset_0()
                        .text_color(color)
                        .with_transformation(Transformation::rotate(gpui::radians(turn))),
                )
                .child(gpui::canvas(
                    |_, _, _| {},
                    |_, (), window, _| window.request_animation_frame(),
                )),
        )
}
