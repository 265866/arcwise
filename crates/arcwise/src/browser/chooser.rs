//! The map and agent choosers: a centered dialog for the first choice, then dropdowns under the
//! header's pickers.

use arcwise_core::{Chooser, Role};
use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, MouseButton, Window, anchored, deferred,
    div, point, prelude::*, px,
};

use super::{Browser, plural};
use crate::{theme, ui};

impl Browser {
    /// The chooser's card: the first-run dialog, or a dropdown's contents.
    fn chooser_card(
        &self,
        which: Chooser,
        dropdown: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let content = match which {
            Chooser::Map => self.map_choices(dropdown, cx),
            Chooser::Agent => self.agent_choices(dropdown, cx),
        };
        div()
            .id("chooser-card")
            .flex()
            .flex_col()
            .gap(px(18.))
            .max_h(window.viewport_size().height - ui::BAR_HEIGHT - px(30.))
            .overflow_y_scroll()
            .border_1()
            .border_color(theme::line())
            .bg(theme::panel())
            .shadow_lg()
            .occlude()
            .when(dropdown, |d| d.p(px(14.)).rounded(px(12.)))
            .when(!dropdown, |d| {
                d.px(px(26.)).pt(px(24.)).pb(px(26.)).rounded(px(16.))
            })
            .child(content)
    }

    /// The dropdown under a header picker, while its chooser is open as one. Kept inside the
    /// window, and drawn over everything else.
    pub(super) fn dropdown(
        &self,
        which: Chooser,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement + use<>> {
        if !self.browse.dropdown() || self.browse.chooser() != Some(which) {
            return None;
        }
        Some(
            div()
                .absolute()
                .left_0()
                .top(gpui::relative(1.))
                .child(deferred(
                    anchored()
                        .offset(point(px(0.), px(8.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(self.chooser_card(which, true, window, cx)),
                )),
        )
    }

    /// What sits behind an open chooser: the first-run dialog's backdrop and card, or the layer
    /// that closes a dropdown when clicked. That layer covers the board but not the header, so
    /// the other picker still switches choosers in one click.
    pub(super) fn chooser(&self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let which = self.browse.chooser()?;
        let dismiss = cx.listener(|this, _, _, cx| {
            this.browse.dismiss_chooser(&this.library);
            cx.notify();
        });
        Some(if self.browse.dropdown() {
            div()
                .absolute()
                .top(ui::BAR_HEIGHT)
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss)
                .into_any_element()
        } else {
            let card = self.chooser_card(which, false, window, cx);
            div()
                .id("chooser")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .p(px(24.))
                .bg(theme::alpha(gpui::rgb(0x05070a), 0.85))
                .occlude()
                .on_mouse_down(MouseButton::Left, dismiss)
                .child(card)
                .into_any_element()
        })
    }

    fn map_choices(&self, dropdown: bool, cx: &mut Context<Self>) -> Div {
        let (width, height) = if dropdown {
            (px(200.), px(76.))
        } else {
            (px(230.), px(92.))
        };
        let cards = self.library.maps_by_name().into_iter().map(|map| {
            let id = map.id.clone();
            let on = self.browse.map() == Some(&map.id);
            let picture = self.image(&self.library.map_card(&map.id), cx).ready();
            let group = format!("map-{}", map.id);
            div()
                .id(gpui::ElementId::Name(group.clone().into()))
                .group(group.clone())
                .relative()
                .w(width)
                .h(height)
                .rounded(px(10.))
                .overflow_hidden()
                .border_1()
                .border_color(if on { theme::accent() } else { theme::line() })
                .bg(theme::raised())
                .cursor_pointer()
                .children(picture.map(|p| {
                    div()
                        .absolute()
                        .inset_0()
                        .opacity(0.55)
                        .group_hover(group.clone(), |s| s.opacity(0.95))
                        .child(ui::cover(p))
                }))
                .child(
                    div()
                        .absolute()
                        .left(px(14.))
                        .top(px(if dropdown { 12. } else { 14. }))
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(px(if dropdown { 15. } else { 17. }))
                        .child(map.name.to_uppercase()),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(14.))
                        .bottom(px(if dropdown { 10. } else { 12. }))
                        .text_size(px(12.))
                        .child(plural(self.library.map_total(&map.id), "lineup")),
                )
                // Pointing at a map starts drawing it, which is usually done by the click.
                .on_hover(cx.listener({
                    let path = self.library.map_image(&map.id, self.browse.side());
                    move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.images.update(cx, |images, cx| images.fetch(&path, cx));
                        }
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.browse.choose_map(&this.library, id.clone());
                    cx.notify();
                }))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .when(!dropdown, |d| {
                d.child(heading(None, "1 of 2", "Choose a map"))
            })
            .child(
                div()
                    .w((width + px(if dropdown { 8. } else { 10. })) * 3.)
                    .flex()
                    .flex_wrap()
                    .gap(px(if dropdown { 8. } else { 10. }))
                    .children(cards),
            )
    }

    fn agent_choices(&self, dropdown: bool, cx: &mut Context<Self>) -> Div {
        let map = self.browse.map().and_then(|id| self.library.map(id));
        let back = map.filter(|_| !dropdown).map(|map| {
            div()
                .id("chooser-back")
                .flex()
                .items_center()
                .gap(px(8.))
                .mb(px(8.))
                .px(px(10.))
                .py(px(4.))
                .rounded(px(7.))
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
                let on = self.browse.agent() == Some(&agent.id);
                let color = theme::agent(agent.color);
                let portrait = self.image(&self.library.portrait(&agent.id), cx).ready();
                let id = agent.id.clone();
                div()
                    .id(gpui::ElementId::Name(format!("agent-{}", agent.id).into()))
                    .w(px(72.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .py(px(6.))
                    .rounded(px(9.))
                    .text_size(px(12.))
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
                            // The markers wait for these icons; pointing at the agent starts them.
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.images.update(cx, |images, cx| {
                                        for icon in &icons {
                                            images.fetch(icon, cx);
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
                        ui::face(agent, portrait, count == 0, px(52.), px(10.))
                            .border_2()
                            .border_color(if on {
                                color
                            } else {
                                gpui::transparent_black().into()
                            }),
                    )
                    .child(agent.name.clone())
                    .child(
                        ui::num(if count > 0 {
                            count.to_string()
                        } else {
                            String::new()
                        })
                        .min_h(px(11.)),
                    )
            });
            div()
                .flex()
                .flex_col()
                .child(
                    ui::label(role.plural())
                        .mb(px(8.))
                        .font_weight(FontWeight::BOLD),
                )
                .child(
                    div()
                        .w(px(312.))
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .children(agents),
                )
        });
        let [duelists, initiators, controllers, sentinels] = roles;
        div()
            .flex()
            .flex_col()
            .gap(px(18.))
            .when(!dropdown, |d| {
                d.child(heading(back, "2 of 2", "Choose an agent"))
            })
            .child(
                div()
                    .flex()
                    .gap(px(28.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(duelists)
                            .child(controllers),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(16.))
                            .child(initiators)
                            .child(sentinels),
                    ),
            )
    }
}

fn heading(back: Option<AnyElement>, step: &'static str, title: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(4.))
        .children(back)
        .child(ui::label(step).font_weight(FontWeight::BOLD))
        .child(
            div()
                .font_family(theme::DISPLAY_FONT)
                .font_weight(FontWeight::BOLD)
                .text_size(px(26.))
                .child(title),
        )
}
