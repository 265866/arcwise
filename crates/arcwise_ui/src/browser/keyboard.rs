//! The settings window's Keyboard tab, where shortcuts are changed: click one, then press the keys
//! for it.

use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, InteractiveElement, IntoElement, KeyUpEvent,
    Keystroke, Modifiers, ModifiersChangedEvent, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, Window,
};

use super::browser_view::BrowserView;
use super::events::SetShortcuts;
use super::settings_window::WIDTH;
use super::tab::Tab;
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

/// The key press that last ended choosing a shortcut, and when it last came: Windows repeats a
/// held key, and the one that ended recording does nothing more while it is held, so holding Esc
/// to cancel does not also close the settings. Repeats come every few tens of milliseconds, so a
/// pause means a new press, even if the release went to another window.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EndedBy {
    keystroke: Keystroke,
    at: Instant,
}

/// A shortcut chosen for one command that another already has, waiting for a yes or no.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Conflict {
    command: Command,
    slot: usize,
    shortcut: Shortcut,
    holder: Command,
}

impl BrowserView {
    /// Takes a key press while a shortcut is being chosen, before any binding can act on it, and
    /// only while the Keyboard tab shows, whichever way it went away. A key press says which
    /// modifiers are held, which a release seen elsewhere may not have; Windows leaves Shift out
    /// for a symbol typed with it. Esc on its own cancels, rather than being refused, and
    /// Windows' own keys, such as Alt+F4, still do what Windows does with them. A modifier on its
    /// own waits for the key.
    pub(super) fn recorded(&mut self, keystroke: &Keystroke, cx: &mut Context<'_, Self>) {
        let Some(mut recording) = self.recording else {
            match &mut self.ended_by {
                Some(ended)
                    if ended.keystroke == *keystroke && ended.at.elapsed() < REPEATS_WITHIN =>
                {
                    ended.at = Instant::now();
                    cx.stop_propagation();
                }
                _ => self.ended_by = None,
            }
            return;
        };
        if !self.settings_open || self.settings_tab != Tab::Keyboard {
            self.stop_recording();
            cx.notify();
            return;
        }
        recording.held = keystroke.modifiers;
        if shortcuts::shifted_key(&keystroke.key).is_some() {
            recording.held.shift = true;
        }
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            cx.stop_propagation();
            self.ended_by = Some(EndedBy {
                keystroke: keystroke.clone(),
                at: Instant::now(),
            });
            self.recording = None;
            cx.notify();
            return;
        }
        let pressed = Shortcut::pressed(keystroke);
        if !matches!(pressed, Some(Err(Refused::Windows))) {
            cx.stop_propagation();
        }
        match pressed {
            None => self.recording = Some(recording),
            Some(Err(refused)) => {
                recording.refused = Some(refused);
                self.recording = Some(recording);
            }
            Some(Ok(shortcut)) => {
                self.ended_by = Some(EndedBy {
                    keystroke: keystroke.clone(),
                    at: Instant::now(),
                });
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
                        Self::change_shortcuts(cx, |s| s.set(command, slot, shortcut));
                    }
                }
            }
        }
        cx.notify();
    }

    /// Forgets the modifiers held and the key that ended recording when the window gains or loses
    /// focus: their releases may have gone to another window, as with Alt+Tab.
    pub(super) fn activation_changed(&mut self, cx: &mut Context<'_, Self>) {
        self.ended_by = None;
        if let Some(recording) = &mut self.recording {
            recording.held = Modifiers::default();
            cx.notify();
        }
    }

    pub(super) fn key_released(
        &mut self,
        _: &KeyUpEvent,
        _: &mut Window,
        _: &mut Context<'_, Self>,
    ) {
        self.ended_by = None;
    }

    pub(super) fn modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _: &mut Window,
        cx: &mut Context<'_, Self>,
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

    /// Changes the shortcuts with `change`, binds them once this key press is over, rather than
    /// while GPUI is dispatching it, and has them saved.
    fn change_shortcuts(cx: &mut Context<'_, Self>, change: impl FnOnce(&mut Shortcuts)) {
        let mut shortcuts = cx.global::<Shortcuts>().clone();
        change(&mut shortcuts);
        let changes = shortcuts.changes();
        cx.defer(move |cx| shortcuts::apply(shortcuts, cx));
        cx.emit(SetShortcuts(changes));
        cx.notify();
    }

    fn record(&mut self, command: Command, slot: usize, cx: &mut Context<'_, Self>) {
        self.conflict = None;
        self.recording = Some(Recording {
            command,
            slot,
            held: Modifiers::default(),
            refused: None,
        });
        cx.notify();
    }

    /// The Keyboard tab: two columns while the settings window has its full width, inside its 24px
    /// margins.
    pub(super) fn keyboard(&self, window: &Window, cx: &mut Context<'_, Self>) -> AnyElement {
        let shortcuts = cx.global::<Shortcuts>().clone();
        let group = |g: Group, cx: &mut Context<'_, Self>| {
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
        let two = window.viewport_size().width >= WIDTH + gpui::px(48.);
        let columns = if two {
            gpui::div()
                .flex()
                .items_start()
                .gap(gpui::px(14.))
                .child(column().child(browse).child(window_group))
                .child(column().child(views).child(clip))
        } else {
            column()
                .child(browse)
                .child(views)
                .child(clip)
                .child(window_group)
        };
        gpui::div()
            .flex()
            .flex_col()
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(12.))
                    .mt(gpui::px(10.))
                    .child(
                        gpui::div()
                            .flex_1()
                            .text_size(gpui::px(12.5))
                            .text_color(theme::muted())
                            .child(
                                "Click a shortcut to change it. They work while Arcwise has focus.",
                            ),
                    )
                    .child(
                        ui::button("reset-shortcuts".into(), "Reset all")
                            .h(gpui::px(32.))
                            .px(gpui::px(12.))
                            .text_size(gpui::px(13.))
                            .when(shortcuts.all_default(), |b| b.opacity(0.45))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.stop_recording();
                                if !cx.global::<Shortcuts>().all_default() {
                                    Self::change_shortcuts(cx, |s| *s = Shortcuts::default());
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
    fn conflict_banner(&self, cx: &mut Context<'_, Self>) -> Option<Div> {
        let conflict = self.conflict.clone()?;
        let key = conflict.shortcut.label();
        Some(
            gpui::div()
                .flex()
                .items_center()
                .gap(gpui::px(10.))
                .mt(gpui::px(12.))
                .px(gpui::px(14.))
                .py(gpui::px(10.))
                .rounded(gpui::px(12.))
                .border_1()
                .border_color(theme::accent())
                .bg(theme::alpha(theme::accent().into(), 0.08))
                .text_size(gpui::px(13.))
                .child(gpui::div().flex_1().child(format!(
                    "{key} already does {}. Use it for {} instead?",
                    conflict.holder.label(),
                    conflict.command.label()
                )))
                .child(
                    ui::button("conflict-cancel".into(), "Cancel")
                        .h(gpui::px(32.))
                        .px(gpui::px(12.))
                        .text_size(gpui::px(13.))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.conflict = None;
                            cx.notify();
                        })),
                )
                .child(
                    accent_button("conflict-take", format!("Use {key} here")).on_click(
                        cx.listener(|this, _, _, cx| {
                            if let Some(c) = this.conflict.take() {
                                Self::change_shortcuts(cx, |s| {
                                    s.set(c.command, c.slot, c.shortcut);
                                });
                            }
                        }),
                    ),
                ),
        )
    }

    /// A command's row. The keys stay as they are while new ones are chosen, the one being
    /// replaced marked; the prompt has a line of its own below, so nothing in the row moves or
    /// wraps. The button that adds a shortcut shows only on the row under the pointer, unless there
    /// is no shortcut at all, and two shortcuts stack, rather than wrap, in a narrow column.
    fn command_row(
        &self,
        command: Command,
        shortcuts: &Shortcuts,
        cx: &mut Context<'_, Self>,
    ) -> Stateful<Div> {
        let keys = shortcuts.keys(command);
        let recording = self.recording.filter(|r| r.command == command);
        let conflicted = self
            .conflict
            .as_ref()
            .is_some_and(|c| c.command == command || c.holder == command);
        let group = SharedString::from(format!("row-{command:?}"));
        let slots: Vec<AnyElement> = keys
            .iter()
            .enumerate()
            .map(|(slot, shortcut)| {
                let marked = conflicted || recording.is_some_and(|r| r.slot == slot);
                key_slot(command, slot, shortcut, marked, cx).into_any_element()
            })
            .collect();
        let add = (keys.len() < MOST && recording.is_none()).then(|| {
            ui::icon_button(
                ElementId::Name(format!("add-{command:?}").into()),
                PLUS,
                gpui::px(26.),
                theme::muted().into(),
            )
            .when(!keys.is_empty(), |b| {
                b.opacity(0.).group_hover(group.clone(), |s| s.opacity(1.))
            })
            .tooltip(ui::tip(if keys.is_empty() {
                "Add a shortcut".into()
            } else {
                "Add a second shortcut".into()
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
                ElementId::Name(format!("reset-{command:?}").into()),
                RESET,
                gpui::px(26.),
                theme::muted().into(),
            )
            .tooltip(ui::tip(format!("Reset to {was}").into()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.stop_recording();
                Self::change_shortcuts(cx, |s| s.reset(command));
            }))
        });
        gpui::div()
            .id(group.clone())
            .group(group)
            .when(recording.is_some() || conflicted, |d| d.bg(theme::hover()))
            .child(
                row()
                    .child(
                        gpui::div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap(gpui::px(6.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(gpui::div().min_w_0().truncate().child(command.label()))
                            .when(!shortcuts.is_default(command), |d| {
                                d.child(
                                    gpui::div()
                                        .flex_none()
                                        .text_color(theme::accent())
                                        .child("•"),
                                )
                            }),
                    )
                    .children(add)
                    .child(
                        gpui::div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .gap(gpui::px(4.))
                            .children(slots),
                    )
                    .child(gpui::div().flex_none().w(gpui::px(26.)).children(reset)),
            )
            .children(recording.map(|r| Self::recording_line(r, keys.len(), cx)))
    }

    /// The lines under a row whose shortcut is being chosen: the keys held so far, a way to remove
    /// the shortcut instead, and how to cancel or why the last key cannot be used.
    fn recording_line(recording: Recording, count: usize, cx: &mut Context<'_, Self>) -> Div {
        let Recording {
            command,
            slot,
            refused,
            ..
        } = recording;
        gpui::div()
            .flex()
            .flex_col()
            .gap(gpui::px(4.))
            .pl(gpui::px(14.))
            .pr(gpui::px(8.))
            .pb(gpui::px(10.))
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(8.))
                    .child(listening(recording))
                    .child(gpui::div().flex_1())
                    .when(slot < count, |d| {
                        d.child(
                            small_button("remove-shortcut", "Remove").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.recording = None;
                                    Self::change_shortcuts(cx, |s| s.remove(command, slot));
                                },
                            )),
                        )
                    }),
            )
            .child(
                gpui::div()
                    .text_size(gpui::px(11.5))
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
    gpui::div()
        .h(gpui::px(28.))
        .px(gpui::px(10.))
        .flex()
        .items_center()
        .rounded(gpui::px(7.))
        .border_1()
        .border_color(theme::accent())
        .bg(theme::alpha(theme::accent().into(), 0.12))
        .text_size(gpui::px(12.5))
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
    cx: &mut Context<'_, BrowserView>,
) -> Stateful<Div> {
    gpui::div()
        .id(SharedString::from(format!("key-{command:?}-{slot}")))
        .flex()
        .items_center()
        .gap(gpui::px(4.))
        .p(gpui::px(3.))
        .rounded(gpui::px(7.))
        .border_1()
        .border_color(if marked {
            theme::accent().into()
        } else {
            gpui::transparent_black()
        })
        .cursor_pointer()
        .hover(|s| s.border_color(theme::line_strong()).bg(theme::panel()))
        .children(shortcut.caps().into_iter().map(cap))
        .tooltip(ui::tip("Click to change".into()))
        .on_click(cx.listener(move |this, _, _, cx| this.record(command, slot, cx)))
}

/// A key cap in the Keyboard tab, larger than the hints along the panel's foot.
fn cap(text: SharedString) -> Div {
    gpui::div()
        .min_w(gpui::px(24.))
        .h(gpui::px(22.))
        .px(gpui::px(7.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(gpui::px(6.))
        .border_1()
        .border_b_2()
        .border_color(theme::line_hover())
        .bg(theme::panel())
        .font_weight(FontWeight::BOLD)
        .text_size(gpui::px(12.))
        .child(text)
}

/// A row for keys that cannot be changed.
fn fixed(label: &'static str, keys: &[&'static str]) -> AnyElement {
    row()
        .child(
            gpui::div()
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::muted())
                .child(label),
        )
        .child(
            gpui::div()
                .flex()
                .gap(gpui::px(4.))
                .p(gpui::px(3.))
                .children(keys.iter().map(|k| cap((*k).into()))),
        )
        .child(
            gpui::div()
                .w(gpui::px(26.))
                .flex()
                .justify_center()
                .text_size(gpui::px(11.))
                .text_color(theme::faint())
                .child("fixed"),
        )
        .into_any_element()
}

/// A titled card of shortcuts.
fn section(title: &str, rows: Vec<AnyElement>) -> Div {
    let rows = rows.into_iter().enumerate().map(|(i, row)| {
        gpui::div()
            .when(i > 0, |d| d.border_t_1().border_color(theme::line()))
            .child(row)
    });
    gpui::div()
        .flex()
        .flex_col()
        .gap(gpui::px(8.))
        .child(
            ui::label(title)
                .px(gpui::px(2.))
                .font_weight(FontWeight::BOLD),
        )
        .child(
            gpui::div()
                .rounded(gpui::px(12.))
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .overflow_hidden()
                .children(rows),
        )
}

/// A column of [`section`]s.
fn column() -> Div {
    gpui::div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(gpui::px(14.))
        .mt(gpui::px(14.))
}

/// A row of a [`section`]: what it is, then its keys.
fn row() -> Div {
    gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(8.))
        .min_h(gpui::px(42.))
        .pl(gpui::px(14.))
        .pr(gpui::px(8.))
        .py(gpui::px(6.))
}

/// A small text button, under a row.
fn small_button(id: &'static str, label: &'static str) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .h(gpui::px(26.))
        .px(gpui::px(8.))
        .flex()
        .items_center()
        .rounded(gpui::px(6.))
        .text_size(gpui::px(12.))
        .text_color(theme::muted())
        .cursor_pointer()
        .hover(|s| s.bg(theme::panel()).text_color(theme::text()))
        .child(label)
}

/// A button in the accent color, for the answer that changes something.
fn accent_button(id: &'static str, label: String) -> Stateful<Div> {
    gpui::div()
        .id(id)
        .h(gpui::px(32.))
        .px(gpui::px(12.))
        .flex()
        .items_center()
        .rounded(gpui::px(8.))
        .bg(theme::accent())
        .text_color(gpui::rgb(theme::DARK.text))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(gpui::px(13.))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(label)
}

#[cfg(test)]
mod tests {
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
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), Command::ALL.len());
    }
}
