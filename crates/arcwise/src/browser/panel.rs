//! The lineup panel: what the selected lineup is, where to stand, aim and throw, and how.

use std::rc::Rc;
use std::sync::Arc;

use arcwise_core::{Bolt, Charge, Landing, Lineup, Tag, View};
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, IntoElement, Pixels, RenderImage,
    SharedString, canvas, div, point, prelude::*, px, rgb, size, svg,
};

use super::Browser;
use super::board::place;
use crate::drawn::{Drawn, Line};
use crate::images::{Image, Look};
use crate::shortcuts::{Command, Shortcuts};
use crate::{theme, ui};

/// How far the stand view zooms into the map.
pub(super) const STAND_ZOOM: f32 = 3.;

/// The least width of the panel's content, which keeps it from reflowing as the panel grows and
/// shrinks.
pub(super) const CONTENT_MIN: f32 = 520.;

impl Browser {
    /// The panel's contents, faded and nudged in as it slides `open`; the browser sets its width.
    pub(super) fn panel(&self, open: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let lineup = self.library.lineup(self.shown?)?;
        let landing = self
            .browse
            .landings()
            .iter()
            .find(|g| g.lineups.contains(&lineup.id));
        let agent = self.library.agent(&lineup.agent)?;
        let color: Hsla = theme::agent(agent.color).into();
        Some(
            div()
                .id("panel")
                .size_full()
                .overflow_x_hidden()
                .overflow_y_scroll()
                .border_l_1()
                .border_color(theme::alpha(theme::line(), open))
                .bg(theme::surface())
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .min_w(px(CONTENT_MIN))
                        .px(px(24.))
                        .py(px(20.))
                        .ml(px(24. * (1. - open)))
                        .opacity(open)
                        .child(self.panel_head(lineup, color, cx))
                        .child(
                            div()
                                .font_family(theme::DISPLAY_FONT)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(22.))
                                .line_height(px(26.))
                                .child(lineup.title.clone()),
                        )
                        .child(route(lineup, color))
                        .children(landing.and_then(|g| self.throw_from(g, color, cx)))
                        .child(self.viewer(lineup, color, cx))
                        .child(self.step_tiles(lineup, color, cx))
                        .children(bow(lineup))
                        .children(technique(lineup, color))
                        .child(keys(cx.global::<Shortcuts>())),
                )
                .into_any_element(),
        )
    }

    fn panel_head(&self, lineup: &Lineup, color: Hsla, cx: &mut Context<Self>) -> Div {
        let ability = self.library.ability(lineup.ability);
        let icon = self.chip_icon(lineup.ability, cx).ready();
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .pl(px(6.))
                    .pr(px(10.))
                    .rounded(px(6.))
                    .bg(theme::alpha(color, 0.16))
                    .text_color(theme::readable(color))
                    .text_size(px(12.))
                    .font_weight(FontWeight::BOLD)
                    .children(icon.map(|icon| {
                        gpui::img(gpui::ImageSource::Render(icon))
                            .size(px(16.))
                            .object_fit(gpui::ObjectFit::Contain)
                    }))
                    .children(ability.map(|a| a.name.clone())),
            )
            .children(lineup.tags.iter().filter_map(|tag| {
                let (label, color) = match tag {
                    Tag::Essential => ("Essential", theme::essential()),
                    Tag::Pro => ("Pro", theme::pro()),
                    Tag::Other => return None,
                };
                Some(
                    div()
                        .h(px(22.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .border_1()
                        .border_color(theme::alpha(color, 0.25))
                        .text_color(color)
                        .text_size(px(11.))
                        .font_weight(FontWeight::BOLD)
                        .child(label),
                )
            }))
    }

    /// The other stand spots for the same landing, when there are any.
    fn throw_from(&self, landing: &Landing, color: Hsla, cx: &mut Context<Self>) -> Option<Div> {
        if landing.lineups.len() < 2 {
            return None;
        }
        let selected = self.browse.selected_lineup();
        let pills = landing.lineups.iter().enumerate().filter_map(|(i, &id)| {
            let lineup = self.library.lineup(id)?;
            let on = selected == Some(id);
            Some(
                div()
                    .id(gpui::ElementId::Name(format!("from-{id}").into()))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .pl(px(5.))
                    .pr(px(12.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(if on { color } else { theme::line().into() })
                    .when(on, |d| d.bg(theme::raised()))
                    .text_color(if on { theme::text() } else { theme::muted() })
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(|s| {
                        s.text_color(theme::text())
                            .border_color(theme::line_strong())
                    })
                    .child(number_box(i + 1, on.then_some(color)))
                    .child(place(lineup.stand.callout.as_deref()))
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.peek_lineup = if *hovered {
                            Some(id)
                        } else {
                            this.peek_lineup.filter(|&p| p != id)
                        };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.peek_lineup = None;
                        this.browse.select_lineup(id);
                        cx.notify();
                    })),
            )
        });
        Some(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(section_label("Throw from").mr(px(6.)))
                .children(pills),
        )
    }

    /// The big picture: the clip, or one step of the throw.
    fn viewer(&self, lineup: &Lineup, color: Hsla, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.peek_view.unwrap_or(self.browse.view());
        let body = match view {
            View::Clip => div()
                .relative()
                .size_full()
                .child(picture(
                    self.image(&self.library.start_still(lineup.id), cx).ready(),
                    false,
                ))
                .child(super::follow::place(self.clip_slot.clone()))
                .into_any_element(),
            View::Stand => {
                self.stand_zoom(lineup, color, px(16.), self.stand_view.clone(), true, cx)
            }
            View::Aim => match self.aim(lineup.id, cx) {
                Image::Missing => missing("This lineup has no aim picture. Follow the clip."),
                image => picture(image.ready(), false),
            },
            View::Lands => match self.image(&self.library.end_still(lineup.id), cx) {
                Image::Missing => missing("The landing frame for this clip is missing."),
                image => picture(image.ready(), false),
            },
        };
        // The aim and landing pictures open full size when clicked.
        let zoom = match view {
            View::Aim => matches!(self.aim(lineup.id, cx), Image::Ready(_))
                .then(|| self.library.aim_image(lineup.id)),
            View::Lands => Some(self.library.end_still(lineup.id))
                .filter(|path| matches!(self.image(path, cx), Image::Ready(_))),
            View::Clip | View::Stand => None,
        };
        // Over the clip, the browser draws the note with the clip; see [`Browser::clip_note`].
        let note = (view == View::Aim)
            .then(|| lineup.steps.as_ref()?.note.clone())
            .flatten();
        div()
            .id("viewer")
            .relative()
            .w_full()
            .aspect_ratio(16. / 9.)
            .rounded(px(12.))
            .overflow_hidden()
            .border_1()
            .border_color(theme::line())
            .bg(gpui::black())
            .when_some(zoom, |d, path| {
                let full = path.clone();
                d.cursor_pointer()
                    // The aim picture shows shrunk; pointing at it starts the full size.
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.images
                                .update(cx, |images, cx| images.fetch(&full, Look::Plain, cx));
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lightbox = Some(path.clone());
                        cx.notify();
                    }))
            })
            .child(body)
            .children(note.map(note_box))
    }

    /// The shown lineup's note, to draw over its clip.
    pub(super) fn clip_note(&self) -> Option<Div> {
        let lineup = self.library.lineup(self.shown?)?;
        Some(note_box(lineup.steps.as_ref()?.note.clone()?))
    }

    /// The map around the lineup's stand spot, with its throw.
    fn stand_zoom(
        &self,
        lineup: &Lineup,
        color: Hsla,
        dot: Pixels,
        drawn: Rc<Drawn>,
        zoomed: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let map = self.map_picture(&lineup.map, lineup.side, cx).ready();
        // The viewer's stand view shows the map three times its width: drawn just around the
        // stand spot, rather than the whole map that large.
        let near = zoomed
            .then(|| {
                let path = self.library.map_image(&lineup.map, lineup.side);
                let look = self.stand_window(lineup);
                self.images
                    .update(cx, |images, cx| images.get_as(&path, look, cx))
                    .ready()
            })
            .flatten();
        let stand = lineup.stand.at();
        let land = lineup.land.at();
        let throw: Vec<_> = std::iter::once(stand)
            .chain(lineup.path.iter().copied())
            .collect();
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let side = bounds.size.width * STAND_ZOOM;
                let origin = point(
                    bounds.center().x - side * (stand.x / 100.),
                    bounds.center().y - side * (stand.y / 100.),
                );
                let to_px = |p: arcwise_core::Point| {
                    point(
                        origin.x + side * (p.x / 100.),
                        origin.y + side * (p.y / 100.),
                    )
                };
                window.paint_quad(gpui::fill(bounds, theme::bg()));
                if let Some(near) = near {
                    window
                        .paint_image(bounds, bounds, Default::default(), near, 0, false)
                        .ok();
                } else if let Some(map) = map {
                    let image = Bounds::new(origin, size(side, side));
                    window
                        .paint_image(bounds, image, Default::default(), map, 0, false)
                        .ok();
                }
                let line = Line {
                    points: throw.iter().map(|&p| to_px(p) - bounds.origin).collect(),
                    width: px(2.),
                    color,
                    dash: Some((px(6.), px(5.))),
                };
                drawn.paint(bounds, vec![line], window);
                window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                    let r = bounds.size.width * 0.022;
                    circle(window, to_px(land), r, color);
                    circle(
                        window,
                        bounds.center(),
                        dot / 2. + px(4.),
                        theme::alpha(color, 0.35),
                    );
                    circle(window, bounds.center(), dot / 2., color);
                });
            },
        )
        .size_full()
        .into_any_element()
    }

    fn step_tiles(&self, lineup: &Lineup, color: Hsla, cx: &mut Context<Self>) -> Div {
        let view = self.peek_view.unwrap_or(self.browse.view());
        let pinned = self.browse.view();
        let aim = self.aim(lineup.id, cx);
        let has_aim = !matches!(aim, Image::Missing);
        let tiles = [
            (
                View::Clip,
                Command::WatchClip,
                "Watch",
                picture(
                    self.image(&self.library.start_still(lineup.id), cx).ready(),
                    true,
                ),
            ),
            (
                View::Stand,
                Command::ShowStand,
                "1 · Stand",
                self.stand_zoom(lineup, color, px(9.), self.stand_tile.clone(), false, cx),
            ),
            (
                View::Aim,
                Command::ShowAim,
                "2 · Aim",
                if has_aim {
                    picture(aim.ready(), false)
                } else {
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .child("none")
                        .into_any_element()
                },
            ),
            (
                View::Lands,
                Command::ShowLanding,
                "3 · Lands",
                picture(
                    self.image(&self.library.end_still(lineup.id), cx).ready(),
                    false,
                ),
            ),
        ];
        // Each tile's key as the settings have it, and none for a view left without one.
        let shortcuts = cx.global::<Shortcuts>().clone();
        div().flex().gap(px(8.)).children(tiles.into_iter().map(
            |(tile_view, command, label, media)| {
                let on = tile_view == view;
                let border: Hsla = if tile_view == pinned {
                    color
                } else if on {
                    theme::text().into()
                } else {
                    gpui::transparent_black()
                };
                div()
                    .id(label)
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .cursor_pointer()
                    .child(
                        div()
                            .w_full()
                            .aspect_ratio(16. / 9.)
                            .rounded(px(8.))
                            .overflow_hidden()
                            .border_2()
                            .border_color(border)
                            .bg(gpui::black())
                            .child(media),
                    )
                    // A long shortcut such as Ctrl+Shift+W shortens the name beside it rather than
                    // spilling into the next tile.
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .min_w_0()
                            .font_weight(FontWeight::BOLD)
                            .text_size(px(12.))
                            .children(shortcuts.first(command).map(|key| {
                                ui::kbd(key.label(), false)
                                    .flex_none()
                                    .max_w(px(96.))
                                    .overflow_hidden()
                            }))
                            .child(div().min_w_0().truncate().child(label)),
                    )
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        // A drag along the clip's progress bar that strays onto a tile keeps
                        // the clip in view, rather than swapping it for the tile's picture.
                        if *hovered && this.player.read(cx).scrubbing() {
                            return;
                        }
                        this.peek_view = if *hovered {
                            Some(tile_view)
                        } else {
                            this.peek_view.filter(|&v| v != tile_view)
                        };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.peek_view = None;
                        this.browse.set_view(tile_view);
                        cx.notify();
                    }))
            },
        ))
    }
}

/// A note along the bottom left of the viewer.
fn note_box(note: impl Into<SharedString>) -> Div {
    div()
        .absolute()
        .left(px(10.))
        .bottom(px(12.))
        .px(px(10.))
        .py(px(6.))
        .rounded(px(7.))
        .bg(theme::alpha(gpui::black(), 0.78))
        .text_color(gpui::rgb(theme::DARK.text))
        .text_size(px(13.))
        .child(note.into())
}

/// A picture filling its box, with a play mark over it for the clip's tile.
fn picture(image: Option<Arc<RenderImage>>, play: bool) -> AnyElement {
    div()
        .relative()
        .size_full()
        .children(image.map(ui::cover))
        .when(play, |d| d.child(ui::play_glyph(px(14.))))
        .into_any_element()
}

fn missing(text: &'static str) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        // In the viewer, which is black in either theme.
        .text_color(gpui::rgb(theme::DARK.muted))
        .child(text)
        .into_any_element()
}

fn circle(
    window: &mut gpui::Window,
    center: gpui::Point<Pixels>,
    r: Pixels,
    color: impl Into<Hsla>,
) {
    let bounds = Bounds::new(point(center.x - r, center.y - r), size(r * 2., r * 2.));
    window.paint_quad(gpui::fill(bounds, color.into()).corner_radii(r));
}

/// Stand → Lands, with their place names.
fn route(lineup: &Lineup, color: Hsla) -> Div {
    let part = |label: &'static str, callout: Option<&str>| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(ui::label(label).font_weight(FontWeight::BOLD))
            .child(
                div()
                    .font_family(theme::DISPLAY_FONT)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(16.))
                    .child(place(callout)),
            )
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(part("Stand", lineup.stand.callout.as_deref()))
        .child(ui::arrow(theme::faint(), color).mt(px(12.)))
        .child(part("Lands", lineup.land.callout.as_deref()))
}

/// The throw's steps as a numbered list.
fn technique(lineup: &Lineup, color: Hsla) -> Option<Div> {
    let lines: Vec<String> = lineup.steps.as_ref()?.lines().collect();
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(10.))
            .bg(theme::raised())
            .children(lines.into_iter().enumerate().map(|(i, line)| {
                div()
                    .flex()
                    .gap(px(10.))
                    .line_height(px(22.))
                    .child(
                        div()
                            .w(px(14.))
                            .flex_none()
                            .text_color(theme::readable(color))
                            .font_weight(FontWeight::BOLD)
                            .child(format!("{}.", i + 1)),
                    )
                    .child(div().flex_1().min_w_0().child(line))
            })),
    )
}

/// The bow's HUD for each of Sova's bolts, drawn as the game draws it: the charge bar filled into
/// the section to release in, red when full, and a lit diamond for each bounce.
fn bow(lineup: &Lineup) -> Option<Div> {
    let bolts = &lineup.bolts;
    if bolts.is_empty() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(14.))
            .py(px(12.))
            .rounded(px(10.))
            .bg(theme::raised())
            .children(bolts.iter().enumerate().map(|(i, bolt)| {
                let name = match (bolts.len(), i) {
                    (1, _) => "Bow",
                    (_, 0) => "1st bolt",
                    _ => "2nd bolt",
                };
                let hud = div()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .font_weight(FontWeight::BOLD)
                            .child(name),
                    )
                    .child(bow_hud(bolt))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_color(theme::muted())
                            .child(format!(
                                "{}, {}",
                                bolt.charge.label(),
                                bolt.bounces.label().to_lowercase()
                            )),
                    );
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(hud)
                    .children(bolt.aim.as_ref().map(|aim| {
                        div()
                            .pl(px(78.))
                            .line_height(px(20.))
                            .child(format!("Aim: {aim}"))
                    }))
            })),
    )
}

/// The game's own fill colors, so the bar reads like the one on screen.
const CHARGE_FILL: u32 = 0x4bcaa9;
const CHARGE_FULL: u32 = 0xf02d2d;
const DIAMOND: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M6 .5 11.5 6 6 11.5.5 6Z"/></svg>"#;
const DIAMOND_OUTLINE: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 12"><path d="M6 1.2 10.8 6 6 10.8 1.2 6Z" fill="none" stroke="#000" stroke-width="1.4"/></svg>"##;

/// One bolt's bar and bounce diamonds.
fn bow_hud(bolt: &Bolt) -> Div {
    let full = bolt.charge == Charge::Full;
    let reached = bolt.charge.sections();
    // The released section shows part filled, as the fill was when the bolt left.
    let section = |i: u8| {
        let fill = if full || i < reached {
            1.
        } else if i == reached {
            0.45
        } else {
            0.
        };
        div().flex_1().h_full().bg(theme::raised()).child(
            div().h_full().w(gpui::relative(fill)).bg(rgb(if full {
                CHARGE_FULL
            } else {
                CHARGE_FILL
            })),
        )
    };
    div()
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(6.))
        .child(
            div()
                .w(px(156.))
                .h(px(12.))
                .flex()
                .gap(px(2.))
                .p(px(1.))
                .border_1()
                .border_color(theme::muted())
                .bg(theme::muted())
                .children((0..3).map(section)),
        )
        .child(div().flex().gap(px(6.)).children((0..2).map(|i| {
            svg()
                .data(if i < bolt.bounces.count() {
                    DIAMOND
                } else {
                    DIAMOND_OUTLINE
                })
                .size(px(12.))
                .text_color(theme::text())
        })))
}

fn section_label(text: &'static str) -> Div {
    ui::label(text).font_weight(FontWeight::BOLD)
}

/// The number of a stand spot, as on its map marker.
fn number_box(n: usize, lit: Option<Hsla>) -> Div {
    div()
        .size(px(22.))
        .rounded(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .bg(lit.unwrap_or_else(|| theme::text().into()))
        .text_color(if lit.is_some() {
            gpui::black()
        } else {
            theme::bg().into()
        })
        .font_weight(FontWeight::EXTRA_BOLD)
        .text_size(px(12.))
        .child(n.to_string())
}

/// The keyboard shortcuts, along the panel's foot: the first of each command's, as changed in
/// the settings. A command left without a key is not shown.
fn keys(shortcuts: &Shortcuts) -> Div {
    let hint = |commands: &[Command], what: &'static str| {
        let caps: Vec<_> = commands
            .iter()
            .filter_map(|&c| shortcuts.first(c))
            .map(|s| ui::kbd(s.label(), false))
            .collect();
        (!caps.is_empty()).then(|| {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .children(caps)
                .child(div().ml(px(2.)).child(what))
        })
    };
    div()
        .flex()
        .flex_wrap()
        .gap_x(px(18.))
        .gap_y(px(8.))
        .pt(px(14.))
        .border_t_1()
        .border_color(theme::line())
        .text_size(px(12.))
        .text_color(theme::faint())
        .children(hint(
            &[Command::PreviousLanding, Command::NextLanding],
            "landing",
        ))
        .children(hint(
            &[Command::PreviousStand, Command::NextStand],
            "throw from",
        ))
        .children(hint(&[Command::TogglePause], "pause"))
        .children(hint(&[Command::RestartClip], "restart"))
        .children(hint(&[Command::SlowMotion], "slow-mo"))
        .children(hint(&[Command::SwitchSide], "side"))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(ui::kbd("Esc", false))
                .child(div().ml(px(2.)).child("close")),
        )
}
