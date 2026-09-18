//! The map column: the ability filter over the map, with a marker for every landing.

use arcwise_core::{Landing, LandingKey};
use gpui::{AnyElement, Context, Div, FontWeight, Hsla, IntoElement, Pixels, div, prelude::*, px};

use super::Browser;
use crate::images::{Image, Look};
use crate::{theme, ui};

/// A landing marker's diameter.
const MARKER: f32 = 30.;

impl Browser {
    pub(super) fn map_column(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(12.))
            .pt(px(16.))
            .px(px(20.))
            .pb(px(20.))
            .bg(theme::bg())
            .child(self.ability_filter(cx))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(ui::measure(self.map_area.clone()))
                    .child(self.map(cx)),
            )
    }

    /// One button per ability with lineups on this side, and one for all of them. Hidden, but
    /// still taking its space, while there is only one ability.
    fn ability_filter(&self, cx: &mut Context<Self>) -> Div {
        let abilities = self.browse.abilities(&self.library);
        let total: usize = abilities.iter().map(|(_, n)| n).sum();
        let row = div()
            .h(px(34.))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(px(6.));
        if abilities.len() < 2 {
            return row;
        }
        let current = self.browse.ability();
        let all = filter_button(
            "ability-all",
            1,
            None,
            "All".into(),
            total,
            current.is_none(),
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.browse.set_ability(&this.library, None);
            cx.notify();
        }));
        let buttons = abilities.into_iter().enumerate().map(|(i, (id, count))| {
            let name = self
                .library
                .ability(id)
                .map_or_else(String::new, |a| a.name.clone());
            let icon = self.image(&self.library.ability_icon(id), cx).ready();
            filter_button(
                gpui::ElementId::Integer(u64::from(id.0)),
                i + 2,
                icon,
                name,
                count,
                current == Some(id),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.browse.set_ability(&this.library, Some(id));
                cx.notify();
            }))
        });
        row.child(all).children(buttons)
    }

    /// The map image, sized to the largest square that fits, with the landing markers on it.
    /// Sized from the area measured on the previous frame; see [`ui::measure`].
    fn map(&self, cx: &mut Context<Self>) -> AnyElement {
        let area = self.map_area.get().size;
        let side = area.width.min(area.height);
        let (Some(map), Some(agent)) = (
            self.browse.map(),
            self.browse.agent().and_then(|id| self.library.agent(id)),
        ) else {
            return div().into_any_element();
        };
        let image = self.image(&self.library.map_image(map, self.browse.side()), cx);
        // Markers wait for the map and the ability icons, so they never float over an empty
        // square or show as empty rings.
        let icons_ready = agent.abilities.iter().all(|ab| {
            !matches!(
                self.image(&self.library.ability_icon(ab.id), cx),
                Image::Loading
            )
        });
        let ready = matches!(image, Image::Ready(_)) && icons_ready;
        let color: Hsla = theme::agent(agent.color).into();
        let selected = self.browse.selected_landing().map(|g| g.key);
        let mut square = div()
            .relative()
            .flex_none()
            .size(side)
            .children(image.ready().map(|map| {
                div()
                    .absolute()
                    .inset_0()
                    .child(gpui::img(gpui::ImageSource::Render(map)).size_full())
            }));
        if ready {
            let markers: Vec<AnyElement> = self
                .browse
                .landings()
                .iter()
                .map(|landing| {
                    self.landing_marker(landing, side, color, selected, cx)
                        .into_any_element()
                })
                .collect();
            square = square
                .children(markers)
                .children(self.landing_card(side, color));
        } else {
            square = square.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .px(px(14.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(theme::alpha(theme::panel(), 0.9))
                            .text_color(theme::muted())
                            .text_size(px(13.))
                            .child("Loading lineups…"),
                    ),
            );
        }
        square.into_any_element()
    }

    fn landing_marker(
        &self,
        landing: &Landing,
        side: Pixels,
        color: Hsla,
        selected: Option<LandingKey>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let on = selected == Some(landing.key);
        let dimmed = selected.is_some() && !on;
        let look = if on { Look::Silhouette } else { Look::Plain };
        let icon = self
            .images
            .update(cx, |images, cx| {
                images.get_as(&self.library.ability_icon(landing.ability), look, cx)
            })
            .ready();
        let key = landing.key;
        let count = landing.lineups.len();
        let slot = px(MARKER * 1.25);
        let group = format!("landing-{}-{}", key.ability.0, key.first.0);
        div()
            .id(gpui::ElementId::Name(group.clone().into()))
            .group(group.clone())
            .absolute()
            .left(side * (landing.at.x / 100.) - slot / 2.)
            .top(side * (landing.at.y / 100.) - slot / 2.)
            .size(slot)
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let hovered = hovered.then_some(key);
                if this.hovered != hovered {
                    this.hovered = hovered.or(this.hovered.filter(|k| *k != key));
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.hovered = None;
                this.browse.select_landing(&this.library, key);
                cx.notify();
            }))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(if on { MARKER * 1.25 } else { MARKER }))
                    .rounded_full()
                    .border_2()
                    .border_color(color)
                    .bg(if on {
                        color
                    } else {
                        theme::alpha(theme::bg(), 0.91)
                    })
                    .when(dimmed, |d| d.opacity(0.35))
                    .group_hover(group, |s| s.size(px(MARKER * 1.2)).opacity(1.))
                    .children(icon.map(|icon| {
                        gpui::img(gpui::ImageSource::Render(icon))
                            .size(px(17.))
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
                    .when(count > 1, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(-7.))
                                .right(px(-9.))
                                .min_w(px(17.))
                                .h(px(17.))
                                .px(px(4.))
                                .rounded(px(9.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(theme::text())
                                .text_color(theme::bg())
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .text_size(px(10.))
                                .child(count.to_string()),
                        )
                    }),
            )
    }

    /// The card beside a hovered landing: what lands there and where it is thrown from.
    fn landing_card(&self, side: Pixels, color: Hsla) -> Option<impl IntoElement> {
        let key = self.hovered?;
        if self.browse.selected_landing().is_some_and(|g| g.key == key) {
            return None;
        }
        let landing = self.browse.landing(key)?;
        let ability = self
            .library
            .ability(landing.ability)
            .map_or("", |a| a.name.as_str());
        let from = landing
            .stands
            .iter()
            .map(|(callout, n)| {
                let place = callout.as_deref().unwrap_or("?");
                if *n > 1 {
                    format!("{place} ×{n}")
                } else {
                    place.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let (x, y) = (side * (landing.at.x / 100.), side * (landing.at.y / 100.));
        let gap = px(22.);
        Some(
            div()
                .absolute()
                .top(y - px(36.))
                .map(|d| {
                    if landing.at.x > 60. {
                        d.right(side - x + gap)
                    } else {
                        d.left(x + gap)
                    }
                })
                .max_w(px(260.))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::alpha(theme::panel(), 0.95))
                .shadow_lg()
                .child(
                    div()
                        .text_size(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(color)
                        .child(format!("{ability} lands at")),
                )
                .child(
                    div()
                        .mt(px(2.))
                        .mb(px(4.))
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(17.))
                        .child(place(landing.name.as_deref())),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(theme::muted())
                        .child(format!("from {from}")),
                ),
        )
    }
}

/// A place name, or a stand-in when the library has none.
pub(super) fn place(callout: Option<&str>) -> String {
    callout.unwrap_or("unknown spot").to_string()
}

fn filter_button(
    id: impl Into<gpui::ElementId>,
    key: usize,
    icon: Option<std::sync::Arc<gpui::RenderImage>>,
    name: String,
    count: usize,
    on: bool,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(7.))
        .h(px(34.))
        .pl(px(8.))
        .pr(px(12.))
        .rounded(px(8.))
        .border_1()
        .border_color(if on {
            gpui::rgb(0x3a4556)
        } else {
            theme::line()
        })
        .when(on, |d| d.bg(theme::raised()))
        .text_color(if on { theme::text() } else { theme::muted() })
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|s| {
            s.text_color(theme::text())
                .border_color(theme::line_hover())
        })
        .child(ui::kbd(key.to_string(), on))
        .children(icon.map(|icon| {
            gpui::img(gpui::ImageSource::Render(icon))
                .size(px(18.))
                .object_fit(gpui::ObjectFit::Contain)
                .opacity(if on { 1. } else { 0.65 })
        }))
        .child(name)
        .child(ui::num(count.to_string()))
}
