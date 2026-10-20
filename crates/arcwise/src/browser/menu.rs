//! The settings menu, opened from the gear at the right of the header.

use gpui::{
    Anchor, Bounds, Context, DispatchPhase, Div, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Stateful, WeakEntity, Window, anchored,
    canvas, deferred, div, point, prelude::*, px, svg,
};

use super::{Browser, ChangeLibrary, SetBlur, SetBlurStrength};
use crate::settings::Settings;
use crate::{theme, ui};

/// A cog: teeth from a dashed ring, around a ring for the body.
const GEAR: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="9" fill="none" stroke="#000" stroke-width="3.5" stroke-dasharray="3.534 3.534"/><circle cx="12" cy="12" r="6" fill="none" stroke="#000" stroke-width="3"/></svg>"##;

/// The strength slider's knob, which hangs half its width past each end of the track.
const KNOB: Pixels = px(14.);

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
            .when(blur, |menu| menu.child(self.strength_row(cx)))
            .child(div().h(px(1.)).mx(px(10.)).bg(theme::line()))
            .child(row().justify_end().child(self.reset_button(cx)))
    }

    /// Puts every setting but the library back as it first was. Dimmed while they already are.
    fn reset_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let defaults = Settings::default();
        let changed = theme::blur() != defaults.blur || theme::strength() != defaults.blur_strength;
        if !changed {
            return ui::idle_button("reset-settings", "Reset to defaults");
        }
        ui::button("reset-settings", "Reset to defaults").on_click(cx.listener(
            move |_, _, window, cx| {
                theme::set_blur(defaults.blur);
                theme::set_strength(defaults.blur_strength);
                window.set_background_appearance(theme::window_background());
                window.refresh();
                cx.emit(SetBlur(defaults.blur));
                cx.emit(SetBlurStrength(defaults.blur_strength));
            },
        ))
    }

    /// How much of the blur shows: a track to click or drag along, which changes the window as
    /// it moves and is saved when let go.
    fn strength_row(&self, cx: &mut Context<Self>) -> Div {
        let strength = theme::strength();
        let browser = cx.entity().downgrade();
        let bar = |d: Div| d.absolute().left_0().top(px(9.)).h(px(4.)).rounded_full();
        row()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::muted())
                    .child("Strength"),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(px(22.))
                    .cursor_pointer()
                    .child(bar(div()).right_0().bg(theme::raised()))
                    .child(bar(div()).w(gpui::relative(strength)).bg(theme::accent()))
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .left(gpui::relative(strength))
                            .ml(-KNOB / 2.)
                            .size(KNOB)
                            .rounded_full()
                            .bg(theme::text()),
                    )
                    .child(
                        canvas(
                            |bounds, _, _| bounds,
                            move |_, bounds, window, _| {
                                track_strength(bounds, browser, window);
                            },
                        )
                        .absolute()
                        .inset_0(),
                    ),
            )
            .child(
                ui::num(format!("{:.0}%", strength * 100.))
                    .w(px(34.))
                    .flex()
                    .justify_end(),
            )
    }

    fn set_strength(&mut self, strength: f32, window: &mut Window) {
        if strength.is_nan() {
            return;
        }
        // Whole percents, as the slider shows.
        let strength = (strength.clamp(0., 1.) * 100.).round() / 100.;
        if strength != theme::strength() {
            theme::set_strength(strength);
            window.refresh();
        }
    }

    /// Ends a drag along the strength track, and saves where it stopped.
    pub(super) fn end_strength_drag(&mut self, cx: &mut Context<Self>) {
        if self.strength_drag {
            self.strength_drag = false;
            cx.emit(SetBlurStrength(theme::strength()));
        }
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

/// Follows the pointer along the strength track at `bounds`: a press on the track starts a drag,
/// which follows the pointer anywhere in the window until the button is let go.
fn track_strength(bounds: Bounds<Pixels>, browser: WeakEntity<Browser>, window: &mut Window) {
    let along = move |x: Pixels| (x - bounds.left()) / bounds.size.width;
    window.on_mouse_event({
        let browser = browser.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && bounds.dilate(KNOB / 2.).contains(&event.position)
            {
                browser
                    .update(cx, |this, _| {
                        this.strength_drag = true;
                        this.set_strength(along(event.position.x), window);
                    })
                    .ok();
            }
        }
    });
    window.on_mouse_event({
        let browser = browser.clone();
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            browser
                .update(cx, |this, cx| {
                    if !this.strength_drag {
                        return;
                    }
                    // A release can go missing when another window takes the pointer, as with
                    // Alt+Tab mid-drag.
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.set_strength(along(event.position.x), window);
                    } else {
                        this.end_strength_drag(cx);
                    }
                })
                .ok();
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
            browser
                .update(cx, |this, cx| this.end_strength_drag(cx))
                .ok();
        }
    });
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
