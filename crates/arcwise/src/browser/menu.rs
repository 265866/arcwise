//! The settings menu, opened from the gear at the right of the header.

use gpui::{
    Anchor, Context, Div, FontWeight, IntoElement, MouseButton, Stateful, anchored, deferred, div,
    point, prelude::*, px, svg,
};

use super::{Browser, ChangeLibrary, SetBlur};
use crate::{theme, ui};

/// A cog: teeth from a dashed ring, around a ring for the body.
const GEAR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="none" stroke="#000" stroke-width="3.5" stroke-dasharray="3.534 3.534"/><circle cx="12" cy="12" r="6" fill="none" stroke="#000" stroke-width="3"/></svg>"##;

impl Browser {
    /// The gear that opens and closes the menu, with the menu under it while open.
    pub(super) fn menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.menu_open;
        div()
            .id("settings")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(px(42.))
            .rounded(px(10.))
            .border_1()
            .border_color(theme::line())
            .cursor_pointer()
            .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
            .when(open, |d| {
                d.bg(theme::raised()).border_color(theme::line_hover())
            })
            .child(svg().data(GEAR).size(px(18.)).text_color(if open {
                theme::text()
            } else {
                theme::muted()
            }))
            .children(open.then(|| {
                div()
                    .absolute()
                    .right_0()
                    .top(gpui::relative(1.))
                    .child(deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .offset(point(px(0.), px(8.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(self.menu(cx)),
                    ))
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.menu_open {
                        this.menu_open = false;
                    } else {
                        // A chooser that cannot close yet, such as the agent's after a map with
                        // no lineups for them, keeps the menu shut until a choice is made.
                        this.browse.dismiss_chooser(&this.library);
                        this.menu_open = this.browse.chooser().is_none();
                    }
                    cx.notify();
                }),
            )
    }

    fn menu(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let root = self.library.root();
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let blur = theme::blur();
        div()
            .id("menu")
            .w(px(340.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .p(px(8.))
            .rounded(px(12.))
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .occlude()
            .child(
                row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(ui::label("Library"))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .truncate()
                                    .child(root.display().to_string()),
                            ),
                    )
                    .child(
                        ui::button("change-library", "Change…").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.menu_open = false;
                                cx.emit(ChangeLibrary);
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(div().h(px(1.)).mx(px(10.)).bg(theme::line()))
            .child(
                row()
                    .id("blur")
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::raised()))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Background blur"),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .child("Show what is behind the window, blurred"),
                            ),
                    )
                    .child(switch(blur))
                    .on_click(cx.listener(|_, _, window, cx| {
                        let on = !theme::blur();
                        theme::set_blur(on);
                        window.set_background_appearance(theme::window_background());
                        // Every view draws again, cached ones too, in the new colors.
                        window.refresh();
                        cx.emit(SetBlur(on));
                    })),
            )
    }

    /// While the menu is open, the layer under it that closes it when clicked. It covers the
    /// board but not the header, like a chooser's dropdown.
    pub(super) fn menu_layer(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        self.menu_open.then(|| {
            div()
                .absolute()
                .top(ui::BAR_HEIGHT)
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.menu_open = false;
                        cx.notify();
                    }),
                )
        })
    }
}

/// A line in the menu: what it sets, then its control.
fn row() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(10.))
        .py(px(10.))
        .rounded(px(8.))
}

/// An on and off switch.
fn switch(on: bool) -> Div {
    div()
        .flex_none()
        .flex()
        .items_center()
        .w(px(38.))
        .h(px(22.))
        .px(px(3.))
        .rounded_full()
        .border_1()
        .when(on, |d| {
            d.justify_end()
                .bg(theme::accent())
                .border_color(theme::accent())
        })
        .when(!on, |d| {
            d.bg(theme::raised()).border_color(theme::line_hover())
        })
        .child(div().size(px(14.)).rounded_full().bg(if on {
            theme::text()
        } else {
            theme::muted()
        }))
}
