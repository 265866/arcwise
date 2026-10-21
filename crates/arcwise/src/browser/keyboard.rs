//! The settings window's Keyboard tab, where shortcuts are changed: click one, then press the keys
//! for it.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, Div, FontWeight, IntoElement, KeyUpEvent, Keystroke, Modifiers,
    ModifiersChangedEvent, SharedString, Stateful, Window, div, prelude::*, px,
};

use super::settings_window::{Tab, WIDTH};
use super::{Browser, SetShortcuts};
use crate::shortcuts::{self, Command, Group, MOST, Refused, Shortcut, Shortcuts};
use crate::{theme, ui};

/// A circling arrow, on the button that puts a command's shortcuts back.
const RESET: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3M4.5 4v4h4" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;
/// Longer than Windows waits before repeating a held key, and than it waits between repeats.
const REPEATS_WITHIN: Duration = Duration::from_millis(750);

/// A plus, on the button that adds a second shortcut.
const PLUS: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 5v14M5 12h14" fill="none" stroke="#000" stroke-width="2.5" stroke-linecap="round"/></svg>"##;

/// A shortcut being chosen: which command, which of its shortcuts, and what has been pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Recording {
    command: Command,
    slot: usize,
    /// The modifiers held so far, shown before the key that finishes the shortcut.
    held: Modifiers,
    /// Why the last key pressed cannot be a shortcut.
    refused: Option<Refused>,
}

/// A shortcut chosen for one command that another already has, waiting for a yes or no.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Conflict {
    command: Command,
    slot: usize,
    shortcut: Shortcut,
    holder: Command,
}

impl Browser {
    /// Takes a key press while a shortcut is being chosen, before any binding can act on it.
    pub(super) fn recorded(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(mut recording) = self.recording else {
            // Windows repeats a held key; the one that ended recording does nothing more while it
            // is held, so holding Esc to cancel does not also close the settings. Repeats come
            // every few tens of milliseconds, so a pause means a new press, even if the release
            // went to another window.
            match &mut self.ended_by {
                Some((ended, at)) if ended == keystroke && at.elapsed() < REPEATS_WITHIN => {
                    *at = Instant::now();
                    cx.stop_propagation();
                }
                _ => self.ended_by = None,
            }
            return;
        };
        // Only while the Keyboard tab shows, whichever way it went away.
        if !self.settings_open || self.settings_tab != Tab::Keyboard {
            self.stop_recording();
            cx.notify();
            return;
        }
        // A key press says which modifiers are held, which a release seen elsewhere may not have;
        // Windows leaves Shift out for a symbol typed with it.
        recording.held = keystroke.modifiers;
        if shortcuts::shifted_key(&keystroke.key).is_some() {
            recording.held.shift = true;
        }
        // Esc on its own cancels, rather than being refused.
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            cx.stop_propagation();
            self.ended_by = Some((keystroke.clone(), Instant::now()));
            self.recording = None;
            cx.notify();
            return;
        }
        let pressed = Shortcut::pressed(keystroke);
        // Windows' own keys, such as Alt+F4, still do what Windows does with them.
        if !matches!(pressed, Some(Err(Refused::Windows))) {
            cx.stop_propagation();
        }
        match pressed {
            // A modifier on its own: wait for the key.
            None => self.recording = Some(recording),
            Some(Err(refused)) => {
                recording.refused = Some(refused);
                self.recording = Some(recording);
            }
            Some(Ok(shortcut)) => {
                self.ended_by = Some((keystroke.clone(), Instant::now()));
                self.recording = None;
                let shortcuts = cx.global::<Shortcuts>();
                match shortcuts.holder(&shortcut) {
                    Some(holder) if holder == recording.command => {}
                    Some(holder) => {
                        self.conflict = Some(Conflict {
                            command: recording.command,
                            slot: recording.slot,
                            shortcut,
                            holder,
                        });
                    }
                    None => {
                        let (command, slot) = (recording.command, recording.slot);
                        self.change_shortcuts(cx, |s| s.set(command, slot, shortcut));
                    }
                }
            }
        }
        cx.notify();
    }

    /// Forgets the modifiers held and the key that ended recording when the window gains or loses
    /// focus: their releases may have gone to another window, as with Alt+Tab.
    pub(super) fn activation_changed(&mut self, cx: &mut Context<Self>) {
        self.ended_by = None;
        if let Some(recording) = &mut self.recording {
            recording.held = Modifiers::default();
            cx.notify();
        }
    }

    pub(super) fn key_released(&mut self, _: &KeyUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.ended_by = None;
    }

    pub(super) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(recording) = &mut self.recording {
            recording.held = event.modifiers;
            cx.notify();
        }
    }

    /// Stops choosing a shortcut and forgets one waiting for a yes or no.
    pub(super) fn stop_recording(&mut self) {
        self.recording = None;
        self.conflict = None;
    }

    /// Changes the shortcuts with `change`, binds them, and has them saved.
    fn change_shortcuts(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Shortcuts)) {
        let mut shortcuts = cx.global::<Shortcuts>().clone();
        change(&mut shortcuts);
        let changes = shortcuts.changes();
        // Bound once this key press is over, rather than while GPUI is dispatching it.
        cx.defer(move |cx| shortcuts::apply(shortcuts, cx));
        cx.emit(SetShortcuts(changes));
        cx.notify();
    }

    fn record(&mut self, command: Command, slot: usize, cx: &mut Context<Self>) {
        self.conflict = None;
        self.recording = Some(Recording {
            command,
            slot,
            held: Modifiers::default(),
            refused: None,
        });
        cx.notify();
    }

    pub(super) fn keyboard(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let shortcuts = cx.global::<Shortcuts>().clone();
        let group = |g: Group, cx: &mut Context<Self>| {
            let mut rows: Vec<AnyElement> = Command::ALL
                .into_iter()
                .filter(|c| c.group() == g)
                .map(|c| self.command_row(c, &shortcuts, cx).into_any_element())
                .collect();
            match g {
                Group::Browse => rows.push(fixed("Ability filters", &["1", "…", "9"])),
                Group::Window => rows.push(fixed("Close", &["Esc"])),
                Group::Views | Group::Clip => {}
            }
            section(g.label(), rows)
        };
        let [browse, views, clip, window_group] = Group::ALL.map(|g| group(g, cx));
        // Two columns while the settings window has its full width, inside its 24px margins.
        let two = window.viewport_size().width >= WIDTH + px(48.);
        let columns = if two {
            div()
                .flex()
                .items_start()
                .gap(px(14.))
                .child(column().child(browse).child(window_group))
                .child(column().child(views).child(clip))
        } else {
            column()
                .child(browse)
                .child(views)
                .child(clip)
                .child(window_group)
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .mt(px(10.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.5))
                            .text_color(theme::muted())
                            .child(
                                "Click a shortcut to change it. They work while Arcwise has focus.",
                            ),
                    )
                    .child(
                        ui::button("reset-shortcuts", "Reset all")
                            .h(px(32.))
                            .px(px(12.))
                            .text_size(px(13.))
                            .when(shortcuts.all_default(), |b| b.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop_recording();
                                if !cx.global::<Shortcuts>().all_default() {
                                    this.change_shortcuts(cx, |s| *s = Shortcuts::default());
                                }
                                cx.notify();
                            })),
                    ),
            )
            .children(self.conflict_banner(cx))
            .child(columns)
            .into_any_element()
    }

    /// Asks before moving a shortcut from the command that has it.
    fn conflict_banner(&self, cx: &mut Context<Self>) -> Option<Div> {
        let conflict = self.conflict.clone()?;
        let key = conflict.shortcut.label();
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .mt(px(12.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(12.))
                .border_1()
                .border_color(theme::accent())
                .bg(theme::alpha(theme::accent(), 0.08))
                .text_size(px(13.))
                .child(div().flex_1().child(format!(
                    "{key} already does {}. Use it for {} instead?",
                    conflict.holder.label(),
                    conflict.command.label()
                )))
                .child(
                    ui::button("conflict-cancel", "Cancel")
                        .h(px(32.))
                        .px(px(12.))
                        .text_size(px(13.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.conflict = None;
                            cx.notify();
                        })),
                )
                .child(
                    accent_button("conflict-take", format!("Use {key} here")).on_click(
                        cx.listener(|this, _, _, cx| {
                            if let Some(c) = this.conflict.take() {
                                this.change_shortcuts(cx, |s| s.set(c.command, c.slot, c.shortcut));
                            }
                        }),
                    ),
                ),
        )
    }

    fn command_row(
        &self,
        command: Command,
        shortcuts: &Shortcuts,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let keys = shortcuts.keys(command);
        let recording = self.recording.filter(|r| r.command == command);
        let conflicted = self
            .conflict
            .as_ref()
            .is_some_and(|c| c.command == command || c.holder == command);
        let group = SharedString::from(format!("row-{command:?}"));
        // The keys stay as they are while new ones are chosen, the one being replaced marked; the
        // prompt has a line of its own below, so nothing in the row moves or wraps.
        let slots: Vec<AnyElement> = keys
            .iter()
            .enumerate()
            .map(|(slot, shortcut)| {
                let marked = conflicted || recording.is_some_and(|r| r.slot == slot);
                key_slot(command, slot, shortcut, marked, cx).into_any_element()
            })
            .collect();
        // Shown only on the row under the pointer, unless there is no shortcut at all.
        let add = (keys.len() < MOST && recording.is_none()).then(|| {
            ui::icon_button(
                SharedString::from(format!("add-{command:?}")),
                PLUS,
                px(26.),
                theme::muted(),
            )
            .when(!keys.is_empty(), |b| {
                b.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .tooltip(ui::tip(if keys.is_empty() {
                "Add a shortcut"
            } else {
                "Add a second shortcut"
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                let slot = cx.global::<Shortcuts>().keys(command).len();
                this.record(command, slot, cx);
            }))
        });
        let reset = (!shortcuts.is_default(command)).then(|| {
            let was = Shortcuts::default()
                .keys(command)
                .iter()
                .map(|s| s.label().to_string())
                .collect::<Vec<_>>()
                .join(" or ");
            ui::icon_button(
                SharedString::from(format!("reset-{command:?}")),
                RESET,
                px(26.),
                theme::muted(),
            )
            .tooltip(ui::tip(format!("Reset to {was}")))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.stop_recording();
                this.change_shortcuts(cx, |s| s.reset(command));
            }))
        });
        div()
            .id(group.clone())
            .group(group)
            .when(recording.is_some() || conflicted, |d| d.bg(theme::hover()))
            .child(
                row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(div().min_w_0().truncate().child(command.label()))
                            .when(!shortcuts.is_default(command), |d| {
                                d.child(div().flex_none().text_color(theme::accent()).child("•"))
                            }),
                    )
                    .children(add)
                    // Two shortcuts stack, rather than wrap, in a narrow column.
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(px(4.))
                            .children(slots),
                    )
                    .child(div().flex_none().w(px(26.)).children(reset)),
            )
            .children(recording.map(|r| self.recording_line(r, keys.len(), cx)))
    }

    /// The lines under a row whose shortcut is being chosen: the keys held so far, a way to remove
    /// the shortcut instead, and how to cancel or why the last key cannot be used.
    fn recording_line(&self, recording: Recording, count: usize, cx: &mut Context<Self>) -> Div {
        let Recording {
            command,
            slot,
            refused,
            ..
        } = recording;
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .pl(px(14.))
            .pr(px(8.))
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(listening(recording))
                    .child(div().flex_1())
                    .when(slot < count, |d| {
                        d.child(
                            small_button("remove-shortcut", "Remove").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.recording = None;
                                    this.change_shortcuts(cx, |s| s.remove(command, slot));
                                },
                            )),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(if refused.is_some() {
                        theme::attack()
                    } else {
                        theme::faint()
                    })
                    .child(refused.map_or("Esc cancels.", Refused::why)),
            )
    }
}

/// The prompt for a shortcut being chosen, showing the modifiers held so far.
fn listening(recording: Recording) -> Div {
    let held = recording.held;
    let mut parts: Vec<&str> = [
        (held.control, "Ctrl"),
        (held.alt, "Alt"),
        (held.shift, "Shift"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let prompt = if parts.is_empty() {
        "Press a shortcut…".to_owned()
    } else {
        parts.push("…");
        parts.join(" + ")
    };
    div()
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .border_1()
        .border_color(theme::accent())
        .bg(theme::alpha(theme::accent(), 0.12))
        .text_size(px(12.5))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::accent())
        .child(prompt)
}

/// One of a command's shortcuts, as key caps; clicked, it waits for new keys.
fn key_slot(
    command: Command,
    slot: usize,
    shortcut: &Shortcut,
    marked: bool,
    cx: &mut Context<Browser>,
) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("key-{command:?}-{slot}")))
        .flex()
        .items_center()
        .gap(px(4.))
        .p(px(3.))
        .rounded(px(7.))
        .border_1()
        .border_color(if marked {
            theme::accent().into()
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .hover(|s| s.border_color(theme::line_strong()).bg(theme::panel()))
        .children(shortcut.caps().into_iter().map(cap))
        .tooltip(ui::tip("Click to change"))
        .on_click(cx.listener(move |this, _, _, cx| this.record(command, slot, cx)))
}

/// A key cap in the Keyboard tab, larger than the hints along the panel's foot.
fn cap(text: SharedString) -> Div {
    div()
        .min_w(px(24.))
        .h(px(22.))
        .px(px(7.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .border_1()
        .border_b_2()
        .border_color(theme::line_hover())
        .bg(theme::panel())
        .font_weight(FontWeight::BOLD)
        .text_size(px(12.))
        .child(text)
}

/// A row for keys that cannot be changed.
fn fixed(label: &'static str, keys: &[&'static str]) -> AnyElement {
    row()
        .child(
            div()
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted())
                .child(label),
        )
        .child(
            div()
                .flex()
                .gap(px(4.))
                .p(px(3.))
                .children(keys.iter().map(|k| cap((*k).into()))),
        )
        .child(
            div()
                .w(px(26.))
                .flex()
                .justify_center()
                .text_size(px(11.))
                .text_color(theme::faint())
                .child("fixed"),
        )
        .into_any_element()
}

/// A titled card of shortcuts.
fn section(title: &str, rows: Vec<AnyElement>) -> Div {
    let rows = rows.into_iter().enumerate().map(|(i, row)| {
        div()
            .when(i > 0, |d| d.border_t_1().border_color(theme::line()))
            .child(row)
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(ui::label(title).px(px(2.)).font_weight(FontWeight::BOLD))
        .child(
            div()
                .rounded(px(12.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .overflow_hidden()
                .children(rows),
        )
}

fn column() -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(14.))
        .mt(px(14.))
}

fn row() -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(42.))
        .pl(px(14.))
        .pr(px(8.))
        .py(px(6.))
}

fn small_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(26.))
        .px(px(8.))
        .flex()
        .items_center()
        .rounded(px(6.))
        .text_size(px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::panel()).text_color(theme::text()))
        .child(label)
}

fn accent_button(id: &'static str, label: String) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(32.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(8.))
        .bg(theme::accent())
        .text_color(gpui::rgb(theme::DARK.text))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(13.))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(label)
}

#[cfg(test)]
mod tests {
    // The shortcut rules themselves are tested in crate::shortcuts; this tab draws them.
    use crate::shortcuts::{Command, Group};

    #[test]
    fn every_group_lists_something() {
        for group in Group::ALL {
            assert!(Command::ALL.into_iter().any(|c| c.group() == group));
        }
    }

    #[test]
    fn labels_are_distinct() {
        let mut labels: Vec<_> = Command::ALL.into_iter().map(Command::label).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), Command::ALL.len());
    }
}
