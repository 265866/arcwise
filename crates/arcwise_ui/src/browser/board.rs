//! The map column: the ability filter over the map, with a marker for every landing.

use std::iter;
use std::sync::Arc;

use arcwise_core::{Landing, LandingKey, Lineup, Point};
use arcwise_remote::remote;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, Hsla, ImageSource, InteractiveElement,
    IntoElement, ObjectFit, ParentElement, Pixels, RenderImage, Stateful,
    StatefulInteractiveElement, Styled, StyledImage,
};

use super::browser_view::BrowserView;
use crate::drawn::{Dash, Line};
use crate::images::{Image, Look};
use crate::ui::Cap;
use crate::{theme, ui};

/// A landing marker's diameter.
const MARKER: f32 = 30.;

impl BrowserView {
    /// A click on the board that reaches no control closes the lineup, back to the map.
    pub(super) fn map_column(&self, cx: &mut Context<'_, Self>) -> Stateful<Div> {
        gpui::div()
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
            .gap(gpui::px(12.))
            .pt(gpui::px(16.))
            .px(gpui::px(20.))
            .pb(gpui::px(20.))
            .child(self.ability_filter(cx))
            .child(
                gpui::div()
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
    /// still taking its space, while there is only one ability. In a narrow column the buttons
    /// wrap onto more lines, and the map below takes what is left.
    fn ability_filter(&self, cx: &mut Context<'_, Self>) -> Div {
        let abilities = self.browse.abilities(&self.library);
        let total: usize = abilities.iter().map(|tally| tally.count).sum();
        let row = gpui::div()
            .min_h(gpui::px(34.))
            .flex()
            .flex_wrap()
            .justify_center()
            .gap(gpui::px(6.));
        if abilities.len() < 2 {
            return row;
        }
        let current = self.browse.ability();
        let all = filter_button(
            ElementId::Name("ability-all".into()),
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
        let buttons = abilities.into_iter().enumerate().map(|(i, tally)| {
            let id = tally.item;
            let count = tally.count;
            let name = self
                .library
                .ability(id)
                .map_or_else(String::new, |a| a.name.clone());
            let icon = self.chip_icon(id, cx).ready();
            filter_button(
                ElementId::Integer(u64::from(id.0)),
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
    /// Sized from the area measured on the previous frame; see [`ui::measure`]. While an agent is being
    /// chosen, the map shows on its own. Markers wait for the map and the ability icons, so they
    /// never float over an empty square or show as empty rings, but a missing map picture still
    /// shows the landings, over a note saying it is missing. The selected landing comes last, so
    /// it draws over its neighbours.
    fn map(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let area = self.map_area.get().size;
        let side = area.width.min(area.height);
        let Some(map) = self.browse.map() else {
            return gpui::div().into_any_element();
        };
        let image = self.map_picture(map, self.browse.side(), cx);
        let missing = matches!(image, Image::Missing);
        let mut square = gpui::div()
            .relative()
            .flex_none()
            .size(side)
            .children(image.ready().map(|map| {
                gpui::div()
                    .absolute()
                    .inset_0()
                    .child(gpui::img(ImageSource::Render(map)).size_full())
            }));
        if missing {
            square = square.child(missing_map());
        }
        let Some(agent) = self.browse.agent().and_then(|id| self.library.agent(id)) else {
            return square.into_any_element();
        };
        let icons_ready = agent.abilities.iter().all(|ab| {
            !matches!(
                self.image(&self.library.ability_icon(ab.id), cx),
                Image::Loading
            )
        });
        let ready = !matches!(image, Image::Loading) && icons_ready;
        let color: Hsla = theme::agent(agent.color).into();
        let selected = self.browse.selected_landing().map(|g| g.key);
        if ready {
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
            square = square.child(loading_lineups());
        }
        square.into_any_element()
    }

    /// How many lineups a landing marker holds, when more than one. Drawn after every marker,
    /// so a neighbouring marker never covers it. It sits over the top right of the ring, at the
    /// size the marker draws the ring, its right edge 9 pixels past the ring's, and keeps the
    /// map picture's colors, which are dark in either theme.
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
        let ring = gpui::px(if on {
            MARKER * 1.25
        } else if hovered {
            MARKER * 1.2
        } else {
            MARKER
        });
        let (x, y) = (side * (landing.at.x / 100.), side * (landing.at.y / 100.));
        let room = gpui::px(40.);
        Some(
            gpui::div()
                .absolute()
                .left(x + ring / 2. + gpui::px(9.) - room)
                .top(y - ring / 2. - gpui::px(7.))
                .w(room)
                .flex()
                .justify_end()
                .when(selected.is_some() && !on && !hovered, |d| d.opacity(0.35))
                .child(
                    gpui::div()
                        .min_w(gpui::px(17.))
                        .h(gpui::px(17.))
                        .px(gpui::px(4.))
                        .rounded(gpui::px(9.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui::rgb(theme::DARK.text))
                        .text_color(gpui::rgb(theme::DARK.bg))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(gpui::px(10.))
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
        cx: &mut Context<'_, Self>,
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
        let slot = gpui::px(MARKER * 1.25);
        let group = format!("landing-{}-{}", key.ability.0, key.first.0);
        gpui::div()
            .id(ElementId::Name(group.clone().into()))
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
                gpui::div()
                    .relative()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(gpui::px(if on { MARKER * 1.25 } else { MARKER }))
                    .rounded_full()
                    .border_2()
                    .border_color(color)
                    .bg(if on {
                        color
                    } else {
                        theme::alpha(gpui::rgb(theme::DARK.bg).into(), 0.91)
                    })
                    .when(dimmed, |d| d.opacity(0.35))
                    .group_hover(group, |s| s.size(gpui::px(MARKER * 1.2)).opacity(1.))
                    .children(icon.map(|icon| {
                        gpui::img(ImageSource::Render(icon))
                            .size(gpui::px(17.))
                            .object_fit(ObjectFit::Contain)
                    })),
            )
    }

    /// The selected landing's throws, from each stand spot to where it lands. The focused one
    /// is solid; the others are dashed. The focused throw comes last, so it draws over the others.
    fn throw_paths(&self, color: Hsla) -> impl IntoElement {
        let focus = self.peek_lineup.or(self.browse.selected_lineup());
        let mut throws: Vec<Throw> = self
            .browse
            .selected_landing()
            .into_iter()
            .flat_map(|landing| landing.lineups.iter())
            .filter_map(|&id| {
                let lineup = self.library.lineup(id)?;
                let points: Vec<Point> = iter::once(lineup.stand.at())
                    .chain(lineup.path.iter().copied())
                    .collect();
                Some(Throw {
                    focused: Some(id) == focus,
                    points,
                })
            })
            .collect();
        throws.sort_by_key(|throw| throw.focused);
        let drawn = self.throws.clone();
        gpui::canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let to_px = |p: &Point| {
                    gpui::point(
                        bounds.size.width * (p.x / 100.),
                        bounds.size.height * (p.y / 100.),
                    )
                };
                let mut lines = Vec::new();
                for throw in &throws {
                    let points: Vec<_> = throw.points.iter().map(to_px).collect();
                    let line = |width: f32, color: Hsla, dash| Line {
                        points: points.clone(),
                        width: gpui::px(width),
                        color,
                        dash,
                    };
                    if throw.focused {
                        lines.push(line(8., theme::alpha(color, 0.22), None));
                        lines.push(line(2.5, color, None));
                    } else {
                        let dash = Dash {
                            dash: gpui::px(4.),
                            gap: gpui::px(5.),
                        };
                        lines.push(line(1.5, theme::alpha(color, 0.45), Some(dash)));
                    }
                }
                if let Err(e) = drawn.paint(bounds, lines, window) {
                    remote::report(format_args!("drawing the throws on the map: {e:#}"));
                }
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
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement {
        let id = lineup.id;
        let on = self.peek_lineup.or(self.browse.selected_lineup()) == Some(id);
        let marker = gpui::px(if on { 26. } else { 22. });
        gpui::div()
            .id(ElementId::Name(format!("stand-{id}").into()))
            .absolute()
            .left(side * (lineup.stand.x / 100.) - marker / 2.)
            .top(side * (lineup.stand.y / 100.) - marker / 2.)
            .size(marker)
            .rounded(gpui::px(6.))
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
            .text_size(gpui::px(12.))
            .shadow_md()
            .cursor_pointer()
            .child(number.to_string())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.browse.select_lineup(id);
                cx.notify();
            }))
    }

    /// The card beside a hovered landing: what lands there and where it is thrown from. It goes
    /// on whichever side of the landing has more of the map, and no wider than that side, so a
    /// small map keeps it clear of the panel beside it.
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
            .map(|tally| {
                let place = tally.item.as_deref().unwrap_or("?");
                if tally.count > 1 {
                    format!("{place} ×{}", tally.count)
                } else {
                    place.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let (x, y) = (side * (landing.at.x / 100.), side * (landing.at.y / 100.));
        let gap = gpui::px(22.);
        let (left_room, right_room) = (x - gap, side - x - gap);
        let to_left = left_room > right_room;
        let room = f32::from(if to_left { left_room } else { right_room });
        Some(
            gpui::div()
                .absolute()
                .top(y - gpui::px(36.))
                .map(|d| {
                    if to_left {
                        d.right(side - x + gap)
                    } else {
                        d.left(x + gap)
                    }
                })
                .max_w(gpui::px(room.clamp(140., 260.)))
                .px(gpui::px(12.))
                .py(gpui::px(10.))
                .rounded(gpui::px(10.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::alpha(theme::panel().into(), 0.95))
                .shadow_lg()
                .child(
                    gpui::div()
                        .text_size(gpui::px(11.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::readable(color.into()))
                        .child(format!("{ability} lands at")),
                )
                .child(
                    gpui::div()
                        .mt(gpui::px(2.))
                        .mb(gpui::px(4.))
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::BOLD)
                        .text_size(gpui::px(17.))
                        .child(place(landing.name.as_deref())),
                )
                .child(
                    gpui::div()
                        .text_size(gpui::px(12.))
                        .line_height(gpui::px(17.))
                        .text_color(theme::muted())
                        .child(format!("from {from}")),
                ),
        )
    }
}

/// The note over the map's square when this side's picture is missing.
fn missing_map() -> Div {
    gpui::div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(gpui::px(12.))
        .border_1()
        .border_color(theme::line())
        .text_color(theme::faint())
        .child("This side's map picture is missing from the library.")
}

/// The note over the map while its markers wait for their pictures.
fn loading_lineups() -> Div {
    gpui::div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            gpui::div()
                .px(gpui::px(14.))
                .py(gpui::px(8.))
                .rounded(gpui::px(8.))
                .bg(theme::alpha(theme::panel().into(), 0.9))
                .text_color(theme::muted())
                .text_size(gpui::px(13.))
                .child("Loading lineups…"),
        )
}

/// A place name, or a stand-in when the library has none.
pub(super) fn place(callout: Option<&str>) -> String {
    callout.unwrap_or("unknown spot").to_string()
}

/// A button of the ability filter: its number key, its icon, its name and how many lineups it has.
#[cfg_attr(
    dylint_lib = "trellis_lints",
    expect(
        shared_ownership,
        reason = "GPUI's ImageSource::Render holds a decoded picture as an Arc<RenderImage>"
    )
)]
fn filter_button(
    id: ElementId,
    key: usize,
    icon: Option<Arc<RenderImage>>,
    name: String,
    count: usize,
    on: bool,
) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .flex()
        .items_center()
        .gap(gpui::px(7.))
        .h(gpui::px(34.))
        .pl(gpui::px(8.))
        .pr(gpui::px(12.))
        .rounded(gpui::px(8.))
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
        .child(ui::kbd(
            key.to_string().into(),
            if on { Cap::Lit } else { Cap::Plain },
        ))
        .children(icon.map(|icon| {
            gpui::img(ImageSource::Render(icon))
                .size(gpui::px(18.))
                .object_fit(ObjectFit::Contain)
                .opacity(if on { 1. } else { 0.65 })
        }))
        .child(name)
        .child(ui::num(count.to_string().into()))
}

/// One throw of the selected landing, as the map draws it.
struct Throw {
    /// Whether it is the lineup pointed at or selected, drawn solid over the others.
    focused: bool,
    /// From the stand spot to where it lands.
    points: Vec<Point>,
}
