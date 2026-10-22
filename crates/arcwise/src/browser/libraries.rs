//! The settings' list of libraries, in the General tab: which are shown, adding and removing them,
//! and keeping a library on the web on disk to use offline.

use arcwise_core::MapId;
use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px, relative, svg,
};

use super::settings_window::{described, group, setting, text_button};
use super::{AddLibraryFolder, Browser, SetLibraries};
use crate::downloads::{OnDisk, Progress};
use crate::settings::LibrarySource;
use crate::sources::{self, State, Status};
use crate::{remote, theme, ui};

const CHECK: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 12.5l4.5 4.5L19 7.5" fill="none" stroke="#000" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

impl Browser {
    /// The Libraries group of the General tab.
    pub(super) fn libraries(&self, cx: &mut Context<Self>) -> Div {
        let mut rows: Vec<AnyElement> = self
            .library_list
            .iter()
            .enumerate()
            .map(|(i, source)| self.library_row(i, source, cx))
            .collect();
        rows.push(
            setting()
                .child(described(
                    "Add a library",
                    if self.paste_hint {
                        "There is no web address on the clipboard. Copy a library's address, then paste it."
                    } else {
                        "A folder that holds library.json, or a library's web address copied to the clipboard"
                    },
                ))
                .child(ui::button("add-library-folder", "Folder…").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.close_settings(cx);
                        cx.emit(AddLibraryFolder);
                    },
                )))
                .child(
                    ui::button("add-library-url", "Paste address")
                        .tooltip(ui::tip("Add the library whose web address is on the clipboard"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let url = cx
                                .read_from_clipboard()
                                .and_then(|item| item.text())
                                .map(|text| text.trim().to_owned())
                                .filter(|text| remote::is_url(text) && !text.contains(char::is_whitespace));
                            this.paste_hint = url.is_none();
                            let Some(url) = url else {
                                cx.notify();
                                return;
                            };
                            let mut list = this.library_list.clone();
                            match list.iter_mut().find(|l| remote::same_mirror(&l.location, &url)) {
                                Some(known) => known.on = true,
                                None => list.push(LibrarySource::new(remote::base(&url))),
                            }
                            this.set_libraries(list, true, cx);
                        })),
                )
                .into_any_element(),
        );
        group("Libraries", rows)
    }

    /// Keeps a changed list for the settings to show at once, and has it saved; `reload` reads the
    /// libraries again, when which lineups show has changed.
    fn set_libraries(&mut self, list: Vec<LibrarySource>, reload: bool, cx: &mut Context<Self>) {
        self.library_list = list.clone();
        cx.emit(SetLibraries { list, reload });
        cx.notify();
    }

    fn status(&self, location: &str) -> Option<&Status> {
        self.library_statuses
            .iter()
            .find(|s| s.location == location)
    }

    fn library_row(&self, i: usize, source: &LibrarySource, cx: &mut Context<Self>) -> AnyElement {
        let status = self.status(&source.location);
        let title = status.map_or_else(|| sources::title(&source.location), Status::title);
        let line = match status.map(|s| &s.state) {
            None if source.on => "Not read yet".to_owned(),
            None | Some(State::Off) => "Off".to_owned(),
            Some(State::Loaded {
                lineups,
                hidden,
                maps,
                stale,
                ..
            }) => loaded_line(*lineups, *hidden, maps.len(), *stale),
            Some(State::Failed(problem)) => capitalized(problem),
        };
        let failed = matches!(status.map(|s| &s.state), Some(State::Failed(_)));
        let web = remote::is_url(&source.location);
        let loaded = matches!(status.map(|s| &s.state), Some(State::Loaded { .. }));
        let expanded = self.offline_open.as_deref() == Some(source.location.as_str());
        let builtin = remote::base(&source.location) == remote::DEFAULT;
        let on = source.on;
        let row = setting()
            .child(
                div()
                    .id(SharedString::from(format!("library-on-{i}")))
                    .flex_none()
                    .cursor_pointer()
                    .child(check(on))
                    .tooltip(ui::tip(if on {
                        "Hide its lineups"
                    } else {
                        "Show its lineups"
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut list = this.library_list.clone();
                        list[i].on = !on;
                        this.set_libraries(list, true, cx);
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(title),
                    )
                    .child(
                        div()
                            .font_family(theme::MONO_FONT)
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .truncate()
                            .child(source.location.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if failed {
                                theme::attack()
                            } else {
                                theme::faint()
                            })
                            .child(line),
                    ),
            )
            .when(web && loaded && on, |row| {
                row.child(
                    text_button(
                        SharedString::from(format!("library-offline-{i}")),
                        if expanded { "Done" } else { "Offline…" },
                    )
                    .on_click(cx.listener({
                        let location = source.location.clone();
                        move |this, _, _, cx| {
                            this.offline_open = (!expanded).then(|| location.clone());
                            if !expanded {
                                this.downloads.update(cx, |d, cx| d.measure(&location, cx));
                            }
                            cx.notify();
                        }
                    })),
                )
            })
            .when(!builtin, |row| {
                row.child(
                    text_button(SharedString::from(format!("library-remove-{i}")), "Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let mut list = this.library_list.clone();
                            let removed = list.remove(i);
                            if this.offline_open.as_ref() == Some(&removed.location) {
                                this.offline_open = None;
                            }
                            let shared = list
                                .iter()
                                .any(|l| remote::same_mirror(&l.location, &removed.location));
                            this.downloads
                                .update(cx, |d, cx| d.forget(&removed.location, shared, cx));
                            this.set_libraries(list, true, cx);
                        })),
                )
            });
        let mut column = div().flex().flex_col().child(row);
        if expanded
            && let Some(Status {
                state: State::Loaded { maps, .. },
                ..
            }) = status
        {
            column = column.child(self.offline(i, source, maps, cx));
        }
        column.into_any_element()
    }

    /// The maps of a library on the web, each kept on disk or not, and the download.
    fn offline(
        &self,
        i: usize,
        source: &LibrarySource,
        maps: &[(MapId, String, usize)],
        cx: &mut Context<Self>,
    ) -> Div {
        let downloads = self.downloads.read(cx);
        let on_disk = downloads
            .on_disk(&source.location)
            .cloned()
            .unwrap_or_default();
        let progress = downloads.progress(&source.location);
        let all = source.keep_all;
        let location = source.location.clone();
        let mut panel = div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .mx(px(16.))
            .mb(px(12.))
            .p(px(8.))
            .rounded(px(8.))
            .bg(theme::panel())
            .child(
                div()
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(12.5))
                    .text_color(theme::muted())
                    .child("Lineups on the maps kept here play without a connection. The rest stream when shown."),
            )
            .child(keep_row(
                "keep-all".into(),
                all,
                "All maps".into(),
                "Including maps the library adds later".into(),
                total(&on_disk),
            ).on_click(cx.listener({
                let location = location.clone();
                move |this, _, _, cx| {
                    let mut list = this.library_list.clone();
                    list[i].keep_all = !all;
                    let kept = list[i].keep.clone();
                    this.downloads.update(cx, |d, cx| {
                        if all {
                            let dropped = this.library_maps(&location).into_iter().filter(|m| !kept.contains(m)).collect();
                            d.remove(&location, dropped, kept, cx);
                        } else {
                            d.start(&location, None, cx);
                        }
                    });
                    this.set_libraries(list, false, cx);
                }
            })));
        for (map, name, lineups) in maps {
            let kept = all || source.keep.contains(map);
            let sizes = on_disk.get(map).copied().unwrap_or_default();
            panel = panel.child(
                keep_row(
                    format!("keep-{map}").into(),
                    kept,
                    name.clone().into(),
                    plural(*lineups, "lineup").into(),
                    Some(sizes),
                )
                .when(all, |row| row.opacity(0.6))
                .on_click(cx.listener({
                    let location = location.clone();
                    let map = map.clone();
                    move |this, _, _, cx| {
                        if all {
                            return;
                        }
                        let mut list = this.library_list.clone();
                        let keep = &mut list[i].keep;
                        if kept {
                            keep.retain(|m| *m != map);
                            let keep = keep.clone();
                            this.downloads.update(cx, |d, cx| {
                                d.remove(&location, vec![map.clone()], keep, cx)
                            });
                        } else {
                            keep.push(map.clone());
                            let keep = keep.clone();
                            this.downloads
                                .update(cx, |d, cx| d.start(&location, Some(keep), cx));
                        }
                        this.set_libraries(list, false, cx);
                    }
                })),
            );
        }
        panel.children(progress.filter(|p| !p.finished || p.failed > 0).map(|p| {
            download_line(p).child(text_button("stop-download", "Stop").on_click(cx.listener(
                move |this, _, _, cx| {
                    let location = location.clone();
                    this.downloads.update(cx, |d, cx| d.stop(&location, cx));
                },
            )))
        }))
    }

    /// The ids of the maps a library on the web has lineups on.
    fn library_maps(&self, location: &str) -> Vec<MapId> {
        match self.status(location).map(|s| &s.state) {
            Some(State::Loaded { maps, .. }) => maps.iter().map(|(id, _, _)| id.clone()).collect(),
            _ => Vec::new(),
        }
    }
}

/// A row of the offline panel: a box to tick, what it is, and how much of it is on disk.
fn keep_row(
    id: SharedString,
    on: bool,
    title: SharedString,
    about: SharedString,
    sizes: Option<OnDisk>,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(8.))
        .py(px(6.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(|s| s.bg(theme::hover()))
        .child(check(on))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .gap(px(8.))
                .items_baseline()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::faint())
                        .child(about),
                ),
        )
        .children(sizes.filter(|s| s.total > 0).map(|s| {
            div()
                .flex_none()
                .text_size(px(12.))
                .text_color(if s.have == s.total {
                    theme::text()
                } else {
                    theme::faint()
                })
                .child(if s.have == s.total {
                    format!("{} on disk", size(s.total))
                } else if s.have == 0 {
                    size(s.total)
                } else {
                    format!("{} of {}", size(s.have), size(s.total))
                })
        }))
}

/// The bar and words of a download running, or of one that ended with files missing.
fn download_line(p: Progress) -> Div {
    let fraction = if p.total == 0 {
        1.
    } else {
        p.done as f32 / p.total as f32
    };
    let words = if p.finished {
        format!(
            "{} could not be downloaded; try again later",
            plural(p.failed, "file")
        )
    } else {
        format!("Downloading {} of {}", size(p.done), size(p.total))
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(8.))
        .pt(px(6.))
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child(words),
                )
                .child(
                    div().h(px(4.)).rounded_full().bg(theme::line()).child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(theme::accent())
                            .w(relative(fraction.clamp(0., 1.))),
                    ),
                ),
        )
}

/// A box to tick.
fn check(on: bool) -> Div {
    div()
        .flex_none()
        .size(px(18.))
        .rounded(px(5.))
        .border_1()
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| d.bg(theme::accent()).border_color(theme::accent()))
        .when(!on, |d| {
            d.bg(theme::panel()).border_color(theme::line_hover())
        })
        .when(on, |d| {
            d.child(
                svg()
                    .data(CHECK)
                    .size(px(13.))
                    .text_color(gpui::rgb(theme::DARK.text)),
            )
        })
}

fn total(sizes: &std::collections::HashMap<MapId, OnDisk>) -> Option<OnDisk> {
    let total = sizes.values().fold(OnDisk::default(), |a, s| OnDisk {
        have: a.have + s.have,
        total: a.total + s.total,
    });
    (total.total > 0).then_some(total)
}

fn size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1. {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", (bytes as f64 / 1e6).max(1.))
    }
}

/// What the settings say of a library that was read.
fn loaded_line(lineups: usize, hidden: usize, maps: usize, stale: bool) -> String {
    let mut line = format!("{} · {}", plural(lineups, "lineup"), plural(maps, "map"));
    if hidden > 0 {
        line.push_str(&format!(
            " · {hidden} hidden, since a library above has the same {}",
            if hidden == 1 { "one" } else { "ones" }
        ));
    }
    if stale {
        line.push_str(" · could not reach it, so showing the copy from last time");
    }
    line
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn plural(n: usize, word: &str) -> String {
    let n_text = n.to_string();
    let grouped = n_text
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(",");
    format!("{grouped} {word}{}", if n == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_counts_read_well() {
        assert_eq!(size(2_940_000_000), "2.9 GB");
        assert_eq!(size(48_000_000), "48 MB");
        assert_eq!(size(10), "1 MB");
        assert_eq!(plural(3752, "lineup"), "3,752 lineups");
        assert_eq!(plural(1, "map"), "1 map");
    }

    #[test]
    fn a_library_says_how_many_of_its_lineups_are_hidden() {
        assert_eq!(loaded_line(120, 0, 3, false), "120 lineups · 3 maps");
        assert_eq!(
            loaded_line(120, 3, 3, false),
            "120 lineups · 3 maps · 3 hidden, since a library above has the same ones"
        );
        assert_eq!(
            loaded_line(1, 1, 1, true),
            "1 lineup · 1 map · 1 hidden, since a library above has the same one · could not reach it, so showing the copy from last time"
        );
    }
}
