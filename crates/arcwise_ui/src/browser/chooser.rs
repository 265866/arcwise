//! The map and agent choosers: a centered dialog for the first choice, then dropdowns under the
//! header's pickers.

use arcwise_core::{Agent, Chooser, Role};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Stateful, StatefulInteractiveElement, Styled, Window,
};

use super::browser_view::{self, BrowserView};
use crate::images::Look;
use crate::theme::Mode;
use crate::ui::Shade;
use crate::{theme, ui};

/// Where a chooser shows: as the first-run dialog, or as a dropdown under its picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Dialog,
    Dropdown,
}

impl BrowserView {
    /// The chooser's card: the first-run dialog, or a dropdown's contents.
    fn chooser_card(
        &self,
        which: Chooser,
        form: Form,
        window: &Window,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement + use<> {
        let content = match which {
            Chooser::Map => self.map_choices(form, cx),
            Chooser::Agent => self.agent_choices(form, cx),
        };
        gpui::div()
            .id("chooser-card")
            .flex()
            .flex_col()
            .gap(gpui::px(18.))
            .max_h(window.viewport_size().height - ui::BAR_HEIGHT - gpui::px(30.))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .occlude()
            .map(|d| match form {
                Form::Dropdown => d.p(gpui::px(14.)).rounded(gpui::px(12.)),
                Form::Dialog => d
                    .px(gpui::px(26.))
                    .pt(gpui::px(24.))
                    .pb(gpui::px(26.))
                    .rounded(gpui::px(16.)),
            })
            .child(content)
    }

    /// The dropdown under a header picker, while its chooser is open as one. Kept inside the
    /// window, and drawn over everything else.
    pub(super) fn dropdown(
        &self,
        which: Chooser,
        window: &Window,
        cx: &mut Context<'_, Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.browse.dropdown() || self.browse.chooser() != Some(which) {
            return None;
        }
        Some(
            gpui::div()
                .absolute()
                .left_0()
                .top(gpui::relative(1.))
                .child(gpui::deferred(
                    gpui::anchored()
                        .offset(gpui::point(gpui::px(0.), gpui::px(8.)))
                        .snap_to_window_with_margin(gpui::px(8.))
                        .child(self.chooser_card(which, Form::Dropdown, window, cx)),
                )),
        )
    }

    /// What sits behind an open chooser: the first-run dialog's backdrop and card, or the layer
    /// that closes a dropdown when clicked. That layer covers the board but not the header, so
    /// the other picker still switches choosers in one click. The dialog, too, sits below the
    /// header, which stays usable as the window's title bar.
    pub(super) fn chooser(
        &self,
        window: &Window,
        cx: &mut Context<'_, Self>,
    ) -> Option<AnyElement> {
        let which = self.browse.chooser()?;
        let dismiss = cx.listener(|this, _, _, cx| {
            this.browse.dismiss_chooser(&this.library);
            cx.notify();
        });
        Some(if self.browse.dropdown() {
            gpui::div()
                .absolute()
                .top(ui::BAR_HEIGHT)
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss)
                .into_any_element()
        } else {
            let card = self.chooser_card(which, Form::Dialog, window, cx);
            gpui::div()
                .id("chooser")
                .absolute()
                .inset_0()
                .top(ui::BAR_HEIGHT)
                .flex()
                .items_center()
                .justify_center()
                .p(gpui::px(24.))
                .bg(theme::scrim())
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss)
                .child(card)
                .into_any_element()
        })
    }

    /// A card per map. Pointing at a map starts drawing it, which is usually done by the click.
    fn map_choices(&self, form: Form, cx: &mut Context<'_, Self>) -> Div {
        let dropdown = form == Form::Dropdown;
        let width = gpui::px(if dropdown { 200. } else { 230. });
        let height = gpui::px(if dropdown { 76. } else { 92. });
        let cards = self.library.maps_by_name().into_iter().map(|map| {
            let id = map.id.clone();
            let on = self.browse.map() == Some(&map.id);
            let picture = self.image(&self.library.map_card(&map.id), cx).ready();
            let group = format!("map-{}", map.id);
            gpui::div()
                .id(ElementId::Name(group.clone().into()))
                .group(group.clone())
                .relative()
                .w(width)
                .h(height)
                .rounded(gpui::px(10.))
                .overflow_hidden()
                .border_1()
                .border_color(if on { theme::accent() } else { theme::line() })
                .bg(theme::raised())
                .cursor_pointer()
                .children(picture.map(|p| {
                    gpui::div()
                        .absolute()
                        .inset_0()
                        .opacity(0.55)
                        .group_hover(group.clone(), |s| s.opacity(0.95))
                        .child(ui::cover(p))
                }))
                .child(
                    gpui::div()
                        .absolute()
                        .left(gpui::px(14.))
                        .top(gpui::px(if dropdown { 12. } else { 14. }))
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(gpui::px(if dropdown { 15. } else { 17. }))
                        .child(map.name.to_uppercase()),
                )
                .child(
                    gpui::div()
                        .absolute()
                        .left(gpui::px(14.))
                        .bottom(gpui::px(if dropdown { 10. } else { 12. }))
                        .text_size(gpui::px(12.))
                        .child(browser_view::plural(
                            self.library.map_total(&map.id),
                            "lineup",
                        )),
                )
                .on_hover(cx.listener({
                    let path = self.library.map_image(&map.id, self.browse.side());
                    move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            let look = Look::Across(this.across);
                            this.images
                                .update(cx, |images, cx| images.fetch(&path, look, cx));
                        }
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.browse.choose_map(&this.library, id.clone());
                    cx.notify();
                }))
        });
        gpui::div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.))
            .when(!dropdown, |d| d.child(heading(None, "Choose a map")))
            .child(
                gpui::div()
                    .w((width + gpui::px(if dropdown { 8. } else { 10. })) * 3.)
                    .flex()
                    .flex_wrap()
                    .gap(gpui::px(if dropdown { 8. } else { 10. }))
                    .children(cards),
            )
    }

    /// The agents by role. The markers wait for an agent's ability icons, so pointing at the
    /// agent starts loading them, and on the light theme the black ones its chips draw too.
    fn agent_choices(&self, form: Form, cx: &mut Context<'_, Self>) -> Div {
        let dropdown = form == Form::Dropdown;
        let map = self.browse.map().and_then(|id| self.library.map(id));
        let back = map.filter(|_| !dropdown).map(|map| {
            gpui::div()
                .id("chooser-back")
                .flex()
                .items_center()
                .gap(gpui::px(8.))
                .mb(gpui::px(8.))
                .px(gpui::px(10.))
                .py(gpui::px(4.))
                .rounded(gpui::px(7.))
                .border_1()
                .border_color(theme::line())
                .text_color(theme::muted())
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|s| {
                    s.text_color(theme::text())
                        .border_color(theme::line_hover())
                })
                .child("‹")
                .child(map.name.clone())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.browse.back_to_maps();
                    cx.notify();
                }))
                .into_any_element()
        });
        let roles = Role::ALL.map(|role| {
            let agents = self.library.roster(role).into_iter().map(|agent| {
                let count = map.map_or(0, |m| self.library.count(&m.id, &agent.id));
                self.agent_choice(agent, count, cx)
            });
            gpui::div()
                .flex()
                .flex_col()
                .child(
                    ui::label(role.plural())
                        .mb(gpui::px(8.))
                        .font_weight(FontWeight::BOLD),
                )
                .child(
                    gpui::div()
                        .w(gpui::px(312.))
                        .flex()
                        .flex_wrap()
                        .gap(gpui::px(8.))
                        .children(agents),
                )
        });
        let [duelists, initiators, controllers, sentinels] = roles;
        gpui::div()
            .flex()
            .flex_col()
            .gap(gpui::px(18.))
            .when(!dropdown, |d| d.child(heading(back, "Choose an agent")))
            .child(
                gpui::div()
                    .flex()
                    .gap(gpui::px(28.))
                    .child(
                        gpui::div()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(16.))
                            .child(duelists)
                            .child(controllers),
                    )
                    .child(
                        gpui::div()
                            .flex()
                            .flex_col()
                            .gap(gpui::px(16.))
                            .child(initiators)
                            .child(sentinels),
                    ),
            )
    }

    /// An agent to choose, with how many lineups they have on the map, `count`.
    fn agent_choice(
        &self,
        agent: &Agent,
        count: usize,
        cx: &mut Context<'_, Self>,
    ) -> Stateful<Div> {
        let on = self.browse.agent() == Some(&agent.id);
        let color = theme::agent(agent.color);
        let portrait = self.image(&self.library.portrait(&agent.id), cx).ready();
        let id = agent.id.clone();
        let shade = if count == 0 {
            Shade::Gray
        } else {
            Shade::Color
        };
        gpui::div()
            .id(ElementId::Name(format!("agent-{}", agent.id).into()))
            .w(gpui::px(72.))
            .flex()
            .flex_col()
            .items_center()
            .gap(gpui::px(4.))
            .py(gpui::px(6.))
            .rounded(gpui::px(9.))
            .text_size(gpui::px(12.))
            .text_color(if on { theme::text() } else { theme::muted() })
            .when(count == 0, |d| d.opacity(0.22))
            .when(count > 0, |d| {
                let icons: Vec<_> = agent
                    .abilities
                    .iter()
                    .map(|ab| self.library.ability_icon(ab.id))
                    .collect();
                d.cursor_pointer()
                    .hover(|s| s.bg(theme::raised()).text_color(theme::text()))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.images.update(cx, |images, cx| {
                                for icon in &icons {
                                    images.fetch(icon, Look::Plain, cx);
                                    if theme::mode() == Mode::Light {
                                        images.fetch(icon, Look::Silhouette, cx);
                                    }
                                }
                            });
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.browse.choose_agent(&this.library, id.clone());
                        cx.notify();
                    }))
            })
            .child(
                ui::face(agent, portrait, shade, gpui::px(52.), gpui::px(10.))
                    .border_2()
                    .border_color(if on {
                        color.into()
                    } else {
                        gpui::transparent_black()
                    }),
            )
            .child(agent.name.clone())
            .child(
                ui::num(if count > 0 {
                    count.to_string().into()
                } else {
                    "".into()
                })
                .min_h(gpui::px(11.)),
            )
    }
}

/// The dialog's title, with a way back to the maps above it on the agent step.
fn heading(back: Option<AnyElement>, title: &'static str) -> Div {
    gpui::div()
        .flex()
        .flex_col()
        .items_start()
        .gap(gpui::px(4.))
        .children(back)
        .child(
            gpui::div()
                .font_family(theme::DISPLAY_FONT)
                .font_weight(FontWeight::BOLD)
                .text_size(gpui::px(26.))
                .child(title),
        )
}
