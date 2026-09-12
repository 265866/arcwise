//! Arcwise: a desktop lineup browser for Valorant.

mod theme;

use gpui::{
    App, Bounds, Context, FontWeight, Hsla, PathBuilder, Pixels, TitlebarOptions, Window,
    WindowBounds, WindowOptions, canvas, div, point, prelude::*, px, size,
};

struct Arcwise;

impl Render for Arcwise {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family(theme::FONT)
            .text_size(px(14.))
            .child(
                div()
                    .h(px(60.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(16.))
                    .border_b_1()
                    .border_color(theme::line())
                    .bg(theme::panel())
                    .child(brand()),
            )
    }
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
fn diamond(size: Pixels, color: impl Into<Hsla>) -> impl IntoElement {
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
fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Arcwise".into()),
                    ..TitlebarOptions::default()
                }),
                ..WindowOptions::default()
            },
            |_, cx| cx.new(|_| Arcwise),
        )
        .expect("open the main window");
        cx.activate(true);
    });
}
