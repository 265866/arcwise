//! The settings menu, opened from the gear at the right of the header.

use gpui::{
    Anchor, Bounds, Context, DispatchPhase, Div, FontWeight, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Stateful, WeakEntity, Window, anchored,
    canvas, deferred, div, point, prelude::*, px, svg,
};

use super::{
    Browser, ChangeLibrary, Relaunch, SetAutoUpdate, SetBlur, SetBlurStrength, SetCustomTitleBar,
    SetTheme,
};
use crate::settings::Settings;
use crate::updater::State;
use crate::{theme, titlebar, ui};

/// A ring around an arrow pointing up, on the gear while an update waits to be installed.
const UPDATE_BADGE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10.5" fill="none" stroke="#000" stroke-width="2.5"/><path d="M12 17V7.5M8 11.5l4-4 4 4" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// The gear button's fill and border, notched where [`UPDATE_BADGE`] sits on its corner so the
/// badge shows what is behind it rather than a line through it.
const NOTCHED_FILL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect width="42" height="42" rx="10.5" mask="url(#m)"/></svg>"##;
const NOTCHED_BORDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect x="0.5" y="0.5" width="41" height="41" rx="10" fill="none" stroke="#000" stroke-width="1" mask="url(#m)"/></svg>"##;

/// An arrow down onto a line, on the button that installs an update.
const DOWNLOAD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 4v11M7 10.5l5 5 5-5M5 20h14" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// The app icon's arc and diamond without its square, as two masks to tint apart.
const LOGO_ARC: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><path d="M50 206 Q 104 30 182 146" fill="none" stroke="#000" stroke-width="20" stroke-linecap="round"/></svg>"##;
const LOGO_DIAMOND: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><rect x="170" y="150" width="50" height="50" rx="6" transform="rotate(45 195 175)"/></svg>"##;

/// The strength slider's knob, which hangs half its width past each end of the track.
const KNOB: Pixels = px(14.);

impl Browser {
    /// The gear that opens and closes the menu, with the menu under it while open, and a badge
    /// while an update waits to be installed.
    pub(super) fn menu_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.menu_open;
        let waiting = self.updater.read(cx).waiting();
        div()
            .id("settings")
            // Over the header, which moves the window where nothing else is.
            .occlude()
            .group("settings")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(px(42.))
            .cursor_pointer()
            .map(|d| {
                if waiting {
                    let (fill, border) = if open {
                        (theme::raised().into(), theme::line_hover())
                    } else {
                        (gpui::transparent_black(), theme::line())
                    };
                    d.child(
                        layer(NOTCHED_FILL)
                            .text_color(fill)
                            .group_hover("settings", |s| s.text_color(theme::raised())),
                    )
                    .child(
                        layer(NOTCHED_BORDER)
                            .text_color(border)
                            .group_hover("settings", |s| s.text_color(theme::line_hover())),
                    )
                } else {
                    d.rounded(px(10.5))
                        .border_1()
                        .border_color(theme::line())
                        .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
                        .when(open, |d| {
                            d.bg(theme::raised()).border_color(theme::line_hover())
                        })
                }
            })
            .child(svg().data(ui::GEAR).size(px(18.)).text_color(if open {
                theme::text()
            } else {
                theme::muted()
            }))
            // Over the corner, with the right tip of its arrowhead on the border.
            .when(waiting, |d| {
                d.child(
                    svg()
                        .data(UPDATE_BADGE)
                        .absolute()
                        .top(px(-4.))
                        .right(px(-4.))
                        .size(px(14.))
                        .text_color(theme::bright()),
                )
            })
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
                    this.lightbox = None;
                    if this.menu_open {
                        this.menu_open = false;
                        cx.notify();
                    } else {
                        this.open_menu(cx);
                    }
                }),
            )
    }

    /// Opens the settings menu, unless a chooser that cannot close yet, such as the agent's after
    /// a map with no lineups for them, keeps it shut until a choice is made. Returns whether it
    /// opened.
    pub fn open_menu(&mut self, cx: &mut Context<Self>) -> bool {
        self.browse.dismiss_chooser(&self.library);
        self.menu_open = self.browse.chooser().is_none();
        cx.notify();
        self.menu_open
    }

    fn menu(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let root = self.library.root();
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let blur = theme::blur();
        let automatic = self.updater.read(cx).automatic();
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
            .child(divider())
            .child(
                row()
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Theme"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(2.))
                            .p(px(3.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(theme::line())
                            .child(mode_button(theme::Mode::Dark, "Dark", cx))
                            .child(mode_button(theme::Mode::Light, "Light", cx)),
                    ),
            )
            .child(
                row()
                    .id("title-bar")
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::raised()))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Custom title bar"),
                    )
                    .children((titlebar::chosen() != titlebar::custom()).then(|| {
                        text_button("relaunch", "Relaunch").on_click(cx.listener(|_, _, _, cx| {
                            // The row around it switches the title bar when clicked.
                            cx.stop_propagation();
                            cx.emit(Relaunch);
                        }))
                    }))
                    .child(switch(titlebar::chosen()))
                    .on_click(cx.listener(|_, _, _, cx| {
                        let on = !titlebar::chosen();
                        titlebar::choose(on);
                        cx.emit(SetCustomTitleBar(on));
                        cx.notify();
                    })),
            )
            .child(divider())
            .child(
                row()
                    .id("blur")
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::raised()))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Background blur"),
                    )
                    .children(self.reset_blur(cx))
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
            .child(divider())
            .child(
                row()
                    .id("auto-update")
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::raised()))
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Automatic updates"),
                    )
                    .child(switch(automatic))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let on = !this.updater.read(cx).automatic();
                        this.updater
                            .update(cx, |updater, cx| updater.set_automatic(on, cx));
                        cx.emit(SetAutoUpdate(on));
                    })),
            )
            .child(divider())
            .child(self.version(cx))
    }

    /// Puts background blur and its strength back as they first were. Shown only while either
    /// differs.
    fn reset_blur(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let defaults = Settings::default();
        if theme::blur() == defaults.blur && theme::strength() == defaults.blur_strength {
            return None;
        }
        let reset =
            text_button("reset-blur", "Reset").on_click(cx.listener(move |_, _, window, cx| {
                // The row around it turns blur on or off when clicked.
                cx.stop_propagation();
                theme::set_blur(defaults.blur);
                theme::set_strength(defaults.blur_strength);
                window.set_background_appearance(theme::window_background());
                window.refresh();
                cx.emit(SetBlur(defaults.blur));
                cx.emit(SetBlurStrength(defaults.blur_strength));
            }));
        Some(reset)
    }

    /// The logo and the version running, with a button that updates to a newer one and relaunches.
    fn version(&self, cx: &mut Context<Self>) -> Div {
        let (newer, downloading) = match self.updater.read(cx).state() {
            State::UpToDate => (None, false),
            State::Available(version) => (Some(version.clone()), false),
            State::Downloading { version, .. } => (Some(version.clone()), true),
            State::Ready(update) => (Some(update.version.clone()), false),
        };
        row()
            .gap(px(10.))
            .py(px(6.))
            .child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(20.))
                    .child(layer(LOGO_ARC).text_color(theme::text()))
                    .child(layer(LOGO_DIAMOND).text_color(theme::accent())),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(13.))
                    .text_color(theme::muted())
                    .child(concat!("Arcwise ", env!("CARGO_PKG_VERSION"))),
            )
            .children(newer.map(|version| {
                let tip = ui::tip(format!("Update to {version} and relaunch"));
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::faint())
                            .child(format!("{version} available")),
                    )
                    .child(
                        if downloading {
                            // Clicked mid-download, it installs once the download is verified.
                            ui::icon_frame("update-now", px(30.))
                                .cursor_pointer()
                                .child(ui::spinner(px(14.), theme::text()))
                        } else {
                            ui::icon_button("update-now", DOWNLOAD, px(30.), theme::text())
                        }
                        .tooltip(tip)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.updater
                                .update(cx, |updater, cx| updater.install_now(cx));
                        })),
                    )
            }))
    }

    /// How much of the blur shows, from 0 to 100: a track to click or drag along, which changes
    /// the window as it moves and is saved when let go.
    fn strength_row(&self, cx: &mut Context<Self>) -> Div {
        let strength = theme::strength();
        let along = f32::from(strength) / 100.;
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
                    .child(bar(div()).w(gpui::relative(along)).bg(theme::accent()))
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .left(gpui::relative(along))
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
                ui::num(format!("{strength}%"))
                    .w(px(34.))
                    .flex()
                    .justify_end(),
            )
    }

    /// Sets the strength from a point `along` the track, 0 at its left end and 1 at its right.
    fn set_strength(&mut self, along: f32, window: &mut Window) {
        if along.is_nan() {
            return;
        }
        let strength = (along.clamp(0., 1.) * 100.).round() as u8;
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

/// An SVG filling its parent, to stack with others and tint apart.
fn layer(data: &'static [u8]) -> gpui::Svg {
    svg().data(data).absolute().inset_0().size_full()
}

/// A small text button at the end of a row, beside its switch.
fn text_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        // As tall as the switch beside it, so the row keeps its height when this shows.
        .h(px(22.))
        .flex()
        .items_center()
        .px(px(8.))
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
        .child(label)
}

/// One side of the theme picker, which switches every color at once.
fn mode_button(mode: theme::Mode, label: &'static str, cx: &mut Context<Browser>) -> Stateful<Div> {
    let on = theme::mode() == mode;
    div()
        .id(label)
        .h(px(26.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if on { theme::text() } else { theme::muted() })
        .when(on, |d| d.bg(theme::raised()))
        .when(!on, |d| {
            d.cursor_pointer().hover(|s| s.text_color(theme::text()))
        })
        .child(label)
        .on_click(cx.listener(move |_, _, window, cx| {
            if theme::mode() == mode {
                return;
            }
            theme::set_mode(mode);
            titlebar::sync_native(window);
            // Every view draws again, cached ones too, in the new colors.
            window.refresh();
            cx.emit(SetTheme(mode));
        }))
}

/// A line between the menu's sections.
fn divider() -> Div {
    div().h(px(1.)).mx(px(10.)).bg(theme::line())
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
        // Light on the accent in either theme.
        .child(div().size(px(14.)).rounded_full().bg(if on {
            gpui::rgb(theme::DARK.text)
        } else {
            theme::muted()
        }))
}
