//! What the keyboard shortcuts do; [`crate::shortcuts`] says which keys do it.

use arcwise_core::{Direction, View};
use gpui::{Context, KeyDownEvent, Window};

use super::browser_view::BrowserView;
use crate::player::Player;
use crate::shortcuts::{
    NextLanding, NextStand, PreviousLanding, PreviousStand, RestartClip, ShowAim, ShowLanding,
    ShowStand, SlowMotion, SwitchSide, TogglePause, ToggleSettings, WatchClip,
};

impl BrowserView {
    /// A keyboard choice wins over whatever the pointer happens to rest on.
    fn keyed(&mut self, cx: &mut Context<'_, Self>) {
        self.peek_view = None;
        self.peek_lineup = None;
        self.hovered = None;
        cx.notify();
    }

    /// Whether keys should drive the map: not while a chooser or the settings window covers it.
    fn keys_reach_map(&self) -> bool {
        self.browse.chooser().is_none() && self.lightbox.is_none() && !self.settings_open
    }

    pub(super) fn next_landing(
        &mut self,
        _: &NextLanding,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.step_landing(Direction::Next, cx);
    }

    pub(super) fn previous_landing(
        &mut self,
        _: &PreviousLanding,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.step_landing(Direction::Previous, cx);
    }

    fn step_landing(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        if self.keys_reach_map() {
            self.browse.step_landing(direction);
            self.keyed(cx);
        }
    }

    pub(super) fn next_stand(&mut self, _: &NextStand, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.step_stand(Direction::Next, cx);
    }

    pub(super) fn previous_stand(
        &mut self,
        _: &PreviousStand,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.step_stand(Direction::Previous, cx);
    }

    fn step_stand(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        if self.keys_reach_map() {
            self.browse.step_lineup(direction);
            self.keyed(cx);
        }
    }

    pub(super) fn watch_clip(&mut self, _: &WatchClip, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.show(View::Clip, cx);
    }

    pub(super) fn show_stand(&mut self, _: &ShowStand, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.show(View::Stand, cx);
    }

    pub(super) fn show_aim(&mut self, _: &ShowAim, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.show(View::Aim, cx);
    }

    pub(super) fn show_landing(
        &mut self,
        _: &ShowLanding,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.show(View::Lands, cx);
    }

    fn show(&mut self, view: View, cx: &mut Context<'_, Self>) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.browse.set_view(view);
            self.keyed(cx);
        }
    }

    pub(super) fn toggle_pause(
        &mut self,
        _: &TogglePause,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.player.update(cx, Player::toggle_pause);
        }
    }

    /// Plays the selected lineup's clip again from the start, showing it if another view was.
    pub(super) fn restart_clip(
        &mut self,
        _: &RestartClip,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.show(View::Clip, cx);
            self.player.update(cx, Player::restart);
        }
    }

    pub(super) fn slow_motion(
        &mut self,
        _: &SlowMotion,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.player.update(cx, Player::cycle_speed);
        }
    }

    pub(super) fn switch_side(
        &mut self,
        _: &SwitchSide,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.keys_reach_map() {
            self.browse.toggle_side(&self.library);
            self.keyed(cx);
        }
    }

    /// Closes the topmost thing: the lightbox, the settings window, the chooser if it can close,
    /// or the lineup. A shortcut's question is above the settings window: Esc answers it with no,
    /// and leaves the settings open.
    fn close(&mut self, cx: &mut Context<'_, Self>) {
        if self.lightbox.take().is_none() && self.conflict.take().is_none() {
            if self.settings_open {
                self.close_settings(cx);
            } else if self.browse.chooser().is_some() {
                self.browse.dismiss_chooser(&self.library);
            } else {
                self.browse.clear_selection();
            }
        }
        self.keyed(cx);
    }

    pub(super) fn toggle_settings(
        &mut self,
        _: &ToggleSettings,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.settings_open {
            self.close_settings(cx);
        } else {
            self.open_settings(None, cx);
        }
    }

    /// The keys that stay as they are. Esc closes the topmost thing, once per press: Windows'
    /// repeats of a held Esc do not go on to close what is under it. The number keys pick an
    /// ability: 1 for all of them, 2 and up in slot order.
    pub(super) fn fixed_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() {
            return;
        }
        if keystroke.key == "escape" {
            if !event.is_held {
                self.close(cx);
            }
            return;
        }
        if !self.keys_reach_map() {
            return;
        }
        let number: Result<usize, _> = keystroke.key.parse();
        if let Ok(n @ 1..=9) = number {
            self.browse.ability_key(&self.library, n);
            self.keyed(cx);
        }
    }
}
