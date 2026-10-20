//! The header: the map and agent pickers and the side switch.

use arcwise_core::{Chooser, Side};
use gpui::{
    Context, Div, FontWeight, IntoElement, MouseButton, SharedString, Window, div, prelude::*, px,
};

use super::Browser;
use crate::{theme, ui};

impl Browser {
    pub(super) fn header(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        ui::bar()
            .child(self.map_picker(window, cx))
            .child(self.agent_picker(window, cx))
            .children(self.side_switch(cx))
            .child(div().flex_1())
            .child(self.library_button(cx))
    }

    /// The library's folder name; clicking it opens another library.
    fn library_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.library.root().file_name().map_or_else(
            || self.library.root().display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        div()
            .id("library")
            .flex()
            .flex_col()
            .items_end()
            .gap(px(2.))
            .px(px(10.))
            .py(px(4.))
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|s| s.bg(theme::raised()))
            .child(ui::label("Library"))
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(name),
            )
            .on_click(cx.listener(|_, _, _, cx| cx.emit(super::ChangeLibrary)))
    }

    fn map_picker(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let map = self.browse.map().and_then(|id| self.library.map(id));
        let thumb = map.map(|m| self.image(&self.library.map_card(&m.id), cx));
        picker(
            "map-picker",
            self.browse.chooser() == Some(Chooser::Map),
            div()
                .w(px(56.))
                .h(px(34.))
                .rounded(px(7.))
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
                this.browse.toggle_chooser(&this.library, Chooser::Map);
                cx.notify();
            }),
        )
    }

    fn agent_picker(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let agent = self.browse.agent().and_then(|id| self.library.agent(id));
        let face = match agent {
            Some(agent) => {
                let portrait = self.image(&self.library.portrait(&agent.id), cx);
                ui::face(agent, portrait.ready(), false, px(34.), px(7.)).into_any_element()
            }
            None => div()
                .size(px(34.))
                .rounded(px(7.))
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
                this.browse.toggle_chooser(&this.library, Chooser::Agent);
                cx.notify();
            }),
        )
    }

    /// Attack and defense, with how many lineups each has. Hidden until there are lineups.
    fn side_switch(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.browse.can_close(&self.library) {
            return None;
        }
        let button = |side: Side, cx: &mut Context<Self>| {
            let count = self.browse.side_count(&self.library, side);
            let on = self.browse.side() == side;
            let dot = match side {
                Side::Attack => theme::attack(),
                Side::Defense => theme::defense(),
            };
            div()
                .id(side.label())
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(32.))
                .px(px(14.))
                .rounded(px(7.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if on { theme::text() } else { theme::muted() })
                .when(on, |d| d.bg(theme::raised()))
                .when(count == 0, |d| d.opacity(0.35))
                .when(count > 0 && !on, |d| {
                    d.cursor_pointer().hover(|s| s.text_color(theme::text()))
                })
                .child(
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(if on { dot } else { theme::muted() })
                        .when(!on, |d| d.opacity(0.5)),
                )
                .child(side.label())
                .child(ui::num(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.browse.set_side(&this.library, side);
                    cx.notify();
                }))
        };
        Some(
            div()
                .flex()
                .gap(px(4.))
                .ml(px(6.))
                .p(px(4.))
                .rounded(px(10.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::backdrop())
                .child(button(Side::Attack, cx))
                .child(button(Side::Defense, cx)),
        )
    }
}

/// A header button that opens a chooser: a picture, a label and the current choice.
fn picker(
    id: &'static str,
    open: bool,
    picture: impl IntoElement,
    label: &'static str,
    value: SharedString,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(42.))
        .pl(px(4.))
        .pr(px(12.))
        .rounded(px(10.))
        .border_1()
        .border_color(theme::line())
        .cursor_pointer()
        .hover(|s| s.bg(theme::raised()).border_color(theme::line_hover()))
        .when(open, |d| {
            d.bg(theme::raised()).border_color(theme::line_hover())
        })
        .child(picture)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .min_w(px(64.))
                .child(ui::label(label))
                .child(
                    div()
                        .font_family(theme::DISPLAY_FONT)
                        .font_weight(FontWeight::BOLD)
                        .text_size(px(15.))
                        .line_height(px(16.))
                        .child(value),
                ),
        )
        .child(ui::chevron(theme::muted()))
}
