//! Keyboard shortcuts.

use arcwise_core::{Direction, View};
use gpui::{App, Context, KeyBinding, KeyDownEvent, Window, actions};

use super::Browser;

actions!(
    arcwise,
    [
        NextLanding,
        PreviousLanding,
        NextStand,
        PreviousStand,
        WatchClip,
        ShowStand,
        ShowAim,
        ShowLanding,
        TogglePause,
        RestartClip,
        SlowMotion,
        SwitchSide,
        Close,
        ToggleSettings,
    ]
);

/// The key context the browser's bindings apply in.
pub(super) const CONTEXT: &str = "Browser";

pub fn bind(cx: &mut App) {
    let context = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", NextLanding, context),
        KeyBinding::new("up", PreviousLanding, context),
        KeyBinding::new("right", NextStand, context),
        KeyBinding::new("left", PreviousStand, context),
        KeyBinding::new("q", WatchClip, context),
        KeyBinding::new("w", ShowStand, context),
        KeyBinding::new("e", ShowAim, context),
        KeyBinding::new("r", ShowLanding, context),
        KeyBinding::new("space", TogglePause, context),
        KeyBinding::new("backspace", RestartClip, context),
        KeyBinding::new("s", SlowMotion, context),
        KeyBinding::new("tab", SwitchSide, context),
        KeyBinding::new("escape", Close, context),
        KeyBinding::new("ctrl-,", ToggleSettings, context),
    ]);
}

impl Browser {
    /// A keyboard choice wins over whatever the pointer happens to rest on.
    fn keyed(&mut self, cx: &mut Context<Self>) {
        self.peek_view = None;
        self.peek_lineup = None;
        self.hovered = None;
        cx.notify();
    }

    /// Whether keys should drive the map: not while a chooser or the settings window covers it.
    fn keys_reach_map(&self) -> bool {
        self.browse.chooser().is_none() && self.lightbox.is_none() && !self.settings_open
    }

    pub(super) fn next_landing(&mut self, _: &NextLanding, _: &mut Window, cx: &mut Context<Self>) {
        self.step_landing(Direction::Next, cx);
    }

    pub(super) fn previous_landing(
        &mut self,
        _: &PreviousLanding,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_landing(Direction::Previous, cx);
    }

    fn step_landing(&mut self, direction: Direction, cx: &mut Context<Self>) {
        if self.keys_reach_map() {
            self.browse.step_landing(direction);
            self.keyed(cx);
        }
    }

    pub(super) fn next_stand(&mut self, _: &NextStand, _: &mut Window, cx: &mut Context<Self>) {
        self.step_stand(Direction::Next, cx);
    }

    pub(super) fn previous_stand(
        &mut self,
        _: &PreviousStand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.step_stand(Direction::Previous, cx);
    }

    fn step_stand(&mut self, direction: Direction, cx: &mut Context<Self>) {
        if self.keys_reach_map() {
            self.browse.step_lineup(direction);
            self.keyed(cx);
        }
    }

    pub(super) fn watch_clip(&mut self, _: &WatchClip, _: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Clip, cx);
    }

    pub(super) fn show_stand(&mut self, _: &ShowStand, _: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Stand, cx);
    }

    pub(super) fn show_aim(&mut self, _: &ShowAim, _: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Aim, cx);
    }

    pub(super) fn show_landing(&mut self, _: &ShowLanding, _: &mut Window, cx: &mut Context<Self>) {
        self.show(View::Lands, cx);
    }

    fn show(&mut self, view: View, cx: &mut Context<Self>) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.browse.set_view(view);
            self.keyed(cx);
        }
    }

    pub(super) fn toggle_pause(&mut self, _: &TogglePause, _: &mut Window, cx: &mut Context<Self>) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.player.update(cx, |player, cx| player.toggle_pause(cx));
        }
    }

    /// Plays the selected lineup's clip again from the start, showing it if another view was.
    pub(super) fn restart_clip(&mut self, _: &RestartClip, _: &mut Window, cx: &mut Context<Self>) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.show(View::Clip, cx);
            self.player.update(cx, |player, cx| player.restart(cx));
        }
    }

    pub(super) fn slow_motion(&mut self, _: &SlowMotion, _: &mut Window, cx: &mut Context<Self>) {
        if self.keys_reach_map() && self.browse.selected_lineup().is_some() {
            self.player.update(cx, |player, cx| player.cycle_speed(cx));
        }
    }

    pub(super) fn switch_side(&mut self, _: &SwitchSide, _: &mut Window, cx: &mut Context<Self>) {
        if self.keys_reach_map() {
            self.browse.toggle_side(&self.library);
            self.keyed(cx);
        }
    }

    /// Closes the topmost thing: the lightbox, the settings window, the chooser if it can close,
    /// or the lineup.
    pub(super) fn close(&mut self, _: &Close, _: &mut Window, cx: &mut Context<Self>) {
        if self.lightbox.take().is_none() {
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
        cx: &mut Context<Self>,
    ) {
        if self.settings_open {
            self.close_settings(cx);
        } else {
            self.open_settings(None, cx);
        }
    }

    /// The number keys pick an ability: 1 for all of them, 2 and up in slot order.
    pub(super) fn number_key(
        &mut self,
        event: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.modified() || !self.keys_reach_map() {
            return;
        }
        if let Ok(n @ 1..=9) = keystroke.key.parse::<usize>() {
            self.browse.ability_key(&self.library, n);
            self.keyed(cx);
        }
    }
}
