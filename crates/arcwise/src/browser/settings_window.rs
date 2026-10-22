//! The settings window: a dialog over the map, opened from the gear at the right of the header,
//! with a tab for each kind of setting down its left side.

use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, Div, FontWeight, Hitbox, HitboxBehavior,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString,
    Stateful, WeakEntity, Window, canvas, div, prelude::*, px, svg,
};

use super::{
    Browser, ChangeLibrary, Relaunch, SetAutoUpdate, SetBlur, SetBlurStrength, SetCustomTitleBar,
    SetTheme, plural,
};
use crate::settings::Settings;
use crate::updater::State;
use crate::{theme, titlebar, ui, update};

/// A ring around an arrow pointing up, on the gear while an update waits to be installed.
const UPDATE_BADGE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10.5" fill="none" stroke="#000" stroke-width="2.5"/><path d="M12 17V7.5M8 11.5l4-4 4 4" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// The gear button's fill and border, notched where [`UPDATE_BADGE`] sits on its corner so the
/// badge shows what is behind it rather than a line through it.
const NOTCHED_FILL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect width="42" height="42" rx="10.5" mask="url(#m)"/></svg>"##;
const NOTCHED_BORDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect x="0.5" y="0.5" width="41" height="41" rx="10" fill="none" stroke="#000" stroke-width="1" mask="url(#m)"/></svg>"##;

/// The app icon's arc and diamond without its square, as two masks to tint apart.
const LOGO_ARC: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><path d="M50 206 Q 104 30 182 146" fill="none" stroke="#000" stroke-width="20" stroke-linecap="round"/></svg>"##;
const LOGO_DIAMOND: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><rect x="170" y="150" width="50" height="50" rx="6" transform="rotate(45 195 175)"/></svg>"##;

/// The tabs' icons: a folder, a half-filled circle, and a ring around an arrow pointing up.
const FOLDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" fill="none" stroke="#000" stroke-width="2" stroke-linejoin="round"/></svg>"##;
const CONTRAST: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" fill="none" stroke="#000" stroke-width="2"/><path d="M12 3.5a8.5 8.5 0 0 0 0 17z"/></svg>"##;
const UPDATES: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8.5" fill="none" stroke="#000" stroke-width="2"/><path d="M12 16V8.5M8.75 11.5 12 8.25l3.25 3.25" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
const KEYBOARD: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><rect x="2.5" y="6" width="19" height="12" rx="2" fill="none" stroke="#000" stroke-width="2"/><path d="M6.5 10h.01M10.5 10h.01M14.5 10h.01M18 10h.01M7.5 14h9" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>"##;
/// A cross, on the button that closes the window.
const CROSS: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round"/></svg>"##;

/// The strength slider's knob, which hangs half its width past each end of the track.
const KNOB: Pixels = px(14.);

/// The window's largest size; it shrinks with Arcwise's, and its tabs scroll when they do not fit.
pub(super) const WIDTH: Pixels = px(860.);
const HEIGHT: Pixels = px(600.);

/// A page of the settings window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    General,
    Appearance,
    Keyboard,
    Updates,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::General, Tab::Appearance, Tab::Keyboard, Tab::Updates];

    fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Keyboard => "Keyboard",
            Tab::Updates => "Updates",
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Tab::General => FOLDER,
            Tab::Appearance => CONTRAST,
            Tab::Keyboard => KEYBOARD,
            Tab::Updates => UPDATES,
        }
    }
}

impl Browser {
    /// The gear that opens and closes the settings window, with a badge while an update waits to
    /// be installed.
    pub(super) fn settings_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.settings_open;
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.settings_open {
                        this.close_settings(cx);
                    } else {
                        this.open_settings(None, cx);
                    }
                }),
            )
    }

    /// Opens the settings window, at `tab` or else the one open last, unless a chooser that
    /// cannot close yet, such as the agent's after a map with no lineups for them, keeps it shut
    /// until a choice is made. Returns whether it opened.
    pub fn open_settings(&mut self, tab: Option<Tab>, cx: &mut Context<Self>) -> bool {
        self.lightbox = None;
        self.browse.dismiss_chooser(&self.library);
        self.settings_open = self.browse.chooser().is_none();
        if let Some(tab) = tab
            && tab != self.settings_tab
        {
            self.stop_recording();
            self.settings_tab = tab;
        }
        cx.notify();
        self.settings_open
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.end_strength_drag(cx);
        self.stop_recording();
        self.settings_open = false;
        cx.notify();
    }

    /// The settings window and the shade behind it, which closes it when clicked. Below the
    /// header, which stays usable as the window's title bar.
    pub(super) fn settings_window(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        if !self.settings_open {
            return None;
        }
        let tab = self.settings_tab;
        let body = match tab {
            Tab::General => self.general(cx),
            Tab::Appearance => self.appearance(cx),
            Tab::Keyboard => self.keyboard(window, cx),
            Tab::Updates => self.updates(cx),
        };
        let card = div()
            .id("settings-window")
            .w_full()
            .max_w(WIDTH)
            .h_full()
            .max_h(HEIGHT)
            .flex()
            .rounded(px(16.))
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .overflow_hidden()
            .occlude()
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(px(26.))
                            .pt(px(18.))
                            .pb(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .font_family(theme::DISPLAY_FONT)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(20.))
                                    .child(tab.label()),
                            )
                            .child(
                                ui::icon_button("close-settings", CROSS, px(32.), theme::muted())
                                    .tooltip(ui::tip("Close (Esc)"))
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.close_settings(cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(px(26.))
                            .pb(px(24.))
                            .child(body),
                    ),
            );
        Some(
            div()
                .id("settings-shade")
                .absolute()
                .inset_0()
                .top(ui::BAR_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(theme::scrim())
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_settings(cx)),
                )
                .child(card),
        )
    }

    /// The tabs down the left, with the version running at the bottom.
    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let waiting = self.updater.read(cx).waiting();
        let tabs = Tab::ALL.map(|tab| {
            let on = tab == self.settings_tab;
            div()
                .id(tab.label())
                .flex()
                .items_center()
                .gap(px(10.))
                .h(px(36.))
                .px(px(10.))
                .rounded(px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(theme::raised()))
                .when(!on, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                })
                .child(svg().data(tab.icon()).size(px(16.)).text_color(if on {
                    theme::text()
                } else {
                    theme::muted()
                }))
                .child(div().flex_1().child(tab.label()))
                // The gear's badge, again beside the tab that says what it is about.
                .when(tab == Tab::Updates && waiting, |d| {
                    d.child(div().size(px(7.)).rounded_full().bg(theme::bright()))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.stop_recording();
                    this.settings_tab = tab;
                    cx.notify();
                }))
        });
        div()
            .w(px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(12.))
            .pt(px(18.))
            .pb(px(14.))
            .border_r_1()
            .border_color(theme::line())
            .bg(theme::bg())
            .child(
                div()
                    .px(px(10.))
                    .pb(px(14.))
                    .font_family(theme::DISPLAY_FONT)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(18.))
                    .child("Settings"),
            )
            .children(tabs)
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .text_size(px(12.))
                    .text_color(theme::faint())
                    .child(logo(px(16.)))
                    .child(concat!("Arcwise ", env!("CARGO_PKG_VERSION"))),
            )
    }

    fn general(&self, cx: &mut Context<Self>) -> AnyElement {
        let library = &self.library;
        let root = library.root();
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let counts = format!(
            "{} · {}",
            plural(library.lineups().len(), "lineup"),
            plural(library.agents().len(), "agent")
        );
        group(
            "Library",
            vec![
                setting()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
                            .child(
                                div()
                                    .font_family(theme::MONO_FONT)
                                    .text_size(px(12.))
                                    .text_color(theme::muted())
                                    .truncate()
                                    .child(root.display().to_string()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::faint())
                                    .child(counts),
                            ),
                    )
                    .child(
                        ui::button("change-library", "Change…").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.close_settings(cx);
                                cx.emit(ChangeLibrary);
                            },
                        )),
                    )
                    .into_any_element(),
            ],
        )
        .into_any_element()
    }

    fn appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let blur = theme::blur();
        let relaunch = (titlebar::chosen() != titlebar::custom()).then(|| {
            text_button("relaunch", "Relaunch").on_click(cx.listener(|_, _, _, cx| {
                // The row around it switches the title bar when clicked.
                cx.stop_propagation();
                cx.emit(Relaunch);
            }))
        });
        let mut window_rows = vec![
            toggle(
                "title-bar",
                "Custom title bar",
                "The header doubles as the title bar. Applies after a relaunch",
            )
            .children(relaunch)
            .child(switch(titlebar::chosen()))
            .on_click(cx.listener(|_, _, _, cx| {
                let on = !titlebar::chosen();
                titlebar::choose(on);
                cx.emit(SetCustomTitleBar(on));
                cx.notify();
            }))
            .into_any_element(),
            toggle(
                "blur",
                "Background blur",
                "Shows what is behind the window, blurred",
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
            }))
            .into_any_element(),
        ];
        if blur {
            window_rows.push(self.strength_row(cx).into_any_element());
        }
        div()
            .child(group(
                "Theme",
                vec![
                    setting()
                        .child(name("Theme"))
                        .child(
                            div()
                                .flex()
                                .gap(px(2.))
                                .p(px(3.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme::line_hover())
                                .bg(theme::panel())
                                .child(mode_button(theme::Mode::Dark, "Dark", cx))
                                .child(mode_button(theme::Mode::Light, "Light", cx)),
                        )
                        .into_any_element(),
                ],
            ))
            .child(group("Window", window_rows))
            .into_any_element()
    }

    fn updates(&self, cx: &mut Context<Self>) -> AnyElement {
        let updater = self.updater.read(cx);
        let automatic = updater.automatic();
        let about = if automatic {
            "Downloads new versions in the background and installs them when Arcwise closes"
        } else {
            "Says when a new version is out, and installs it when you ask"
        };
        let running = if update::installed() {
            None
        } else {
            Some("Not run from where it was installed, so it does not update itself")
        };
        let newer = match updater.state() {
            State::UpToDate => None,
            State::Available(version) => Some((
                format!("Arcwise {version} is available"),
                "Install it now, and Arcwise opens again on the new version",
                "Install and relaunch",
                false,
            )),
            State::Downloading { version, .. } => Some((
                format!("Arcwise {version} is downloading"),
                "Click to install it as soon as it has downloaded",
                "Install and relaunch",
                true,
            )),
            State::Ready(update) => Some((
                format!("Arcwise {} is ready", update.version),
                "It installs when you close Arcwise",
                "Restart now",
                false,
            )),
        };
        let newer = newer.map(|(title, about, action, downloading)| {
            div()
                .mt(px(14.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::line_strong())
                .bg(theme::raised())
                .child(
                    setting().child(described(title, about)).child(
                        ui::button("install-update", action)
                            .when(downloading, |b| {
                                b.child(
                                    div()
                                        .relative()
                                        .size(px(14.))
                                        .ml(px(8.))
                                        .child(ui::spinner(px(14.), theme::muted())),
                                )
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.updater
                                    .update(cx, |updater, cx| updater.install_now(cx));
                            })),
                    ),
                )
        });
        div()
            .child(group(
                "Updates",
                vec![
                    toggle("auto-update", "Automatic updates", about)
                        .child(switch(automatic))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = !this.updater.read(cx).automatic();
                            this.updater
                                .update(cx, |updater, cx| updater.set_automatic(on, cx));
                            cx.emit(SetAutoUpdate(on));
                        }))
                        .into_any_element(),
                    setting()
                        .gap(px(12.))
                        .child(logo(px(20.)))
                        .child(described(
                            concat!("Arcwise ", env!("CARGO_PKG_VERSION")),
                            running.unwrap_or("The version running now"),
                        ))
                        .into_any_element(),
                ],
            ))
            .children(newer)
            .into_any_element()
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

    /// How much of the blur shows, from 0 to 100: a track to click or drag along, which changes
    /// the window as it moves and is saved when let go.
    fn strength_row(&self, cx: &mut Context<Self>) -> Div {
        let strength = theme::strength();
        let along = f32::from(strength) / 100.;
        let browser = cx.entity().downgrade();
        let bar = |d: Div| d.absolute().left_0().top(px(9.)).h(px(4.)).rounded_full();
        setting()
            .child(name("Strength"))
            .child(
                div()
                    .relative()
                    .w(px(260.))
                    .h(px(22.))
                    .cursor_pointer()
                    .child(bar(div()).right_0().bg(theme::line_hover()))
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
                            // Under the pointer only where the track shows, not where the tab has
                            // scrolled it out of sight.
                            |bounds, window, _| {
                                let hitbox = window.insert_hitbox(
                                    bounds.dilate(KNOB / 2.),
                                    HitboxBehavior::Normal,
                                );
                                (bounds, hitbox)
                            },
                            move |_, (bounds, hitbox), window, _| {
                                track_strength(bounds, hitbox, browser, window);
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
}

/// Follows the pointer along the strength track at `bounds`: a press on the track, where `hitbox`
/// is under the pointer, starts a drag, which follows the pointer anywhere in the window until the
/// button is let go.
fn track_strength(
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    browser: WeakEntity<Browser>,
    window: &mut Window,
) {
    let along = move |x: Pixels| (x - bounds.left()) / bounds.size.width;
    window.on_mouse_event({
        let browser = browser.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox.is_hovered(window)
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

/// The app's mark, `size` across.
fn logo(size: Pixels) -> Div {
    div()
        .relative()
        .flex_none()
        .size(size)
        .child(layer(LOGO_ARC).text_color(theme::text()))
        .child(layer(LOGO_DIAMOND).text_color(theme::accent()))
}

/// A titled card of settings, one to a row with lines between them.
fn group(title: &str, rows: Vec<AnyElement>) -> Div {
    let rows = rows.into_iter().enumerate().map(|(i, row)| {
        div()
            .when(i > 0, |d| d.border_t_1().border_color(theme::line()))
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .mt(px(14.))
        .child(ui::label(title).px(px(2.)).font_weight(FontWeight::BOLD))
        .child(
            div()
                .rounded(px(12.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .overflow_hidden()
                .children(rows),
        )
}

/// A row in a [`group`]: what it sets, then its control.
fn setting() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .min_h(px(54.))
        .px(px(16.))
        .py(px(10.))
}

/// A row that turns something on or off when clicked anywhere along it.
fn toggle(id: &'static str, label: &'static str, about: impl Into<SharedString>) -> Stateful<Div> {
    setting()
        .id(id)
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(described(label, about))
}

/// A setting's name, taking the room its control leaves.
fn name(text: &'static str) -> Div {
    div().flex_1().font_weight(FontWeight::SEMIBOLD).child(text)
}

/// A setting's name with a line under it saying what it does.
fn described(title: impl Into<SharedString>, about: impl Into<SharedString>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
        .child(
            div()
                .text_size(px(12.5))
                .text_color(theme::muted())
                .child(about.into()),
        )
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
        .hover(|s| s.bg(theme::panel()).text_color(theme::text()))
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
        .when(on, |d| d.bg(theme::hover()))
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
            d.bg(theme::panel()).border_color(theme::line_hover())
        })
        // Light on the accent in either theme.
        .child(div().size(px(14.)).rounded_full().bg(if on {
            gpui::rgb(theme::DARK.text)
        } else {
            theme::muted()
        }))
}
