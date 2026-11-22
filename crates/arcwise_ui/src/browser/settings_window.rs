//! The settings window: a dialog over the map, opened from the gear at the right of the header,
//! with a tab for each kind of setting down its left side.

use std::time::Duration;

use arcwise_remote::remote;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Bounds, Context, DispatchPhase, Div, ElementId, FontWeight, Hitbox, HitboxBehavior,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, SharedString, Stateful, StatefulInteractiveElement, Styled, Svg,
    WeakEntity, Window,
};

use super::browser_view::BrowserView;
use super::events::{
    Relaunch, SetAutoUpdate, SetBlur, SetBlurStrength, SetCustomTitleBar, SetReopenLast,
    SetStartSpeed, SetTheme,
};
use super::tab::Tab;
use crate::pacing::Speed;
use crate::settings::Settings;
use crate::shortcuts::{Command, Shortcuts};
use crate::theme::Mode;
use crate::updater::{State, Updater};
use crate::{theme, titlebar, ui};

/// A ring around an arrow pointing up, on the gear while an update waits to be installed.
const UPDATE_BADGE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="10.5" fill="none" stroke="#000" stroke-width="2.5"/><path d="M12 17V7.5M8 11.5l4-4 4 4" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

/// The gear button's fill and border, notched where [`UPDATE_BADGE`] sits on its corner so the
/// badge shows what is behind it rather than a line through it.
const NOTCHED_FILL: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect width="42" height="42" rx="10.5" mask="url(#m)"/></svg>"##;
const NOTCHED_BORDER: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 42 42"><defs><mask id="m"><rect width="42" height="42" fill="#fff"/><circle cx="39" cy="3" r="9.5" fill="#000"/></mask></defs><rect x="0.5" y="0.5" width="41" height="41" rx="10" fill="none" stroke="#000" stroke-width="1" mask="url(#m)"/></svg>"##;

/// The app icon's arc and diamond without its square, as two masks to tint apart.
const LOGO_ARC: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><path d="M50 206 Q 104 30 182 146" fill="none" stroke="#000" stroke-width="20" stroke-linecap="round"/></svg>"##;
const LOGO_DIAMOND: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="35 53 200 200"><rect x="170" y="150" width="50" height="50" rx="6" transform="rotate(45 195 175)"/></svg>"#;

/// A cross, on the button that closes the window.
const CROSS: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round"/></svg>"##;

/// The strength slider's knob, which hangs half its width past each end of the track.
const KNOB: Pixels = gpui::px(14.);

/// The window's largest size; it shrinks with Arcwise's, and its tabs scroll when they do not fit.
pub(super) const WIDTH: Pixels = gpui::px(860.);
const HEIGHT: Pixels = gpui::px(600.);

impl BrowserView {
    /// The gear that opens and closes the settings window, with a badge while an update waits to
    /// be installed. It occludes the header under it, which moves the window where nothing else
    /// is. The badge sits over the corner, with the right tip of its arrowhead on the border.
    pub(super) fn settings_button(&self, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let open = self.settings_open;
        let waiting = self.updater.read(cx).waiting();
        gpui::div()
            .id("settings")
            .occlude()
            .group("settings")
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .size(gpui::px(42.))
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
                    d.rounded(gpui::px(10.5))
                        .border_1()
                        .border_color(theme::line())
                        .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
                        .when(open, |d| {
                            d.bg(theme::raised()).border_color(theme::line_hover())
                        })
                }
            })
            .child(
                gpui::svg()
                    .data(ui::GEAR)
                    .size(gpui::px(18.))
                    .text_color(if open { theme::text() } else { theme::muted() }),
            )
            .when(waiting, |d| {
                d.child(
                    gpui::svg()
                        .data(UPDATE_BADGE)
                        .absolute()
                        .top(gpui::px(-4.))
                        .right(gpui::px(-4.))
                        .size(gpui::px(14.))
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
    pub(crate) fn open_settings(&mut self, tab: Option<Tab>, cx: &mut Context<'_, Self>) -> bool {
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

    /// Keeps the Updates tab's "Checked N minutes ago" current while it shows, and stops when it
    /// does not, by dropping the task. The task wakes just past each whole minute since the check,
    /// which a later check moves.
    pub(super) fn tick_minutes(&mut self, cx: &mut Context<'_, Self>) {
        let showing = self.settings_open
            && self.settings_tab == Tab::Updates
            && self.updater.read(cx).checked().is_some();
        if !showing {
            self.minutes = None;
        } else if self.minutes.is_none() {
            self.minutes = Some(cx.spawn(async move |this, cx| {
                while let Ok(Some(wait)) = this.update(cx, |this, cx| {
                    let since = this.updater.read(cx).checked()?.at.elapsed().as_millis() % 60_000;
                    let to_minute = 60_000 - u64::try_from(since).unwrap_or_default();
                    Some(Duration::from_millis(to_minute + 50))
                }) {
                    cx.background_executor().timer(wait).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<'_, Self>) {
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
        cx: &mut Context<'_, Self>,
    ) -> Option<impl IntoElement> {
        if !self.settings_open {
            return None;
        }
        let tab = self.settings_tab;
        let body = match tab {
            Tab::General => self.general(cx),
            Tab::Appearance => Self::appearance(cx),
            Tab::Playback => self.playback(cx),
            Tab::Keyboard => self.keyboard(window, cx),
            Tab::Updates => self.updates(cx),
        };
        let card = gpui::div()
            .id("settings-window")
            .w_full()
            .max_w(WIDTH)
            .h_full()
            .max_h(HEIGHT)
            .flex()
            .rounded(gpui::px(16.))
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .overflow_hidden()
            .occlude()
            .child(self.sidebar(cx))
            .child(
                gpui::div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        gpui::div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .px(gpui::px(26.))
                            .pt(gpui::px(18.))
                            .pb(gpui::px(6.))
                            .child(
                                gpui::div()
                                    .flex_1()
                                    .font_family(theme::DISPLAY_FONT)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(gpui::px(20.))
                                    .child(tab.label()),
                            )
                            .child(
                                ui::icon_button(
                                    "close-settings".into(),
                                    CROSS,
                                    gpui::px(32.),
                                    theme::muted().into(),
                                )
                                .tooltip(ui::tip("Close (Esc)".into()))
                                .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
                            ),
                    )
                    .child(
                        gpui::div()
                            .id("settings-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px(gpui::px(26.))
                            .pb(gpui::px(24.))
                            .child(body),
                    ),
            );
        Some(
            gpui::div()
                .id("settings-shade")
                .absolute()
                .inset_0()
                .top(ui::BAR_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .p(gpui::px(24.))
                .bg(theme::scrim())
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_settings(cx)),
                )
                .child(card),
        )
    }

    /// The tabs down the left, with the version running at the bottom. The gear's badge shows
    /// again beside the tab that says what it is about.
    fn sidebar(&self, cx: &mut Context<'_, Self>) -> Div {
        let waiting = self.updater.read(cx).waiting();
        let tabs = Tab::ALL.map(|tab| {
            let on = tab == self.settings_tab;
            gpui::div()
                .id(tab.label())
                .flex()
                .items_center()
                .gap(gpui::px(10.))
                .h(gpui::px(36.))
                .px(gpui::px(10.))
                .rounded(gpui::px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(theme::raised()))
                .when(!on, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(theme::hover()).text_color(theme::text()))
                })
                .child(
                    gpui::svg()
                        .data(tab.icon())
                        .size(gpui::px(16.))
                        .text_color(if on { theme::text() } else { theme::muted() }),
                )
                .child(gpui::div().flex_1().child(tab.label()))
                .when(tab == Tab::Updates && waiting, |d| {
                    d.child(
                        gpui::div()
                            .size(gpui::px(7.))
                            .rounded_full()
                            .bg(theme::bright()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.stop_recording();
                    this.settings_tab = tab;
                    cx.notify();
                }))
        });
        gpui::div()
            .w(gpui::px(200.))
            .flex_none()
            .flex()
            .flex_col()
            .gap(gpui::px(2.))
            .px(gpui::px(12.))
            .pt(gpui::px(18.))
            .pb(gpui::px(14.))
            .border_r_1()
            .border_color(theme::line())
            .bg(theme::bg())
            .child(
                gpui::div()
                    .px(gpui::px(10.))
                    .pb(gpui::px(14.))
                    .font_family(theme::DISPLAY_FONT)
                    .font_weight(FontWeight::BOLD)
                    .text_size(gpui::px(18.))
                    .child("Settings"),
            )
            .children(tabs)
            .child(gpui::div().flex_1())
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(8.))
                    .px(gpui::px(10.))
                    .text_size(gpui::px(12.))
                    .text_color(theme::faint())
                    .child(logo(gpui::px(16.)))
                    .child(concat!("Arcwise ", env!("CARGO_PKG_VERSION"))),
            )
    }

    fn general(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        self.libraries(cx)
            .child(group(
                "When Arcwise opens",
                vec![
                    toggle(
                        "reopen-last",
                        "Open where I left off",
                        "The map, agent and side you had open last".into(),
                    )
                    .child(switch(self.reopen_last))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reopen_last = !this.reopen_last;
                        cx.emit(SetReopenLast(this.reopen_last));
                        cx.notify();
                    }))
                    .into_any_element(),
                ],
            ))
            .into_any_element()
    }

    fn playback(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let about: SharedString = match cx.global::<Shortcuts>().first(Command::SlowMotion) {
            Some(key) => format!(
                "The speed clips play at when Arcwise opens. {} changes it as one plays",
                key.label()
            )
            .into(),
            None => "The speed clips play at when Arcwise opens".into(),
        };
        let buttons = Speed::ALL.map(|speed| {
            let on = speed == self.start_speed;
            choice(SharedString::from(speed.label()), on).on_click(cx.listener(
                move |this, _, _, cx| {
                    if this.start_speed != speed {
                        this.start_speed = speed;
                        this.player
                            .update(cx, |player, cx| player.set_speed(speed, cx));
                        cx.emit(SetStartSpeed(speed));
                        cx.notify();
                    }
                },
            ))
        });
        group(
            "Clips",
            vec![
                setting()
                    .child(described("Starting speed".into(), about))
                    .child(segmented().children(buttons))
                    .into_any_element(),
            ],
        )
        .into_any_element()
    }

    /// The Appearance tab. The title bar's and blur's rows switch them when clicked anywhere, so
    /// the buttons in them stop the click there. Every view draws again, cached ones too, in the
    /// new colors.
    fn appearance(cx: &mut Context<'_, Self>) -> AnyElement {
        let blur = theme::blur();
        let relaunch = (titlebar::chosen() != titlebar::custom()).then(|| {
            text_button("relaunch".into(), "Relaunch").on_click(cx.listener(|_, _, _, cx| {
                cx.stop_propagation();
                cx.emit(Relaunch);
            }))
        });
        let mut window_rows = vec![
            toggle(
                "title-bar",
                "Custom title bar",
                "The header doubles as the title bar. Applies after a relaunch".into(),
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
                "Shows what is behind the window, blurred".into(),
            )
            .children(Self::reset_blur(cx))
            .child(switch(blur))
            .on_click(cx.listener(|_, _, window, cx| {
                let on = !theme::blur();
                theme::set_blur(on);
                window.set_background_appearance(theme::window_background());
                window.refresh();
                cx.emit(SetBlur(on));
            }))
            .into_any_element(),
        ];
        if blur {
            window_rows.push(Self::strength_row(cx).into_any_element());
        }
        gpui::div()
            .child(group(
                "Theme",
                vec![
                    setting()
                        .child(name("Theme"))
                        .child(
                            segmented()
                                .child(mode_button(Some(Mode::Dark), "Dark", cx))
                                .child(mode_button(Some(Mode::Light), "Light", cx))
                                .child(mode_button(None, "System", cx)),
                        )
                        .into_any_element(),
                ],
            ))
            .child(group("Window", window_rows))
            .into_any_element()
    }

    /// The Updates tab: whether updates are automatic, the version running and when Arcwise last
    /// looked for a newer one, and a card for a newer one when there is one.
    fn updates(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let updater = self.updater.read(cx);
        let automatic = updater.automatic();
        let about = if automatic {
            "Downloads new versions in the background and installs them when Arcwise closes"
        } else {
            "Says when a new version is out, and installs it when you ask"
        };
        let installed = arcwise_update::installed();
        let status = update_status(updater, installed);
        let can_check = updater.can_check();
        let newer = newer_version(updater.state()).map(|newer| {
            let Newer {
                title,
                about,
                action,
                downloading,
            } = newer;
            gpui::div()
                .mt(gpui::px(14.))
                .rounded(gpui::px(12.))
                .border_1()
                .border_color(theme::line_strong())
                .bg(theme::raised())
                .child(
                    setting()
                        .child(described(title.into(), about.into()))
                        .child(
                            ui::button("install-update".into(), action)
                                .when(downloading, |b| {
                                    b.child(
                                        gpui::div()
                                            .relative()
                                            .size(gpui::px(14.))
                                            .ml(gpui::px(8.))
                                            .child(ui::spinner(
                                                gpui::px(14.),
                                                theme::muted().into(),
                                            )),
                                    )
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.updater.update(cx, Updater::install_now);
                                })),
                        ),
                )
        });
        gpui::div()
            .child(group(
                "Updates",
                vec![
                    toggle("auto-update", "Automatic updates", about.into())
                        .child(switch(automatic))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let on = !this.updater.read(cx).automatic();
                            this.updater
                                .update(cx, |updater, cx| updater.set_automatic(on, cx));
                            cx.emit(SetAutoUpdate(on));
                        }))
                        .into_any_element(),
                    setting()
                        .gap(gpui::px(12.))
                        .child(logo(gpui::px(20.)))
                        .child(described(
                            concat!("Arcwise ", env!("CARGO_PKG_VERSION")).into(),
                            status,
                        ))
                        .when(installed, |row| {
                            row.child(
                                ui::button("check-now".into(), "Check now")
                                    .h(gpui::px(32.))
                                    .px(gpui::px(12.))
                                    .text_size(gpui::px(13.))
                                    .when(!can_check, |b| b.opacity(0.45))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.updater.update(cx, Updater::check);
                                    })),
                            )
                        })
                        .into_any_element(),
                    setting()
                        .child(described(
                            "What's new".into(),
                            "What each version changed, on GitHub".into(),
                        ))
                        .child(
                            ui::button("whats-new".into(), "Open")
                                .h(gpui::px(32.))
                                .px(gpui::px(12.))
                                .text_size(gpui::px(13.))
                                .on_click(|_, _, cx| cx.open_url(arcwise_update::RELEASES)),
                        )
                        .into_any_element(),
                ],
            ))
            .children(newer)
            .into_any_element()
    }

    /// Puts background blur and its strength back as they first were. Shown only while either
    /// differs. The row around it turns blur on or off when clicked, so it stops the click there.
    fn reset_blur(cx: &mut Context<'_, Self>) -> Option<Stateful<Div>> {
        let defaults = Settings::default();
        if theme::blur() == defaults.blur && theme::strength() == defaults.blur_strength {
            return None;
        }
        let reset = text_button("reset-blur".into(), "Reset").on_click(cx.listener(
            move |_, _, window, cx| {
                cx.stop_propagation();
                theme::set_blur(defaults.blur);
                theme::set_strength(defaults.blur_strength);
                window.set_background_appearance(theme::window_background());
                window.refresh();
                cx.emit(SetBlur(defaults.blur));
                cx.emit(SetBlurStrength(defaults.blur_strength));
            },
        ));
        Some(reset)
    }

    /// How much of the blur shows, from 0 to 100: a track to click or drag along, which changes
    /// the window as it moves and is saved when let go. It takes the pointer only where the track
    /// shows, not where the tab has scrolled it out of sight.
    fn strength_row(cx: &mut Context<'_, Self>) -> Div {
        let strength = theme::strength();
        let along = f32::from(strength) / 100.;
        let browser = cx.entity().downgrade();
        let bar = |d: Div| {
            d.absolute()
                .left_0()
                .top(gpui::px(9.))
                .h(gpui::px(4.))
                .rounded_full()
        };
        setting()
            .child(name("Strength"))
            .child(
                gpui::div()
                    .relative()
                    .w(gpui::px(260.))
                    .h(gpui::px(22.))
                    .cursor_pointer()
                    .child(bar(gpui::div()).right_0().bg(theme::line_hover()))
                    .child(
                        bar(gpui::div())
                            .w(gpui::relative(along))
                            .bg(theme::accent()),
                    )
                    .child(
                        gpui::div()
                            .absolute()
                            .top(gpui::px(4.))
                            .left(gpui::relative(along))
                            .ml(-KNOB / 2.)
                            .size(KNOB)
                            .rounded_full()
                            .bg(theme::text()),
                    )
                    .child(
                        gpui::canvas(
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
                ui::num(format!("{strength}%").into())
                    .w(gpui::px(34.))
                    .flex()
                    .justify_end(),
            )
    }

    /// Ends a drag along the strength track, and saves where it stopped.
    pub(super) fn end_strength_drag(&mut self, cx: &mut Context<'_, Self>) {
        if self.strength_drag {
            self.strength_drag = false;
            cx.emit(SetBlurStrength(theme::strength()));
        }
    }
}

/// What the Updates tab says of the version running.
fn update_status(updater: &Updater, installed: bool) -> SharedString {
    if !installed {
        return "Not run from where it was installed, so it does not update itself".into();
    }
    if updater.checking() {
        return "Looking for a newer version…".into();
    }
    match updater.checked() {
        None => "The version running now".into(),
        Some(checked) if checked.failed => "Could not reach GitHub to look for updates".into(),
        Some(checked) => {
            let when = match checked.at.elapsed().as_secs() / 60 {
                0 => "just now".to_owned(),
                1 => "a minute ago".to_owned(),
                n => format!("{n} minutes ago"),
            };
            match updater.state() {
                State::UpToDate => format!("Up to date. Checked {when}").into(),
                State::Available(_) | State::Downloading { .. } | State::Ready(_) => {
                    format!("Checked {when}").into()
                }
            }
        }
    }
}

/// What the Updates tab's card says of a newer version.
struct Newer {
    title: String,
    about: &'static str,
    /// The button's label.
    action: &'static str,
    /// Whether it is downloading, which the button shows with a spinner.
    downloading: bool,
}

/// The card for a newer version, if there is one.
fn newer_version(state: &State) -> Option<Newer> {
    match state {
        State::UpToDate => None,
        State::Available(version) => Some(Newer {
            title: format!("Arcwise {version} is available"),
            about: "Install it now, and Arcwise opens again on the new version",
            action: "Install and relaunch",
            downloading: false,
        }),
        State::Downloading { version, .. } => Some(Newer {
            title: format!("Arcwise {version} is downloading"),
            about: "Click to install it as soon as it has downloaded",
            action: "Install and relaunch",
            downloading: true,
        }),
        State::Ready(update) => Some(Newer {
            title: format!("Arcwise {} is ready", update.version),
            about: "It installs when you close Arcwise",
            action: "Restart now",
            downloading: false,
        }),
    }
}

/// Follows the pointer along the strength track at `bounds`: a press on the track, where `hitbox`
/// is under the pointer, starts a drag, which follows the pointer anywhere in the window until the
/// button is let go. A release can go missing when another window takes the pointer, as with
/// Alt+Tab mid-drag, so a move with the button up ends the drag too.
fn track_strength(
    bounds: Bounds<Pixels>,
    hitbox: Hitbox,
    browser: WeakEntity<BrowserView>,
    window: &mut Window,
) {
    let along = move |x: Pixels| (x - bounds.left()) / bounds.size.width;
    window.on_mouse_event({
        let browser = browser.clone();
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble
                && event.button == MouseButton::Left
                && hitbox.is_hovered(window)
                && let Some(browser) = browser.upgrade()
            {
                browser.update(cx, |this, _| this.strength_drag = true);
                set_strength(along(event.position.x), window);
            }
        }
    });
    window.on_mouse_event({
        let browser = browser.clone();
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let Some(browser) = browser.upgrade() else {
                return;
            };
            browser.update(cx, |this, cx| {
                if !this.strength_drag {
                    return;
                }
                if event.pressed_button == Some(MouseButton::Left) {
                    set_strength(along(event.position.x), window);
                } else {
                    this.end_strength_drag(cx);
                }
            });
        }
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble
            && event.button == MouseButton::Left
            && let Some(browser) = browser.upgrade()
        {
            browser.update(cx, BrowserView::end_strength_drag);
        }
    });
}

/// Sets the strength from a point `along` the track, 0 at its left end and 1 at its right.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the strength is clamped to 0 through 100 and rounded before the cast"
)]
fn set_strength(along: f32, window: &mut Window) {
    if along.is_nan() {
        return;
    }
    let strength = (along.clamp(0., 1.) * 100.).round() as u8;
    if strength != theme::strength() {
        theme::set_strength(strength);
        window.refresh();
    }
}

/// An SVG filling its parent, to stack with others and tint apart.
fn layer(data: &'static [u8]) -> Svg {
    gpui::svg().data(data).absolute().inset_0().size_full()
}

/// The app's mark, `size` across.
fn logo(size: Pixels) -> Div {
    gpui::div()
        .relative()
        .flex_none()
        .size(size)
        .child(layer(LOGO_ARC).text_color(theme::text()))
        .child(layer(LOGO_DIAMOND).text_color(theme::accent()))
}

/// A titled card of settings, one to a row with lines between them.
pub(super) fn group(title: &str, rows: Vec<AnyElement>) -> Div {
    let rows = rows.into_iter().enumerate().map(|(i, row)| {
        gpui::div()
            .when(i > 0, |d| d.border_t_1().border_color(theme::line()))
            .child(row)
    });
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(8.))
        .mt(gpui::px(14.))
        .child(
            ui::label(title)
                .px(gpui::px(2.))
                .font_weight(FontWeight::BOLD),
        )
        .child(
            gpui::div()
                .rounded(gpui::px(12.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .overflow_hidden()
                .children(rows),
        )
}

/// A row in a [`group`]: what it sets, then its control.
pub(super) fn setting() -> Div {
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(14.))
        .min_h(gpui::px(54.))
        .px(gpui::px(16.))
        .py(gpui::px(10.))
}

/// A row that turns something on or off when clicked anywhere along it.
fn toggle(id: &'static str, label: &'static str, about: SharedString) -> Stateful<Div> {
    setting()
        .id(id)
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(described(label.into(), about))
}

/// A setting's name, taking the room its control leaves.
fn name(text: &'static str) -> Div {
    gpui::div()
        .flex_1()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
}

/// A setting's name with a line under it saying what it does.
pub(super) fn described(title: SharedString, about: SharedString) -> Div {
    gpui::div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(gpui::px(2.))
        .child(gpui::div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(
            gpui::div()
                .text_size(gpui::px(12.5))
                .text_color(theme::muted())
                .child(about),
        )
}

/// A small text button at the end of a row, beside its switch, and as tall, so the row keeps its
/// height when this shows.
pub(super) fn text_button(id: ElementId, label: &'static str) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .h(gpui::px(22.))
        .flex()
        .items_center()
        .px(gpui::px(8.))
        .rounded(gpui::px(6.))
        .text_size(gpui::px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::panel()).text_color(theme::text()))
        .child(label)
}

/// One choice in the theme picker, which switches every color at once: a theme, or with `None`,
/// whichever Windows uses. Every view draws again, cached ones too, in the new colors.
fn mode_button(
    mode: Option<Mode>,
    label: &'static str,
    cx: &mut Context<'_, BrowserView>,
) -> Stateful<Div> {
    let chosen = move || match mode {
        None => theme::follows_system(),
        Some(mode) => !theme::follows_system() && theme::mode() == mode,
    };
    choice(label.into(), chosen()).on_click(cx.listener(move |_, _, window, cx| {
        if chosen() {
            return;
        }
        theme::set_follow_system(mode.is_none());
        match mode {
            Some(mode) => theme::set_mode(mode),
            None => {
                theme::sync_system(window);
            }
        }
        if let Err(e) = titlebar::sync_native(window) {
            remote::report(format_args!("matching the title bar to the theme: {e}"));
        }
        window.refresh();
        cx.emit(SetTheme);
    }))
}

/// A row of [`choice`]s, framed together.
fn segmented() -> Div {
    gpui::div()
        .flex()
        .gap(gpui::px(2.))
        .p(gpui::px(3.))
        .rounded(gpui::px(8.))
        .border_1()
        .border_color(theme::line_hover())
        .bg(theme::panel())
}

/// One of a few choices side by side, of which one is `on`.
fn choice(label: SharedString, on: bool) -> Stateful<Div> {
    gpui::div()
        .id(label.clone())
        .h(gpui::px(26.))
        .px(gpui::px(12.))
        .flex()
        .items_center()
        .rounded(gpui::px(6.))
        .text_size(gpui::px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if on { theme::text() } else { theme::muted() })
        .when(on, |d| d.bg(theme::hover()))
        .when(!on, |d| {
            d.cursor_pointer().hover(|s| s.text_color(theme::text()))
        })
        .child(label)
}

/// An on and off switch, its knob light on the accent in either theme.
fn switch(on: bool) -> Div {
    gpui::div()
        .flex_none()
        .flex()
        .items_center()
        .w(gpui::px(38.))
        .h(gpui::px(22.))
        .px(gpui::px(3.))
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
        .child(gpui::div().size(gpui::px(14.)).rounded_full().bg(if on {
            gpui::rgb(theme::DARK.text)
        } else {
            theme::muted()
        }))
}
