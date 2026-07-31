// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::date::humanize_date;
use crate::core::thread::{EmailThread, EmailThreadList, Flag};
use crate::ui::utils;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
};
use regex::{Regex, RegexBuilder};
use std::rc::Rc;

const MAX_SEARCH_LEN: usize = 256;
const REGEX_SIZE_LIMIT: usize = 1 << 16;

struct Row {
    depth: usize,
    thread: Rc<EmailThread>,
    status: crate::ui::markers::PatchStatus,
    superseded: bool,
}

/// Scrollable thread list sharing the same `ThreadList` as the `Maildir`.
/// Any structural change made to the maildir (insert/remove/invalidate) is
/// reflected here after calling `invalidate()`.
pub struct ThreadsView {
    threads: EmailThreadList,
    state: ListState,
    rows: Vec<Row>,
    unread_only: bool,
    search: Option<(String, Regex)>,
    sender_search: Option<(String, Regex)>,
    markers: bool,
    markers_cache: Rc<std::cell::RefCell<crate::ui::markers::MarkersCache>>,
}

impl ThreadsView {
    fn flatten(&mut self) {
        self.rows.clear();
        let mut max_versions = std::collections::HashMap::new();
        compute_max_versions(&self.threads.borrow(), &mut max_versions);
        let mut max_descendants = std::collections::HashMap::new();
        compute_max_descendant_versions(&self.threads.borrow(), &mut max_descendants);
        flatten_recursive(
            &self.threads.borrow(),
            0,
            &mut self.rows,
            self.markers,
            &mut self.markers_cache.borrow_mut(),
            None,
            &max_versions,
            &max_descendants,
        );
    }

    /// Build the view from a shared `ThreadList`.
    #[cfg(test)]
    pub fn new(threads: EmailThreadList) -> Self {
        Self::with_markers(
            threads,
            false,
            Rc::new(std::cell::RefCell::new(
                crate::ui::markers::MarkersCache::new(Vec::new()),
            )),
        )
    }

    pub fn with_markers(
        threads: EmailThreadList,
        markers: bool,
        markers_cache: Rc<std::cell::RefCell<crate::ui::markers::MarkersCache>>,
    ) -> Self {
        let mut view = Self {
            threads,
            state: ListState::default(),
            rows: Vec::new(),
            unread_only: false,
            search: None,
            sender_search: None,
            markers,
            markers_cache,
        };
        view.flatten();
        if !view.rows.is_empty() {
            view.state.select(Some(0));
        }
        view
    }

    pub fn prev_email(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }

        if let Some(i) = self.state.selected() {
            self.state.select(Some(i.saturating_sub(n)));
        }
    }

    pub fn next_email(&mut self, n: usize) {
        if self.rows.is_empty() {
            return;
        }

        if let Some(i) = self.state.selected() {
            let next = i.saturating_add(n).min(self.rows.len().saturating_sub(1));
            self.state.select(Some(next));
        }
    }

    pub fn first_email(&mut self) {
        if self.rows.is_empty() {
            return;
        }

        self.state.select(Some(0));
    }

    pub fn last_email(&mut self) {
        if self.rows.is_empty() {
            return;
        }

        self.state.select(Some(self.rows.len().saturating_sub(1)));
    }

    /// Return the currently highlighted thread, if any.
    pub fn selected(&self) -> Option<Rc<EmailThread>> {
        self.state
            .selected()
            .and_then(|i| self.rows.get(i))
            .map(|r| r.thread.clone())
    }

    /// Return the root thread of the currently highlighted email.
    /// Walks backwards from the selection to find the `depth == 0` entry.
    pub fn selected_root(&self) -> Option<Rc<EmailThread>> {
        let idx = self.state.selected()?;
        for i in (0..=idx).rev() {
            if self.rows[i].depth == 0 {
                return Some(self.rows[i].thread.clone());
            }
        }
        None
    }

    /// Re-flatten from the shared `ThreadList`, preserving the selection by
    /// message-id if still present, otherwise clamping to the new length.
    pub fn invalidate(&mut self) {
        let old_idx = self.state.selected().unwrap_or(0);
        let selected_id = self
            .rows
            .get(old_idx)
            .map(|r| r.thread.parent.message_id.clone());

        self.flatten();

        if self.unread_only {
            self.rows.retain(|r| r.thread.parent.is_unread());
        }

        if let Some((_, ref re)) = self.search {
            self.rows.retain(|r| re.is_match(&r.thread.parent.subject));
        }

        if let Some((_, ref re)) = self.sender_search {
            self.rows
                .retain(|r| re.is_match(&r.thread.parent.from.to_string()));
        }

        let new_idx = selected_id
            .and_then(|id| {
                self.rows
                    .iter()
                    .position(|r| r.thread.parent.message_id == id)
            })
            .unwrap_or_else(|| old_idx.min(self.rows.len().saturating_sub(1)));

        if self.rows.is_empty() {
            self.state.select(None);
        } else {
            self.state.select(Some(new_idx));
        }
    }

    /// Return whether the view is currently filtering to unread only.
    pub fn is_unread_only(&self) -> bool {
        self.unread_only
    }

    /// Toggle between unread-only and all emails.
    pub fn toggle_unread(&mut self) {
        self.unread_only = !self.unread_only;
        self.invalidate();
        // When toggling the filter, start from the top so the user sees all results.
        if !self.rows.is_empty() {
            self.state.select(Some(0));
        }
        *self.state.offset_mut() = 0;
    }

    /// Set or clear the subject search filter and refresh.
    pub fn set_search(&mut self, query: Option<&str>) {
        self.search = query.map(|q| {
            let q = if q.len() > MAX_SEARCH_LEN {
                &q[..MAX_SEARCH_LEN]
            } else {
                q
            };
            let re = RegexBuilder::new(q)
                .case_insensitive(true)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .unwrap_or_else(|_| {
                    RegexBuilder::new(&regex::escape(q))
                        .case_insensitive(true)
                        .build()
                        .unwrap()
                });
            (q.to_string(), re)
        });
        self.invalidate();
        if query.is_some() {
            if !self.rows.is_empty() {
                self.state.select(Some(0));
            }
            *self.state.offset_mut() = 0;
        } else {
            let idx = self.state.selected().unwrap_or(0);
            *self.state.offset_mut() = idx.saturating_sub(4);
        }
    }

    /// Return the current search query, if any.
    pub fn search(&self) -> Option<&str> {
        self.search.as_ref().map(|(q, _)| q.as_str())
    }

    /// Set or clear the sender search filter and refresh.
    pub fn set_sender_search(&mut self, query: Option<&str>) {
        self.sender_search = query.map(|q| {
            let q = if q.len() > MAX_SEARCH_LEN {
                &q[..MAX_SEARCH_LEN]
            } else {
                q
            };
            let re = RegexBuilder::new(q)
                .case_insensitive(true)
                .size_limit(REGEX_SIZE_LIMIT)
                .build()
                .unwrap_or_else(|_| {
                    RegexBuilder::new(&regex::escape(q))
                        .case_insensitive(true)
                        .build()
                        .unwrap()
                });
            (q.to_string(), re)
        });
        self.invalidate();
        if query.is_some() {
            if !self.rows.is_empty() {
                self.state.select(Some(0));
            }
            *self.state.offset_mut() = 0;
        } else {
            let idx = self.state.selected().unwrap_or(0);
            *self.state.offset_mut() = idx.saturating_sub(4);
        }
    }

    /// Return the current sender search query, if any.
    pub fn sender_search(&self) -> Option<&str> {
        self.sender_search.as_ref().map(|(q, _)| q.as_str())
    }

    /// Mark every visible email as read.
    pub fn mark_all_read(&self) {
        for row in &self.rows {
            if row.thread.parent.is_unread() {
                row.thread.parent.mark(Flag::Seen);
            }
        }
    }

    /// Toggle merged status for the currently selected root thread.
    pub fn toggle_merged(&mut self) {
        let Some(root) = self.selected_root() else {
            return;
        };

        let mut cache = self.markers_cache.borrow_mut();
        let current_status = cache.get_status(&root.parent);

        let new_status = if current_status == crate::ui::markers::PatchStatus::Merged {
            crate::ui::markers::PatchStatus::Normal
        } else {
            crate::ui::markers::PatchStatus::Merged
        };

        cache.set_status(&root.parent, new_status);
        drop(cache);
        self.invalidate();
    }
}

/// Build a single `ListItem` for a thread row.
fn build_row_item(row: &Row, subject_w: usize) -> ListItem<'static> {
    const FROM_W: usize = 27;
    const DATE_W: usize = 16;

    let e = &row.thread.parent;
    let from = utils::fit_string(e.from.short(), FROM_W);
    let indent = if row.depth == 0 {
        String::new()
    } else {
        format!("{}└ ", "  ".repeat(row.depth - 1))
    };
    let subject = if e.subject.is_empty() {
        "(no subject)".to_string()
    } else {
        e.subject.clone()
    };
    let subject_avail = subject_w.saturating_sub(indent.chars().count());
    let subject_padded = utils::fit_string(&subject, subject_avail);
    let mut text_style = Style::default();

    if e.is_unread() {
        text_style = text_style.fg(Color::Green).add_modifier(Modifier::BOLD);
    } else if e.has_mark(Flag::Flagged) {
        text_style = text_style.fg(Color::Red);
    } else if row.superseded {
        text_style = text_style.fg(Color::Rgb(86, 95, 137));
    } else if row.status == crate::ui::markers::PatchStatus::Merged {
        text_style = text_style.fg(Color::Rgb(86, 95, 137));
    } else if row.status == crate::ui::markers::PatchStatus::Reviewed {
        text_style = text_style.fg(Color::Yellow);
    }

    let flagged_span = if e.has_mark(Flag::Flagged) {
        Span::styled("★", Style::default().fg(Color::Yellow))
    } else {
        Span::raw(" ")
    };
    let replied_span = if e.has_mark(Flag::Replied) {
        Span::styled("↩", Style::default().fg(Color::Yellow))
    } else {
        Span::raw(" ")
    };
    let passed_span = if e.has_mark(Flag::Passed) {
        Span::styled("→ ", Style::default().fg(Color::Yellow))
    } else {
        Span::raw("  ")
    };
    let encrypted_span = if e.is_encrypted {
        Span::styled("⚷", Style::default().fg(Color::Magenta))
    } else {
        Span::raw(" ")
    };
    ListItem::new(Line::from(vec![
        Span::styled(from, text_style),
        Span::raw(" "),
        Span::styled(indent, Style::default().fg(Color::DarkGray)),
        Span::styled(subject_padded, text_style),
        Span::raw(" "),
        encrypted_span,
        flagged_span,
        replied_span,
        passed_span,
        Span::styled(
            format!("{:<DATE_W$}", humanize_date(e.timestamp)),
            Style::default().fg(Color::Cyan),
        ),
    ]))
}

/// Render the thread list into `area`.
pub fn draw(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, view: &mut ThreadsView) {
    const FROM_W: usize = 27;
    const DATE_W: usize = 16;
    const FLAGS_W: usize = 5; // "⚷★↩→ " etc.
    let usable = area.width.saturating_sub(2) as usize;
    let subject_w = usable.saturating_sub(FROM_W + DATE_W + FLAGS_W + 2);

    let total = view.rows.len();
    let height = area.height as usize;

    if total == 0 || height == 0 {
        let widget =
            List::new(Vec::<ListItem>::new()).block(Block::default().borders(Borders::NONE));
        frame.render_widget(widget, area);
        return;
    }

    let selected = view.state.selected().unwrap_or(0);

    // Adjust offset so the selected item is always visible.
    let mut offset = view.state.offset();
    if selected < offset {
        offset = selected;
    } else if selected >= offset + height {
        offset = selected - height + 1;
    }
    *view.state.offset_mut() = offset;

    let end = (offset + height).min(total);
    let items: Vec<ListItem> = view.rows[offset..end]
        .iter()
        .map(|row| build_row_item(row, subject_w))
        .collect();

    let mut visible_state = ListState::default();
    visible_state.select(Some(selected - offset));

    let widget = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    frame.render_stateful_widget(widget, area, &mut visible_state);
}

pub(crate) fn version_of(subject: &str) -> (u32, String) {
    let mut rest = subject.trim();
    let mut version = 1;
    let mut found = false;
    while let Some(inner) = rest.strip_prefix('[') {
        let Some(close) = inner.find(']') else {
            break;
        };
        if !found {
            if let Some(v) = inner[..close]
                .split(|c: char| c.is_whitespace() || c == ',')
                .find_map(|t| {
                    let digits = t.strip_prefix('v').or_else(|| t.strip_prefix('V'))?;
                    digits.parse::<u32>().ok()
                })
            {
                version = v;
                found = true;
            }
        }
        rest = inner[close + 1..].trim_start();
    }
    let title = rest.trim();
    let key = if title.is_empty() {
        subject.trim()
    } else {
        title
    };
    (
        version,
        key.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
    )
}

fn compute_max_versions(
    threads: &[Rc<EmailThread>],
    max_versions: &mut std::collections::HashMap<String, u32>,
) {
    for thread in threads {
        let (ver, key) = version_of(&thread.parent.subject);
        let entry = max_versions.entry(key).or_insert(0);
        if ver > *entry {
            *entry = ver;
        }
        compute_max_versions(&thread.replies.borrow(), max_versions);
    }
}

fn compute_max_descendant_versions(
    threads: &[Rc<EmailThread>],
    max_descendants: &mut std::collections::HashMap<String, u32>,
) {
    for thread in threads {
        compute_max_descendant_version_recursive(thread, max_descendants);
    }
}

fn compute_max_descendant_version_recursive(
    thread: &Rc<EmailThread>,
    max_descendants: &mut std::collections::HashMap<String, u32>,
) -> u32 {
    let mut max_ver = 0;
    let subject = thread.parent.subject.trim_start();
    if subject.starts_with('[') {
        let (ver, _) = version_of(subject);
        max_ver = ver;
    }

    for reply in thread.replies.borrow().iter() {
        let child_max = compute_max_descendant_version_recursive(reply, max_descendants);
        if child_max > max_ver {
            max_ver = child_max;
        }
    }
    max_descendants.insert(thread.parent.message_id.clone(), max_ver);
    max_ver
}

fn compute_thread_status(
    thread: &Rc<EmailThread>,
    cache: &mut crate::ui::markers::MarkersCache,
) -> crate::ui::markers::PatchStatus {
    let mut current_status = cache.get_status(&thread.parent);

    if current_status == crate::ui::markers::PatchStatus::Merged {
        return crate::ui::markers::PatchStatus::Merged;
    }

    for reply in thread.replies.borrow().iter() {
        let reply_status = compute_thread_status(reply, cache);
        if reply_status == crate::ui::markers::PatchStatus::Merged {
            return crate::ui::markers::PatchStatus::Merged;
        }
        if reply_status == crate::ui::markers::PatchStatus::Reviewed {
            current_status = crate::ui::markers::PatchStatus::Reviewed;
        }
    }
    current_status
}

fn flatten_recursive(
    threads: &[Rc<EmailThread>],
    depth: usize,
    out: &mut Vec<Row>,
    markers: bool,
    cache: &mut crate::ui::markers::MarkersCache,
    inherited_status: Option<crate::ui::markers::PatchStatus>,
    max_versions: &std::collections::HashMap<String, u32>,
    max_descendants: &std::collections::HashMap<String, u32>,
) {
    for thread in threads {
        let mut status = inherited_status.unwrap_or(crate::ui::markers::PatchStatus::Normal);
        if markers && inherited_status.is_none() {
            status = compute_thread_status(thread, cache);
        }
        let (ver, key) = version_of(&thread.parent.subject);
        let superseded_by_key = max_versions.get(&key).map(|&m| ver < m).unwrap_or(false);
        let superseded_by_descendant = max_descendants
            .get(&thread.parent.message_id)
            .map(|&m| ver > 0 && ver < m)
            .unwrap_or(false);
        let superseded = superseded_by_key || superseded_by_descendant;
        out.push(Row {
            depth,
            thread: thread.clone(),
            status,
            superseded,
        });
        let replies = thread.replies.borrow();
        flatten_recursive(
            &replies,
            depth + 1,
            out,
            markers,
            cache,
            Some(status),
            max_versions,
            max_descendants,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::thread::{Email, EmailThread, EmailThreadList};
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;

    fn tlist(threads: Vec<Rc<EmailThread>>) -> EmailThreadList {
        Rc::new(RefCell::new(threads))
    }

    fn make_email(id: &str, from: &str, subject: &str, unread: bool) -> Email {
        let path = if unread {
            PathBuf::from(format!("/mb/new/{}", id))
        } else {
            PathBuf::from(format!("/mb/cur/{}:2,S", id))
        };
        Email::new(id, None, from, subject, Some(1_700_000_000), path)
    }

    fn make_email_with_flags(id: &str, flags: &str) -> Email {
        Email::new(
            id,
            None,
            "sender",
            "Subject",
            Some(1_700_000_000),
            PathBuf::from(format!("/mb/cur/{}:2,{}", id, flags)),
        )
    }

    fn thread(id: &str, from: &str, subject: &str, unread: bool) -> Rc<EmailThread> {
        Rc::new(EmailThread {
            parent: make_email(id, from, subject, unread),
            replies: RefCell::new(Vec::new()),
        })
    }

    fn with_reply(parent: Rc<EmailThread>, child: Rc<EmailThread>) -> Rc<EmailThread> {
        parent.replies.borrow_mut().push(child);
        parent
    }

    fn rendered_lines(view: &mut ThreadsView, w: u16, h: u16) -> Vec<String> {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, ratatui::layout::Rect::new(0, 0, w, h), view);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        let area = buf.area();
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf.cell((x, y)).map_or(" ", |c| c.symbol()))
                    .collect()
            })
            .collect()
    }

    // ── new ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_empty_has_no_selection() {
        let view = ThreadsView::new(tlist(vec![]));
        assert!(view.selected().is_none());
        assert!(view.rows.is_empty());
    }

    #[test]
    fn new_selects_first_thread() {
        let view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Hello", false),
            thread("b", "Bob", "World", false),
        ]));
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn new_counts_all_rows_including_replies() {
        let parent = with_reply(
            thread("p", "Alice", "Parent", false),
            thread("c", "Bob", "Re: Parent", false),
        );
        let view = ThreadsView::new(tlist(vec![parent]));
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn new_flattens_replies_depth_first() {
        // Tree: A → [B → [C], D]
        let c = thread("c", "", "C", false);
        let b = with_reply(thread("b", "", "B", false), c);
        let d = thread("d", "", "D", false);
        let a = {
            let a = thread("a", "", "A", false);
            a.replies.borrow_mut().push(b);
            a.replies.borrow_mut().push(d);
            a
        };
        let view = ThreadsView::new(tlist(vec![a]));
        // Depth-first: A, B, C, D
        assert_eq!(view.rows.len(), 4);
        let ids: Vec<_> = view
            .rows
            .iter()
            .map(|r| r.thread.parent.message_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b", "c", "d"]);
    }

    #[test]
    fn new_assigns_correct_depths() {
        let child = thread("c", "", "Re", false);
        let parent = with_reply(thread("p", "", "Root", false), child);
        let view = ThreadsView::new(tlist(vec![parent]));
        assert_eq!(view.rows[0].depth, 0);
        assert_eq!(view.rows[1].depth, 1);
    }

    // ── selected_root ─────────────────────────────────────────────────────

    #[test]
    fn selected_root_returns_root_when_on_root() {
        let child = thread("c", "", "Re", false);
        let parent = with_reply(thread("p", "", "Root", false), child);
        let view = ThreadsView::new(tlist(vec![parent]));
        // selection is on "p" (depth 0)
        assert_eq!(view.selected_root().unwrap().parent.message_id, "p");
    }

    #[test]
    fn selected_root_returns_root_when_on_reply() {
        let child = thread("c", "", "Re", false);
        let parent = with_reply(thread("p", "", "Root", false), child);
        let mut view = ThreadsView::new(tlist(vec![parent]));
        view.next_email(1); // select "c" (depth 1)
        assert_eq!(view.selected().unwrap().parent.message_id, "c");
        assert_eq!(view.selected_root().unwrap().parent.message_id, "p");
    }

    #[test]
    fn selected_root_on_empty_returns_none() {
        let view = ThreadsView::new(tlist(vec![]));
        assert!(view.selected_root().is_none());
    }

    // ── invalidate ───────────────────────────────────────────────────────────

    #[test]
    fn invalidate_rebuilds_rows() {
        let list = tlist(vec![thread("a", "", "A", false)]);
        let mut view = ThreadsView::new(list.clone());
        assert_eq!(view.rows.len(), 1);
        list.borrow_mut().push(thread("b", "", "B", false));
        view.invalidate();
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn invalidate_preserves_selection_by_message_id() {
        let list = tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]);
        let mut view = ThreadsView::new(list.clone());
        view.next_email(1); // select "b"
        assert_eq!(view.selected().unwrap().parent.message_id, "b");

        // Refresh with same threads in a different order
        *list.borrow_mut() = vec![
            thread("c", "", "C", false),
            thread("b", "", "B", false),
            thread("a", "", "A", false),
        ];
        view.invalidate();
        assert_eq!(view.selected().unwrap().parent.message_id, "b");
    }

    #[test]
    fn invalidate_selects_same_index_when_thread_removed() {
        let list = tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
            thread("d", "", "D", false),
        ]);
        let mut view = ThreadsView::new(list.clone());
        view.next_email(1);
        view.next_email(1); // select index 2 ("c")

        // Remove "c" — the item now at index 2 is "d"
        *list.borrow_mut() = vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("d", "", "D", false),
        ];
        view.invalidate();
        assert_eq!(view.selected().unwrap().parent.message_id, "d");
    }

    #[test]
    fn invalidate_clamps_index_when_list_shrinks() {
        let list = tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]);
        let mut view = ThreadsView::new(list.clone());
        view.next_email(1);
        view.next_email(1); // select index 2 ("c")

        // Shrink to one item — index 2 doesn't exist, clamp to 0
        *list.borrow_mut() = vec![thread("a", "", "A", false)];
        view.invalidate();
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn invalidate_to_empty_clears_selection() {
        let list = tlist(vec![thread("a", "", "A", false)]);
        let mut view = ThreadsView::new(list.clone());
        list.borrow_mut().clear();
        view.invalidate();
        assert!(view.selected().is_none());
    }

    #[test]
    fn invalidate_from_empty_to_nonempty_selects_first() {
        let list = tlist(vec![]);
        let mut view = ThreadsView::new(list.clone());
        list.borrow_mut().push(thread("a", "", "A", false));
        view.invalidate();
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    // ── prev / next ───────────────────────────────────────────────────────────

    #[test]
    fn prev_at_first_stays() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
        ]));
        view.prev_email(1);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn next_moves_to_second() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
        ]));
        view.next_email(1);
        assert_eq!(view.selected().unwrap().parent.message_id, "b");
    }

    #[test]
    fn next_at_last_stays() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "", "A", false)]));
        view.next_email(1);
        view.next_email(1);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn prev_after_next_returns_to_first() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
        ]));
        view.next_email(1);
        view.prev_email(1);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn next_on_empty_does_not_panic() {
        let mut view = ThreadsView::new(tlist(vec![]));
        view.next_email(1);
        assert!(view.selected().is_none());
    }

    #[test]
    fn prev_on_empty_does_not_panic() {
        let mut view = ThreadsView::new(tlist(vec![]));
        view.prev_email(1);
        assert!(view.selected().is_none());
    }

    #[test]
    fn next_skips_multiple_steps() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
            thread("d", "", "D", false),
            thread("e", "", "E", false),
        ]));
        view.next_email(3);
        assert_eq!(view.selected().unwrap().parent.message_id, "d");
    }

    #[test]
    fn prev_skips_multiple_steps() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
            thread("d", "", "D", false),
            thread("e", "", "E", false),
        ]));
        view.next_email(4);
        view.prev_email(3);
        assert_eq!(view.selected().unwrap().parent.message_id, "b");
    }

    #[test]
    fn next_clamps_at_last() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]));
        view.next_email(100);
        assert_eq!(view.selected().unwrap().parent.message_id, "c");
    }

    #[test]
    fn prev_clamps_at_first() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]));
        view.next_email(2);
        view.prev_email(100);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    // ── first / last email ─────────────────────────────────────────────────

    #[test]
    fn first_email_selects_first() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]));
        view.next_email(1);
        view.next_email(1); // select "c"
        view.first_email();
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn last_email_selects_last() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "", "A", false),
            thread("b", "", "B", false),
            thread("c", "", "C", false),
        ]));
        view.last_email();
        assert_eq!(view.selected().unwrap().parent.message_id, "c");
    }

    #[test]
    fn first_email_on_empty_does_not_panic() {
        let mut view = ThreadsView::new(tlist(vec![]));
        view.first_email();
        assert!(view.selected().is_none());
    }

    #[test]
    fn last_email_on_empty_does_not_panic() {
        let mut view = ThreadsView::new(tlist(vec![]));
        view.last_email();
        assert!(view.selected().is_none());
    }

    // ── show_unread / show_all / toggle_unread ─────────────────────────────

    #[test]
    fn show_unread_filters_read_emails() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Unread", true),
            thread("c", "Carol", "Also read", false),
        ]));
        view.toggle_unread();
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.selected().unwrap().parent.message_id, "b");
    }

    #[test]
    fn show_all_restores_all_emails() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Unread", true),
        ]));
        view.toggle_unread();
        assert_eq!(view.rows.len(), 1);
        view.toggle_unread();
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn toggle_unread_switches_mode() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Unread", true),
        ]));
        view.toggle_unread();
        assert_eq!(view.rows.len(), 1);
        view.toggle_unread();
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn show_unread_resets_to_first_after_toggle() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Unread", true),
            thread("c", "Carol", "Also unread", true),
        ]));
        view.next_email(1);
        view.next_email(1); // select "c"
        view.toggle_unread();
        // toggle always resets to first item so user sees all results
        assert_eq!(view.selected().unwrap().parent.message_id, "b");
    }

    #[test]
    fn show_unread_selects_first_when_selected_is_read() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Also read", false),
            thread("c", "Carol", "Unread", true),
        ]));
        // select "a" (read) — after filtering, selects first unread
        view.toggle_unread();
        assert_eq!(view.selected().unwrap().parent.message_id, "c");
    }

    #[test]
    fn show_unread_with_no_unread_clears_selection() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Also read", false),
        ]));
        view.toggle_unread();
        assert!(view.selected().is_none());
        assert!(view.rows.is_empty());
    }

    #[test]
    fn invalidate_respects_unread_filter() {
        let list = tlist(vec![
            thread("a", "Alice", "Read", false),
            thread("b", "Bob", "Unread", true),
        ]);
        let mut view = ThreadsView::new(list.clone());
        view.toggle_unread();
        assert_eq!(view.rows.len(), 1);

        list.borrow_mut()
            .push(thread("c", "Carol", "New unread", true));
        view.invalidate();
        assert_eq!(view.rows.len(), 2);
    }

    // ── search ──────────────────────────────────────────────────────────────

    #[test]
    fn set_search_filters_by_subject() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Rust is great", false),
            thread("b", "Bob", "Python news", false),
            thread("c", "Carol", "Rust update", false),
        ]));
        view.set_search(Some("rust"));
        assert_eq!(view.rows.len(), 2);
        let ids: Vec<_> = view
            .rows
            .iter()
            .map(|r| r.thread.parent.message_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "HELLO World", false),
            thread("b", "Bob", "goodbye", false),
        ]));
        view.set_search(Some("hello"));
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn clear_search_restores_all() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Rust", false),
            thread("b", "Bob", "Python", false),
        ]));
        view.set_search(Some("rust"));
        assert_eq!(view.rows.len(), 1);
        view.set_search(None);
        assert_eq!(view.rows.len(), 2);
    }

    #[test]
    fn search_returns_current_query() {
        let mut view = ThreadsView::new(tlist(vec![]));
        assert!(view.search().is_none());
        view.set_search(Some("test"));
        assert_eq!(view.search(), Some("test"));
        view.set_search(None);
        assert!(view.search().is_none());
    }

    #[test]
    fn search_combines_with_unread_filter() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Rust news", true),
            thread("b", "Bob", "Rust update", false),
            thread("c", "Carol", "Python news", true),
        ]));
        view.toggle_unread();
        view.set_search(Some("rust"));
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.selected().unwrap().parent.message_id, "a");
    }

    #[test]
    fn search_no_match_clears_selection() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "Alice", "Hello", false)]));
        view.set_search(Some("zzzzz"));
        assert!(view.rows.is_empty());
        assert!(view.selected().is_none());
    }

    #[test]
    fn search_resets_offset_after_scrolling() {
        let emails: Vec<_> = (0..50)
            .map(|i| {
                let id = format!("msg-{i}");
                let from = format!("user-{i}");
                let subject = if i % 3 == 0 {
                    format!("Rust topic {i}")
                } else {
                    format!("Other topic {i}")
                };
                thread(&id, &from, &subject, false)
            })
            .collect();
        let mut view = ThreadsView::new(tlist(emails));

        // Scroll far down the list
        view.next_email(45);
        // Render with a small viewport so offset advances past the top
        rendered_lines(&mut view, 80, 5);
        assert!(view.state.offset() > 0, "offset should have advanced");

        view.set_search(Some("rust"));
        assert_eq!(view.state.selected(), Some(0));
        assert_eq!(view.state.offset(), 0);
        // Verify all Rust matches are actually visible from the top
        let content = rendered_lines(&mut view, 80, 5).join("\n");
        assert!(
            content.contains("Rust topic 0"),
            "first match should be visible after search, got:\n{content}"
        );
    }

    #[test]
    fn search_regex_matches_pattern() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "[PATCH v3 2/7] nvme: add test", false),
            thread("b", "Bob", "[PATCH v2 1/3] nvme: update", false),
            thread("c", "Carol", "[PATCH v3 1/2] irq: fix bug", false),
        ]));
        view.set_search(Some("v3.*nvme"));
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].thread.parent.message_id, "a");
    }

    #[test]
    fn search_regex_alternation() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "nvme: add test", false),
            thread("b", "Bob", "irq: fix bug", false),
            thread("c", "Carol", "sched: update", false),
        ]));
        view.set_search(Some("nvme|irq"));
        assert_eq!(view.rows.len(), 2);
        let ids: Vec<_> = view
            .rows
            .iter()
            .map(|r| r.thread.parent.message_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"]);
    }

    #[test]
    fn search_invalid_regex_falls_back_to_literal() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "[PATCH 1/2] fix", false),
            thread("b", "Bob", "hello world", false),
        ]));
        view.set_search(Some("[PATCH"));
        assert_eq!(view.rows.len(), 1);
        assert_eq!(view.rows[0].thread.parent.message_id, "a");
    }

    // ── draw ─────────────────────────────────────────────────────────────────

    #[test]
    fn draw_shows_threads_content() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "Alice", "Hello", false)]));
        let content: String = rendered_lines(&mut view, 60, 5).join("\n");
        assert!(content.contains("Alice"), "got:\n{content}");
    }

    #[test]
    fn draw_shows_search_filters_results() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "Hello", false),
            thread("b", "Bob", "World", false),
        ]));
        view.set_search(Some("hello"));
        let content: String = rendered_lines(&mut view, 60, 5).join("\n");
        assert!(content.contains("Alice"), "got:\n{content}");
        assert!(!content.contains("Bob"), "got:\n{content}");
    }

    #[test]
    fn draw_renders_from_and_subject() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "Alice", "Hello world", false)]));
        let content: String = rendered_lines(&mut view, 80, 5).join("\n");
        assert!(content.contains("Alice"), "got:\n{content}");
        assert!(content.contains("Hello world"), "got:\n{content}");
    }

    #[test]
    fn draw_uses_no_subject_placeholder() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "Alice", "", false)]));
        let content: String = rendered_lines(&mut view, 80, 5).join("\n");
        assert!(content.contains("(no subject)"), "got:\n{content}");
    }

    #[test]
    fn draw_renders_multiple_threads() {
        let mut view = ThreadsView::new(tlist(vec![
            thread("a", "Alice", "First", false),
            thread("b", "Bob", "Second", false),
        ]));
        let content: String = rendered_lines(&mut view, 80, 6).join("\n");
        assert!(content.contains("Alice"), "got:\n{content}");
        assert!(content.contains("Bob"), "got:\n{content}");
        assert!(content.contains("First"), "got:\n{content}");
        assert!(content.contains("Second"), "got:\n{content}");
    }

    #[test]
    fn draw_shows_replied_indicator() {
        let email = make_email_with_flags("a", "RS");
        let t = Rc::new(EmailThread {
            parent: email,
            replies: RefCell::new(vec![]),
        });
        let mut view = ThreadsView::new(tlist(vec![t]));
        let content: String = rendered_lines(&mut view, 80, 3).join("\n");
        assert!(
            content.contains('↩'),
            "replied indicator missing:\n{content}"
        );
    }

    #[test]
    fn draw_shows_passed_indicator() {
        let email = make_email_with_flags("a", "PS");
        let t = Rc::new(EmailThread {
            parent: email,
            replies: RefCell::new(vec![]),
        });
        let mut view = ThreadsView::new(tlist(vec![t]));
        let content: String = rendered_lines(&mut view, 80, 3).join("\n");
        assert!(
            content.contains('→'),
            "passed indicator missing:\n{content}"
        );
    }

    #[test]
    fn draw_shows_flagged_indicator() {
        let email = make_email_with_flags("a", "FS");
        let t = Rc::new(EmailThread {
            parent: email,
            replies: RefCell::new(vec![]),
        });
        let mut view = ThreadsView::new(tlist(vec![t]));
        let content: String = rendered_lines(&mut view, 80, 3).join("\n");
        assert!(
            content.contains('★'),
            "flagged indicator missing:\n{content}"
        );
    }

    #[test]
    fn mark_all_read_on_empty_does_not_panic() {
        let view = ThreadsView::new(tlist(vec![]));
        view.mark_all_read();
    }

    #[test]
    fn draw_shows_no_indicators_for_plain_email() {
        let mut view = ThreadsView::new(tlist(vec![thread("a", "Alice", "Hello", false)]));
        let content: String = rendered_lines(&mut view, 80, 3).join("\n");
        assert!(
            !content.contains('↩'),
            "unexpected replied indicator:\n{content}"
        );
        assert!(
            !content.contains('→'),
            "unexpected passed indicator:\n{content}"
        );
        assert!(
            !content.contains('★'),
            "unexpected flagged indicator:\n{content}"
        );
    }
}
