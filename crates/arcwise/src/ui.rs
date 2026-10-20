//! Small pieces of UI used across the window.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use arcwise_core::Agent;
use gpui::{
    Bounds, Div, ElementId, FontWeight, Hsla, ImageSource, ObjectFit, PathBuilder, Pixels,
    RenderImage, SharedString, Stateful, canvas, div, img, point, prelude::*, px,
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
    let color = color.into();
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let c = bounds.center();
            let r = bounds.size.width / 2.;
            let mut path = PathBuilder::fill();
            path.add_polygon(
                &[
                    point(c.x, c.y - r),
                    point(c.x + r, c.y),
                    point(c.x, c.y + r),
                    point(c.x - r, c.y),
                ],
                true,
            );
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(size)
}

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
    let color = color.into();
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let c = bounds.center();
            let r = px(4.);
            let mut path = PathBuilder::stroke(px(2.));
            path.move_to(point(c.x - r, c.y - r / 2.));
            path.line_to(point(c.x, c.y + r / 2.));
            path.line_to(point(c.x + r, c.y - r / 2.));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size(px(12.))
    .ml(px(2.))
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
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let c = bounds.center();
            let h = size / 2.;
            let mut path = PathBuilder::fill();
            path.add_polygon(
                &[
                    point(c.x - h * 0.6, c.y - h),
                    point(c.x + h, c.y),
                    point(c.x - h * 0.6, c.y + h),
                ],
                true,
            );
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::text());
            }
        },
    )
    .absolute()
    .inset_0()
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
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let r = bounds.right();
                    let c = bounds.center().y;
                    let mut path = PathBuilder::fill();
                    path.add_polygon(
                        &[
                            point(r - px(6.), c - px(5.)),
                            point(r, c),
                            point(r - px(6.), c + px(5.)),
                        ],
                        true,
                    );
                    if let Ok(path) = path.build() {
                        window.paint_path(path, to);
                    }
                },
            )
            .absolute()
            .inset_0(),
        )
}

/// A turning ring, centered in its parent, for something on its way.
pub fn spinner(size: Pixels) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            // Turns once every 800 ms, measured from a fixed point so redraws do not restart it.
            static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
            let t = EPOCH
                .get_or_init(std::time::Instant::now)
                .elapsed()
                .as_secs_f32();
            let turn = (t / 0.8).fract() * std::f32::consts::TAU;
            let c = bounds.center();
            let r = size / 2. - px(1.5);
            let ring = |from: f32, sweep: f32, color: Hsla, window: &mut gpui::Window| {
                let mut path = PathBuilder::stroke(px(3.));
                let steps = 24;
                for i in 0..=steps {
                    let a = from + sweep * i as f32 / steps as f32;
                    let p = point(c.x + r * a.cos(), c.y + r * a.sin());
                    if i == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            ring(
                0.,
                std::f32::consts::TAU,
                theme::alpha(gpui::white(), 0.19),
                window,
            );
            ring(
                turn,
                std::f32::consts::FRAC_PI_2,
                theme::text().into(),
                window,
            );
            window.request_animation_frame();
        },
    )
    .absolute()
    .inset_0()
}
