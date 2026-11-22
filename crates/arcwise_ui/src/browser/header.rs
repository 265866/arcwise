//! The header: the map and agent pickers, the side switch and the settings gear.

use arcwise_core::{Chooser, Side};
use gpui::prelude::FluentBuilder;
use gpui::{
    Context, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, ParentElement,
    SharedString, Stateful, Styled, Window,
};

use super::browser_view::BrowserView;
use crate::ui::Shade;
use crate::{theme, ui};

impl BrowserView {
    /// The header bar: the pickers on the left, the gear on the right.
    pub(super) fn header(&self, window: &Window, cx: &mut Context<'_, Self>) -> Div {
        ui::bar()
            .child(self.map_picker(window, cx))
            .child(self.agent_picker(window, cx))
            .children(self.side_switch(cx))
            .child(gpui::div().flex_1())
            .child(self.settings_button(cx))
    }

    /// The map picker. The header stays usable over an enlarged picture, which a new choice puts
    /// away.
    fn map_picker(&self, window: &Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let map = self.browse.map().and_then(|id| self.library.map(id));
        let thumb = map.map(|m| self.image(&self.library.map_card(&m.id), cx));
        picker(
            "map-picker",
            self.browse.chooser() == Some(Chooser::Map),
            gpui::div()
                .w(gpui::px(56.))
                .h(gpui::px(34.))
                .rounded(gpui::px(7.))
                .overflow_hidden()
                .bg(theme::raised())
                .children(thumb.and_then(|t| t.ready()).map(ui::cover)),
            "Map",
            map.map_or("Choose".into(), |m| m.name.clone().into()),
        )
        .children(self.dropdown(Chooser::Map, window, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.close_settings(cx);
                this.lightbox = None;
                this.browse.toggle_chooser(&this.library, Chooser::Map);
                cx.notify();
            }),
        )
    }

    /// The agent picker, which like the map's puts an enlarged picture away.
    fn agent_picker(&self, window: &Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let agent = self.browse.agent().and_then(|id| self.library.agent(id));
        let face = match agent {
            Some(agent) => {
                let portrait = self.image(&self.library.portrait(&agent.id), cx);
                ui::face(
                    agent,
                    portrait.ready(),
                    Shade::Color,
                    gpui::px(34.),
                    gpui::px(7.),
                )
                .into_any_element()
            }
            None => gpui::div()
                .size(gpui::px(34.))
                .rounded(gpui::px(7.))
                .bg(theme::raised())
                .into_any_element(),
        };
        picker(
            "agent-picker",
            self.browse.chooser() == Some(Chooser::Agent),
            face,
            "Agent",
            agent.map_or("Choose".into(), |a| a.name.clone().into()),
        )
        .children(self.dropdown(Chooser::Agent, window, cx))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.close_settings(cx);
                this.lightbox = None;
                this.browse.toggle_chooser(&this.library, Chooser::Agent);
                cx.notify();
            }),
        )
    }

    /// Attack and defense, with how many lineups each has. Hidden until there are lineups.
    fn side_switch(&self, cx: &mut Context<'_, Self>) -> Option<impl IntoElement> {
        use gpui::StatefulInteractiveElement as _;

        if !self.browse.can_close(&self.library) {
            return None;
        }
        let button = |side: Side, cx: &mut Context<'_, Self>| {
            let count = self.browse.side_count(&self.library, side);
            let on = self.browse.side() == side;
            let dot = match side {
                Side::Attack => theme::attack(),
                Side::Defense => theme::defense(),
            };
            gpui::div()
                .id(side.label())
                .flex()
                .items_center()
                .gap(gpui::px(8.))
                .h(gpui::px(32.))
                .px(gpui::px(14.))
                .rounded(gpui::px(7.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(theme::raised()))
                .when(count == 0, |d| d.opacity(0.35))
                .when(count > 0 && !on, |d| {
                    d.cursor_pointer().hover(|s| s.text_color(theme::text()))
                })
                .child(
                    gpui::div()
                        .size(gpui::px(7.))
                        .rounded_full()
                        .bg(if on { dot } else { theme::muted() })
                        .when(!on, |d| d.opacity(0.5)),
                )
                .child(side.label())
                .child(ui::num(count.to_string().into()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.lightbox = None;
                    this.browse.set_side(&this.library, side);
                    cx.notify();
                }))
        };
        Some(
            gpui::div()
                .flex()
                .gap(gpui::px(4.))
                .occlude()
                .ml(gpui::px(6.))
                .p(gpui::px(4.))
                .rounded(gpui::px(10.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::backdrop())
                .child(button(Side::Attack, cx))
                .child(button(Side::Defense, cx)),
        )
    }
}

/// A header button that opens a chooser: a picture, a label and the current choice. It occludes
/// the header under it, which moves the window where nothing else is.
fn picker(
    id: &'static str,
    open: bool,
    picture: impl IntoElement,
    label: &'static str,
    value: SharedString,
) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .occlude()
        .relative()
        .flex()
        .items_center()
        .gap(gpui::px(10.))
        .h(gpui::px(42.))
        .pl(gpui::px(4.))
        .pr(gpui::px(12.))
        .rounded(gpui::px(10.))
        .border_1()
        .border_color(theme::line())
        .cursor_pointer()
        .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
        .when(open, |d| {
            d.bg(theme::raised()).border_color(theme::line_hover())
        })
        .child(picture)
        .child(
            gpui::div()
                .flex()
                .flex_col()
                .gap(gpui::px(2.))
                .min_w(gpui::px(64.))
                .child(ui::label(label))
                .child(
                    gpui::div()
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::BOLD)
                        .text_size(gpui::px(15.))
                        .line_height(gpui::px(16.))
                        .child(value),
                ),
        )
        .child(ui::chevron(theme::muted().into()))
}
