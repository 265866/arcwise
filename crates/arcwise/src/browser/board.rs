//! The map column: the ability filter over the map, with a marker for every landing.

use arcwise_core::{Landing, LandingKey, Lineup, Point};
use gpui::{
    AnyElement, Context, Div, FontWeight, Hsla, IntoElement, Pixels, Stateful, canvas, div,
    prelude::*, px,
};

use super::Browser;
use crate::drawn::Line;
use crate::images::{Image, Look};
use crate::{theme, ui};

/// A landing marker's diameter.
const MARKER: f32 = 30.;

impl Browser {
    /// A click on the board that reaches no control closes the lineup, back to the map.
    pub(super) fn map_column(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("board")
            .on_click(cx.listener(|this, _, _, cx| {
                if this.browse.selected_lineup().is_some() {
                    this.browse.clear_selection();
                    this.peek_view = None;
                    this.peek_lineup = None;
                    cx.notify();
                }
            }))
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
            cx.stop_propagation();
            this.browse.set_ability(&this.library, None);
            cx.notify();
        }));
        let buttons = abilities.into_iter().enumerate().map(|(i, (id, count))| {
            let name = self
                .library
                .ability(id)
                .map_or_else(String::new, |a| a.name.clone());
            let icon = self.chip_icon(id, cx).ready();
            filter_button(
                gpui::ElementId::Integer(u64::from(id.0)),
                i + 2,
                icon,
                name,
                count,
                current == Some(id),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
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
        let Some(map) = self.browse.map() else {
            return div().into_any_element();
        };
        let image = self.map_picture(map, self.browse.side(), cx);
        let missing = matches!(image, Image::Missing);
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
        if missing {
            square = square.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme::line())
                    .text_color(theme::faint())
                    .child("This side's map picture is missing from the library."),
            );
        }
        // While an agent is being chosen, the map shows on its own.
        let Some(agent) = self.browse.agent().and_then(|id| self.library.agent(id)) else {
            return square.into_any_element();
        };
        // Markers wait for the map and the ability icons, so they never float over an empty
        // square or show as empty rings.
        let icons_ready = agent.abilities.iter().all(|ab| {
            !matches!(
                self.image(&self.library.ability_icon(ab.id), cx),
                Image::Loading
            )
        });
        // A missing map picture still shows the landings, over a note saying it is missing.
        let ready = !matches!(image, Image::Loading) && icons_ready;
        let color: Hsla = theme::agent(agent.color).into();
        let selected = self.browse.selected_landing().map(|g| g.key);
        if ready {
            // The selected landing last, so it draws over its neighbours.
            let landings: Vec<&Landing> = self
                .browse
                .landings()
                .iter()
                .filter(|g| Some(g.key) != selected)
                .chain(self.browse.selected_landing())
                .collect();
            let markers: Vec<AnyElement> = landings
                .iter()
                .map(|landing| {
                    self.landing_marker(landing, side, color, selected, cx)
                        .into_any_element()
                })
                .collect();
            let counts: Vec<AnyElement> = landings
                .iter()
                .filter_map(|landing| self.landing_count(landing, side, selected))
                .collect();
            let stands: Vec<AnyElement> = self
                .browse
                .selected_landing()
                .into_iter()
                .flat_map(|landing| landing.lineups.iter().enumerate())
                .filter_map(|(i, &id)| {
                    let lineup = self.library.lineup(id)?;
                    Some(
                        self.stand_marker(i + 1, lineup, side, color, cx)
                            .into_any_element(),
                    )
                })
                .collect();
            square = square
                .child(self.throw_paths(color))
                .children(markers)
                .children(counts)
                .children(stands)
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

    /// How many lineups a landing marker holds, when more than one. Drawn after every marker,
    /// so a neighbouring marker never covers it.
    fn landing_count(
        &self,
        landing: &Landing,
        side: Pixels,
        selected: Option<LandingKey>,
    ) -> Option<AnyElement> {
        let count = landing.lineups.len();
        if count < 2 {
            return None;
        }
        let on = selected == Some(landing.key);
        let hovered = self.hovered == Some(landing.key);
        // The ring's size, as the marker draws it.
        let ring = px(if on {
            MARKER * 1.25
        } else if hovered {
            MARKER * 1.2
        } else {
            MARKER
        });
        let (x, y) = (side * (landing.at.x / 100.), side * (landing.at.y / 100.));
        // Over the ring's top right, its right edge 9 pixels past the ring's.
        let room = px(40.);
        Some(
            div()
                .absolute()
                .left(x + ring / 2. + px(9.) - room)
                .top(y - ring / 2. - px(7.))
                .w(room)
                .flex()
                .justify_end()
                .when(selected.is_some() && !on && !hovered, |d| d.opacity(0.35))
                .child(
                    div()
                        .min_w(px(17.))
                        .h(px(17.))
                        .px(px(4.))
                        .rounded(px(9.))
                        .flex()
                        .items_center()
                        .justify_center()
                        // Over the map picture, which is dark in either theme.
                        .bg(gpui::rgb(theme::DARK.text))
                        .text_color(gpui::rgb(theme::DARK.bg))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(px(10.))
                        .child(count.to_string()),
                )
                .into_any_element(),
        )
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
                images.get_as(&self.library.ability_icon(landing.key.ability), look, cx)
            })
            .ready();
        let key = landing.key;
        let first = landing.lineups[0];
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
                // Pointing at a landing starts loading what its panel shows.
                if *hovered {
                    this.fetch_lineup(first, cx);
                }
                let hovered = hovered.then_some(key);
                if this.hovered != hovered {
                    this.hovered = hovered.or(this.hovered.filter(|k| *k != key));
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.hovered = None;
                this.browse.select_landing(key);
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
                        theme::alpha(gpui::rgb(theme::DARK.bg), 0.91)
                    })
                    .when(dimmed, |d| d.opacity(0.35))
                    .group_hover(group, |s| s.size(px(MARKER * 1.2)).opacity(1.))
                    .children(icon.map(|icon| {
                        gpui::img(gpui::ImageSource::Render(icon))
                            .size(px(17.))
                            .object_fit(gpui::ObjectFit::Contain)
                    })),
            )
    }

    /// The selected landing's throws, from each stand spot to where it lands. The focused one
    /// is solid; the others are dashed.
    fn throw_paths(&self, color: Hsla) -> impl IntoElement {
        let focus = self.peek_lineup.or(self.browse.selected_lineup());
        let throws: Vec<(bool, Vec<Point>)> = self
            .browse
            .selected_landing()
            .into_iter()
            .flat_map(|landing| landing.lineups.iter())
            .filter_map(|&id| {
                let lineup = self.library.lineup(id)?;
                let points = std::iter::once(lineup.stand.at())
                    .chain(lineup.path.iter().copied())
                    .collect();
                Some((Some(id) == focus, points))
            })
            .collect();
        let drawn = self.throws.clone();
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let to_px = |p: &Point| {
                    gpui::point(
                        bounds.size.width * (p.x / 100.),
                        bounds.size.height * (p.y / 100.),
                    )
                };
                // The focused throw last, so it draws over the others.
                let mut lines = Vec::new();
                for (focused, points) in throws
                    .iter()
                    .filter(|t| !t.0)
                    .chain(throws.iter().filter(|t| t.0))
                {
                    let points: Vec<_> = points.iter().map(to_px).collect();
                    let line = |width: f32, color: Hsla, dash| Line {
                        points: points.clone(),
                        width: px(width),
                        color,
                        dash,
                    };
                    if *focused {
                        lines.push(line(8., theme::alpha(color, 0.22), None));
                        lines.push(line(2.5, color, None));
                    } else {
                        lines.push(line(1.5, theme::alpha(color, 0.45), Some((px(4.), px(5.)))));
                    }
                }
                drawn.paint(bounds, lines, window);
            },
        )
        .absolute()
        .inset_0()
    }

    /// A numbered marker where the player stands for one lineup of the selected landing.
    fn stand_marker(
        &self,
        number: usize,
        lineup: &Lineup,
        side: Pixels,
        color: Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let id = lineup.id;
        let on = self.peek_lineup.or(self.browse.selected_lineup()) == Some(id);
        let size = px(if on { 26. } else { 22. });
        div()
            .id(gpui::ElementId::Name(format!("stand-{id}").into()))
            .absolute()
            .left(side * (lineup.stand.x / 100.) - size / 2.)
            .top(side * (lineup.stand.y / 100.) - size / 2.)
            .size(size)
            .rounded(px(6.))
            .flex()
            .items_center()
            .justify_center()
            .bg(if on {
                color
            } else {
                gpui::rgb(theme::DARK.text).into()
            })
            .text_color(if on {
                gpui::black()
            } else {
                gpui::rgb(theme::DARK.bg).into()
            })
            .font_weight(FontWeight::EXTRA_BOLD)
            .text_size(px(12.))
            .shadow_md()
            .cursor_pointer()
            .child(number.to_string())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.browse.select_lineup(id);
                cx.notify();
            }))
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
            .ability(landing.key.ability)
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
                        .text_color(theme::readable(color))
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
            theme::line_strong()
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
