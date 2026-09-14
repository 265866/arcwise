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

/// The header bar along the top of the window.
pub fn bar() -> Div {
    div()
        .h(px(60.))
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

/// Records its parent's bounds each frame, for placing something next to it.
pub fn measure(into: Rc<Cell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(move |bounds, _, _| into.set(bounds), |_, _, _, _| {})
        .absolute()
        .inset_0()
}
