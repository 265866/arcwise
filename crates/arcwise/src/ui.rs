//! Small pieces of UI used across the window.

use gpui::{
    Div, ElementId, FontWeight, Hsla, PathBuilder, Pixels, Stateful, canvas, div, point,
    prelude::*, px,
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
