// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use super::compose::BODY_SENTINEL;
use crate::core::address::AddressBook;
use crate::core::attachment::Attachment;
use edtui::{
    EditorEventHandler, EditorMode, EditorState, EditorStatusLine, EditorTheme, EditorView,
    Highlight, Index2, Lines, RowIndex,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

/// Which part of the compose view is focused.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Focus {
    To,
    Cc,
    Subject,
    Body,
}

/// A simple single-line text input with cursor.
struct HeaderField {
    label: &'static str,
    value: String,
    cursor: usize,
}

impl HeaderField {
    fn new(label: &'static str, value: &str) -> Self {
        let cursor = value.len();
        Self {
            label,
            value: value.to_string(),
            cursor,
        }
    }

    fn on_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let prev = self.value[..self.cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.value.remove(prev);
                    self.cursor = prev;
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.value[..self.cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor += self.value[self.cursor..]
                        .chars()
                        .next()
                        .map(|c| c.len_utf8())
                        .unwrap_or(0);
                }
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.value.len(),
            _ => {}
        }
    }

    /// The query fragment for autocomplete: text after the last comma.
    fn current_fragment(&self) -> &str {
        self.value
            .rfind(',')
            .map(|i| self.value[i + 1..].trim_start())
            .unwrap_or(self.value.trim())
    }

    /// Replace the fragment being typed with `addr` and append `, ` for the next entry.
    fn accept_suggestion(&mut self, addr: &str) {
        if let Some(comma_pos) = self.value.rfind(',') {
            self.value.truncate(comma_pos + 1);
            self.value.push(' ');
        } else {
            self.value.clear();
        }
        self.value.push_str(addr);
        self.value.push_str(", ");
        self.cursor = self.value.len();
    }
}

/// Built-in compose view with header fields and vim-style body editor.
pub struct Editor {
    to: HeaderField,
    cc: HeaderField,
    subject: HeaderField,
    in_reply_to: Option<String>,
    body_state: EditorState,
    body_handler: EditorEventHandler,
    focus: Focus,
    suggestions: Vec<String>,
    ac_selected: usize,
    pub(super) attachments: Vec<Attachment>,
    attachment_selected: Option<usize>,
    attachment_path: Option<HeaderField>,
    path_matches: Vec<String>,
    path_selected: Option<usize>,
}

impl Editor {
    /// Create a new compose editor by parsing a draft string.
    pub fn new(content: &str) -> Self {
        let mut to = String::new();
        let mut cc = String::new();
        let mut subject = String::new();
        let mut in_reply_to = None;

        for line in content.lines().take_while(|l| *l != BODY_SENTINEL) {
            if let Some(val) = line.strip_prefix("To:") {
                to = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("Cc:") {
                cc = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("Subject:") {
                subject = val.trim().to_string();
            } else if let Some(val) = line.strip_prefix("In-Reply-To:") {
                in_reply_to = Some(val.trim().to_string());
            }
        }

        let body = content
            .lines()
            .position(|l| l == BODY_SENTINEL)
            .map(|i| content.lines().skip(i + 1).collect::<Vec<_>>().join("\n"))
            .unwrap_or_default();

        Self {
            to: HeaderField::new("To", &to),
            cc: HeaderField::new("Cc", &cc),
            subject: HeaderField::new("Subject", &subject),
            in_reply_to,
            body_state: EditorState::new(Lines::from(body.as_str())),
            body_handler: EditorEventHandler::default(),
            focus: Focus::To,
            suggestions: Vec::new(),
            ac_selected: 0,
            attachments: Vec::new(),
            attachment_selected: None,
            attachment_path: None,
            path_matches: Vec::new(),
            path_selected: None,
        }
    }

    pub fn title(&self) -> &str {
        &self.subject.value
    }

    pub fn draft(&self) -> super::compose::Draft {
        let mut draft = super::compose::Draft::parse(&self.text());
        draft.attachments = self.attachments.clone();
        draft
    }

    pub fn attachment_key(&mut self, key: crossterm::event::KeyEvent) -> anyhow::Result<bool> {
        use crossterm::event::{KeyCode, KeyModifiers};
        if self.attachment_path.is_some() {
            match key.code {
                KeyCode::Esc => self.attachment_path = None,
                KeyCode::Down => {
                    if !self.path_matches.is_empty() {
                        self.path_selected = Some(
                            self.path_selected
                                .map_or(0, |i| (i + 1).min(self.path_matches.len() - 1)),
                        );
                    }
                }
                KeyCode::Up => {
                    self.path_selected = self.path_selected.and_then(|i| i.checked_sub(1));
                }
                KeyCode::Tab | KeyCode::Enter => {
                    let selected = self.path_selected.or_else(|| {
                        (key.code == KeyCode::Tab && !self.path_matches.is_empty()).then_some(0)
                    });
                    if let Some(i) = selected {
                        self.attachment_path =
                            Some(HeaderField::new("Path", &self.path_matches[i]));
                    }
                    let input = &self.attachment_path.as_ref().unwrap().value;
                    let path = expand_path(input);
                    if key.code == KeyCode::Enter && !path.is_dir() {
                        self.attachments.push(Attachment::from_file(&path)?);
                        self.attachment_selected = Some(self.attachments.len() - 1);
                        self.attachment_path = None;
                    } else if key.code == KeyCode::Enter && !input.ends_with('/') {
                        self.attachment_path = Some(HeaderField::new("Path", &format!("{input}/")));
                    }
                    self.refresh_paths();
                }
                _ if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.attachment_path.as_mut().unwrap().on_key(key);
                    self.refresh_paths();
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(ref mut selected) = self.attachment_selected {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.attachment_selected = None,
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(self.attachments.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Char('d') | KeyCode::Delete if !self.attachments.is_empty() => {
                    self.attachments.remove(*selected);
                    *selected = (*selected).min(self.attachments.len().saturating_sub(1));
                }
                KeyCode::Char('a') => {
                    self.attachment_path = Some(HeaderField::new("Path", ""));
                    self.refresh_paths();
                }
                _ => {}
            }
            return Ok(true);
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('a') {
            self.suggestions.clear();
            self.attachment_selected = Some(0);
            return Ok(true);
        }
        Ok(false)
    }

    fn refresh_paths(&mut self) {
        self.path_selected = None;
        self.path_matches = self
            .attachment_path
            .as_ref()
            .map(|field| complete_path(&field.value))
            .unwrap_or_default();
    }

    fn draw_attachments(&self, frame: &mut ratatui::Frame) {
        let Some(selected) = self.attachment_selected else {
            return;
        };
        let labels = if self.attachments.is_empty() {
            vec!["No attachments".to_string()]
        } else {
            self.attachments
                .iter()
                .map(|a| format!("{} ({} bytes)", a.name, a.data.len()))
                .collect()
        };
        super::draw::draw_list_popup(
            frame,
            " Attachments — a: add, d: remove, Esc: close ",
            &labels,
            selected,
        );
        if let Some(field) = &self.attachment_path {
            let area = frame.area();
            let w = area.width.min(80);
            let h = area.height.min(14);
            let popup = Rect::new(
                area.x + area.width.saturating_sub(w) / 2,
                area.y + area.height.saturating_sub(h) / 2,
                w,
                h,
            );
            frame.render_widget(Clear, popup);
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Attach file — Tab: complete, Enter: attach, Esc: cancel ");
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(inner);
            draw_field(frame, chunks[0], field, true);
            let mut state = ListState::default().with_selected(self.path_selected);
            let list = List::new(self.path_matches.clone())
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");
            frame.render_stateful_widget(list, chunks[1], &mut state);
        }
    }

    /// Handle a crossterm key event.
    /// Returns `true` if the caller should refresh autocomplete suggestions.
    pub fn on_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Ctrl+C → Normal mode (body) or do nothing (headers).
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            if self.focus == Focus::Body {
                self.body_state.mode = EditorMode::Normal;
            }
            self.suggestions.clear();
            return false;
        }

        // Autocomplete navigation when suggestions are showing.
        if !self.suggestions.is_empty() {
            match key.code {
                KeyCode::Down => {
                    self.ac_selected =
                        (self.ac_selected + 1).min(self.suggestions.len().saturating_sub(1));
                    return false;
                }
                KeyCode::Up => {
                    self.ac_selected = self.ac_selected.saturating_sub(1);
                    return false;
                }
                KeyCode::Enter => {
                    self.accept_suggestion();
                    return false;
                }
                KeyCode::Esc => {
                    self.suggestions.clear();
                    return false;
                }
                _ => {
                    self.suggestions.clear();
                }
            }
        }

        // Tab/BackTab cycles focus between fields.
        if key.code == KeyCode::Tab {
            self.suggestions.clear();
            if self.focus != Focus::Body || matches!(self.body_state.mode, EditorMode::Normal) {
                self.focus = match self.focus {
                    Focus::To => Focus::Cc,
                    Focus::Cc => Focus::Subject,
                    Focus::Subject => Focus::Body,
                    Focus::Body => Focus::To,
                };
                return false;
            }
        }
        if key.code == KeyCode::BackTab {
            self.focus = match self.focus {
                Focus::To => Focus::Body,
                Focus::Cc => Focus::To,
                Focus::Subject => Focus::Cc,
                Focus::Body => Focus::Subject,
            };
            return false;
        }

        match self.focus {
            Focus::To => self.to.on_key(key),
            Focus::Cc => self.cc.on_key(key),
            Focus::Subject => self.subject.on_key(key),
            Focus::Body => {
                use crossterm::event::KeyCode;
                if matches!(
                    self.body_state.mode,
                    EditorMode::Normal | EditorMode::Visual
                ) {
                    match key.code {
                        KeyCode::Char('}') => {
                            self.move_next_paragraph();
                            return true;
                        }
                        KeyCode::Char('{') => {
                            self.move_prev_paragraph();
                            return true;
                        }
                        _ => {}
                    }
                }
                self.body_handler.on_key_event(key, &mut self.body_state);
            }
        }
        true
    }

    fn is_blank_line(&self, row: usize) -> bool {
        self.body_state
            .lines
            .get(RowIndex::new(row))
            .map(|l| l.iter().all(|c| c.is_whitespace()))
            .unwrap_or(true)
    }

    fn move_next_paragraph(&mut self) {
        let total = self.body_state.lines.len();
        let mut row = self.body_state.cursor.row;
        // skip blank lines at current position
        while row < total && self.is_blank_line(row) {
            row += 1;
        }
        // skip non-blank lines
        while row < total && !self.is_blank_line(row) {
            row += 1;
        }
        // stop at first blank line (paragraph boundary)
        self.body_state.cursor = Index2::new(row.min(total.saturating_sub(1)), 0);
        if self.body_state.mode == EditorMode::Visual
            && let Some(sel) = &mut self.body_state.selection
        {
            sel.end = self.body_state.cursor;
        }
    }

    fn move_prev_paragraph(&mut self) {
        let row = self.body_state.cursor.row;
        if row == 0 {
            return;
        }
        let mut r = row.saturating_sub(1);
        if self.is_blank_line(row) {
            // on a blank line: skip blanks going back, then skip non-blanks, stop at blank
            while r > 0 && self.is_blank_line(r) {
                r -= 1;
            }
            while r > 0 && !self.is_blank_line(r) {
                r -= 1;
            }
        } else {
            // on a non-blank line: skip non-blanks going back, stop at first blank
            while r > 0 && !self.is_blank_line(r) {
                r -= 1;
            }
        }
        // stop at first blank line (paragraph boundary), or line 0
        self.body_state.cursor = Index2::new(r, 0);
        if self.body_state.mode == EditorMode::Visual
            && let Some(sel) = &mut self.body_state.selection
        {
            sel.end = self.body_state.cursor;
        }
    }

    /// Reassemble the full draft text from header fields + body.
    pub fn text(&self) -> String {
        let mut draft = format!("To: {}\nSubject: {}\n", self.to.value, self.subject.value);
        if !self.cc.value.is_empty() {
            draft.push_str(&format!("Cc: {}\n", self.cc.value));
        }
        if let Some(ref irt) = self.in_reply_to {
            draft.push_str(&format!("In-Reply-To: {}\n", irt));
        }
        draft.push_str(&format!("{}\n", BODY_SENTINEL));
        draft.push_str(&self.body_state.lines.to_string());
        draft
    }

    /// Update autocomplete suggestions based on the focused field.
    pub fn update_autocomplete(&mut self, book: &AddressBook) {
        self.suggestions.clear();
        self.ac_selected = 0;

        let field = match self.focus {
            Focus::To => &self.to,
            Focus::Cc => &self.cc,
            _ => return,
        };

        let query = field.current_fragment();
        if query.len() < 2 {
            return;
        }

        self.suggestions = book
            .search(query)
            .into_iter()
            .take(8)
            .map(|a| a.full())
            .collect();
    }

    fn accept_suggestion(&mut self) {
        let Some(addr) = self.suggestions.get(self.ac_selected).cloned() else {
            return;
        };
        self.suggestions.clear();

        match self.focus {
            Focus::To => self.to.accept_suggestion(&addr),
            Focus::Cc => self.cc.accept_suggestion(&addr),
            _ => {}
        }
    }
}

/// Render the compose editor into `area`.
pub fn draw(frame: &mut ratatui::Frame, area: Rect, editor: &mut Editor) {
    let outer = Block::default().borders(Borders::NONE);
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    // Layout: 3 header lines + 1 separator + body.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    // Draw header fields.
    draw_header(frame, chunks[0], editor);

    // Draw body editor.
    frame.render_widget(
        Paragraph::new(format!(
            "Attachments: {} (Ctrl+A to add/remove)",
            editor.attachments.len()
        ))
        .style(Style::default().fg(Color::Cyan)),
        chunks[1],
    );
    draw_body(frame, chunks[2], editor);

    // Draw autocomplete dropdown overlay.
    draw_autocomplete(frame, chunks[0], editor);
    editor.draw_attachments(frame);
}

fn expand_path(input: &str) -> std::path::PathBuf {
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return std::path::PathBuf::from(home).join(rest);
    }
    std::path::PathBuf::from(input)
}

fn complete_path(input: &str) -> Vec<String> {
    let (prefix, fragment) = input
        .rsplit_once('/')
        .map_or(("", input), |(dir, name)| (&input[..dir.len() + 1], name));
    let dir = if prefix.is_empty() {
        std::path::PathBuf::from(".")
    } else {
        expand_path(prefix)
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut matches: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.starts_with(fragment) || name.chars().any(char::is_control) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() && !metadata.is_dir() {
                return None;
            }
            Some(format!(
                "{prefix}{name}{}",
                if metadata.is_dir() { "/" } else { "" }
            ))
        })
        .collect();
    matches.sort();
    matches
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, editor: &Editor) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    draw_field(frame, rows[0], &editor.to, editor.focus == Focus::To);
    draw_field(frame, rows[1], &editor.cc, editor.focus == Focus::Cc);
    draw_field(
        frame,
        rows[2],
        &editor.subject,
        editor.focus == Focus::Subject,
    );
}

fn draw_field(frame: &mut ratatui::Frame, area: Rect, field: &HeaderField, focused: bool) {
    let label_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::BOLD);

    let label_w = field.label.len() as u16 + 2; // "To: "

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(label_w), Constraint::Min(0)])
        .split(area);

    let label = Paragraph::new(Span::styled(format!("{}: ", field.label), label_style));
    frame.render_widget(label, chunks[0]);

    if focused {
        // Show value with cursor.
        let before = &field.value[..field.cursor];
        let cursor_char = field.value[field.cursor..].chars().next().unwrap_or(' ');
        let after_len = field.cursor + cursor_char.len_utf8().min(field.value.len() - field.cursor);
        let after = &field.value[after_len..];

        let line = Line::from(vec![
            Span::raw(before),
            Span::styled(
                cursor_char.to_string(),
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
            ),
            Span::raw(after),
        ]);
        frame.render_widget(Paragraph::new(line), chunks[1]);
    } else {
        frame.render_widget(
            Paragraph::new(field.value.as_str()).style(Style::default()),
            chunks[1],
        );
    }
}

fn draw_body(frame: &mut ratatui::Frame, area: Rect, editor: &mut Editor) {
    // Highlight quoted lines ('>' prefix) in blue.
    let quote_style = Style::default().fg(Color::Blue);
    let mut highlights = Vec::new();
    for row in 0..editor.body_state.lines.len() {
        let Some(line) = editor.body_state.lines.get(RowIndex::new(row)) else {
            continue;
        };
        let first = line.iter().copied().find(|c| *c != ' ');
        if first == Some('>') && !line.is_empty() {
            highlights.push(Highlight::new(
                Index2::new(row, 0),
                Index2::new(row, line.len().saturating_sub(1)),
                quote_style,
            ));
        }
    }
    editor.body_state.set_highlights(highlights);

    let mode_style = match editor.body_state.mode {
        EditorMode::Normal => Style::default().bg(Color::Blue).fg(Color::Black),
        EditorMode::Insert => Style::default().bg(Color::Green).fg(Color::Black),
        EditorMode::Visual => Style::default().bg(Color::Magenta).fg(Color::Black),
        _ => Style::default(),
    };

    let status_line = EditorStatusLine::default()
        .style_mode(mode_style.add_modifier(Modifier::BOLD))
        .style_line(Style::default().fg(Color::DarkGray));

    let mut theme = EditorTheme::default()
        .base(Style::default().bg(Color::Reset).fg(Color::Reset))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .selection_style(Style::default().add_modifier(Modifier::REVERSED))
        .line_numbers_style(Style::default().fg(Color::DarkGray))
        .status_line(status_line);

    if editor.focus == Focus::Body {
        theme =
            theme.cursor_style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD));
    } else {
        theme = theme.hide_cursor();
    }

    let view = EditorView::new(&mut editor.body_state)
        .theme(theme)
        .wrap(true);
    frame.render_widget(view, area);

    // 80-column guide: highlight column 80 with a subtle background (no character, copy-safe)
    if area.width > 80 {
        let lines: Vec<Line> = std::iter::repeat_n(Line::from(" "), area.height as usize).collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(Color::DarkGray)),
            Rect::new(area.x + 80, area.y, 1, area.height),
        );
    }
}

fn draw_autocomplete(frame: &mut ratatui::Frame, header_area: Rect, editor: &Editor) {
    if editor.suggestions.is_empty() {
        return;
    }

    // Position below the focused field.
    let field_row = match editor.focus {
        Focus::To => 0,
        Focus::Cc => 1,
        _ => return,
    };

    let dropdown_y = header_area.y + field_row as u16 + 1;
    let label_w = match editor.focus {
        Focus::To => 4,
        Focus::Cc => 4,
        _ => 4,
    };
    let dropdown_x = header_area.x + label_w;
    let dropdown_w = 44u16.min(header_area.width.saturating_sub(label_w));
    let dropdown_h = (editor.suggestions.len() as u16 + 2).min(10);

    let dropdown_area = Rect::new(dropdown_x, dropdown_y, dropdown_w, dropdown_h);

    frame.render_widget(Clear, dropdown_area);

    let items: Vec<ListItem> = editor
        .suggestions
        .iter()
        .map(|s| ListItem::new(s.as_str()))
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(editor.ac_selected));

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" Addresses "))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED));

    frame.render_stateful_widget(list, dropdown_area, &mut list_state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::address::{Address, AddressBook};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn make_editor(draft: &str) -> Editor {
        Editor::new(draft)
    }

    #[test]
    fn attachment_paths_complete_and_files_can_be_added_and_removed() {
        let dir = std::env::temp_dir().join(format!(
            "kingi-compose-attachment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        let path = dir.join("docs/report ü.pdf");
        std::fs::write(&path, [0, 1, 255]).unwrap();
        let mut ed = Editor::new("To: a@x.com\nSubject: File\n--- body ---\nBody");
        ed.attachment_key(ctrl(KeyCode::Char('a'))).unwrap();
        ed.attachment_key(key(KeyCode::Char('a'))).unwrap();
        ed.attachment_path = Some(HeaderField::new("Path", &format!("{}/do", dir.display())));
        ed.refresh_paths();
        assert_eq!(ed.path_matches, [format!("{}/docs/", dir.display())]);
        ed.attachment_key(key(KeyCode::Tab)).unwrap();
        assert!(
            ed.attachment_path
                .as_ref()
                .unwrap()
                .value
                .ends_with("/docs/")
        );
        assert_eq!(ed.path_matches, [path.to_str().unwrap()]);
        ed.attachment_key(key(KeyCode::Down)).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| draw(frame, frame.area(), &mut ed))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(rendered.contains("report ü.pdf"));
        ed.attachment_key(key(KeyCode::Enter)).unwrap();
        assert!(ed.attachment_path.is_none());
        assert_eq!(ed.attachments[0].name, "report ü.pdf");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(ed.draft().attachments[0].data, [0, 1, 255]);
        ed.attachment_key(key(KeyCode::Char('a'))).unwrap();
        ed.attachment_path = Some(HeaderField::new("Path", path.to_str().unwrap()));
        assert!(ed.attachment_key(key(KeyCode::Enter)).is_err());
        assert!(ed.attachment_path.is_some());
        assert_eq!(ed.attachments.len(), 1);
        ed.attachment_key(key(KeyCode::Esc)).unwrap();
        ed.attachment_key(key(KeyCode::Char('d'))).unwrap();
        assert!(ed.attachments.is_empty());
        ed.attachment_key(key(KeyCode::Esc)).unwrap();
        assert!(!ed.attachment_key(key(KeyCode::Char('a'))).unwrap());
        assert!(Attachment::from_file(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn path_completion_supports_relative_and_home_paths() {
        let matches = complete_path("src/ui/edi");
        assert!(matches.contains(&"src/ui/editor.rs".to_string()));
        assert!(complete_path("src/u").contains(&"src/ui/".to_string()));
        assert!(complete_path("/nonexistent-kingi-directory/").is_empty());
        if let Some(home) = std::env::var_os("HOME") {
            assert_eq!(
                expand_path("~/file.pdf"),
                std::path::PathBuf::from(home).join("file.pdf")
            );
            assert!(complete_path("~/").iter().all(|p| p.starts_with("~/")));
        }
    }

    /// Build an editor focused on the body in Normal mode at the given row.
    fn editor_at(body: &str, row: usize) -> Editor {
        let draft = format!("To: \nSubject: \n--- body ---\n{body}");
        let mut ed = Editor::new(&draft);
        ed.focus = Focus::Body;
        ed.body_state.cursor = Index2::new(row, 0);
        ed
    }

    // ── new / parsing ────────────────────────────────────────────────────────

    #[test]
    fn new_parses_to_field() {
        let ed = make_editor("To: alice@x.com\nSubject: Hi\n--- body ---\n");
        assert_eq!(ed.to.value, "alice@x.com");
    }

    #[test]
    fn new_parses_cc_field() {
        let ed = make_editor("To: \nCc: bob@x.com\nSubject: Hi\n--- body ---\n");
        assert_eq!(ed.cc.value, "bob@x.com");
    }

    #[test]
    fn new_parses_subject_field() {
        let ed = make_editor("To: \nSubject: Hello there\n--- body ---\n");
        assert_eq!(ed.subject.value, "Hello there");
    }

    #[test]
    fn new_parses_in_reply_to() {
        let ed = make_editor("To: \nSubject: \nIn-Reply-To: <id@x.com>\n--- body ---\n");
        assert_eq!(ed.in_reply_to.as_deref(), Some("<id@x.com>"));
    }

    #[test]
    fn new_parses_body_after_sentinel() {
        let ed = make_editor("To: \nSubject: \n--- body ---\nHello world");
        assert_eq!(ed.body_state.lines.to_string(), "Hello world");
    }

    #[test]
    fn new_body_empty_when_no_sentinel() {
        let ed = make_editor("To: alice@x.com\nSubject: Hi\n");
        assert_eq!(ed.body_state.lines.to_string(), "");
    }

    #[test]
    fn new_missing_fields_are_empty() {
        let ed = make_editor("--- body ---\n");
        assert_eq!(ed.to.value, "");
        assert_eq!(ed.cc.value, "");
        assert_eq!(ed.subject.value, "");
        assert!(ed.in_reply_to.is_none());
    }

    #[test]
    fn new_focus_starts_at_to() {
        let ed = make_editor("To: \nSubject: \n--- body ---\n");
        assert_eq!(ed.focus, Focus::To);
    }

    // ── title ────────────────────────────────────────────────────────────────

    #[test]
    fn title_returns_subject_value() {
        let ed = make_editor("To: \nSubject: My Subject\n--- body ---\n");
        assert_eq!(ed.title(), "My Subject");
    }

    #[test]
    fn title_empty_when_no_subject() {
        let ed = make_editor("--- body ---\n");
        assert_eq!(ed.title(), "");
    }

    // ── text / roundtrip ─────────────────────────────────────────────────────

    #[test]
    fn text_contains_to_and_subject() {
        let ed = make_editor("To: alice@x.com\nSubject: Hi\n--- body ---\n");
        let t = ed.text();
        assert!(t.contains("To: alice@x.com"), "got: {t}");
        assert!(t.contains("Subject: Hi"), "got: {t}");
    }

    #[test]
    fn text_omits_cc_when_empty() {
        let ed = make_editor("To: a@x.com\nSubject: Hi\n--- body ---\n");
        assert!(!ed.text().contains("Cc:"));
    }

    #[test]
    fn text_includes_cc_when_present() {
        let ed = make_editor("To: a@x.com\nCc: b@x.com\nSubject: Hi\n--- body ---\n");
        assert!(ed.text().contains("Cc: b@x.com"));
    }

    #[test]
    fn text_includes_in_reply_to_when_present() {
        let ed = make_editor("To: \nSubject: \nIn-Reply-To: <id@x.com>\n--- body ---\n");
        assert!(ed.text().contains("In-Reply-To: <id@x.com>"));
    }

    #[test]
    fn text_contains_body_sentinel() {
        let ed = make_editor("To: \nSubject: \n--- body ---\n");
        assert!(ed.text().contains("--- body ---"));
    }

    #[test]
    fn text_roundtrip_preserves_fields() {
        let original = "To: alice@x.com\nSubject: Test\n--- body ---\nHi there";
        let ed = make_editor(original);
        let out = ed.text();
        let ed2 = make_editor(&out);
        assert_eq!(ed2.to.value, "alice@x.com");
        assert_eq!(ed2.subject.value, "Test");
        assert_eq!(ed2.body_state.lines.to_string(), "Hi there");
    }

    // ── Tab / BackTab focus cycling ──────────────────────────────────────────

    #[test]
    fn tab_cycles_to_cc_subject_body_to() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        assert_eq!(ed.focus, Focus::To);
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::Cc);
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::Subject);
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::Body);
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::To);
    }

    #[test]
    fn backtab_cycles_to_body_subject_cc_to() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        assert_eq!(ed.focus, Focus::To);
        ed.on_key(key(KeyCode::BackTab));
        assert_eq!(ed.focus, Focus::Body);
        ed.on_key(key(KeyCode::BackTab));
        assert_eq!(ed.focus, Focus::Subject);
        ed.on_key(key(KeyCode::BackTab));
        assert_eq!(ed.focus, Focus::Cc);
        ed.on_key(key(KeyCode::BackTab));
        assert_eq!(ed.focus, Focus::To);
    }

    #[test]
    fn tab_in_body_insert_mode_does_not_cycle_focus() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.focus = Focus::Body;
        ed.body_state.mode = EditorMode::Insert;
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::Body);
    }

    #[test]
    fn tab_in_body_normal_mode_cycles_focus() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.focus = Focus::Body;
        ed.body_state.mode = EditorMode::Normal;
        ed.on_key(key(KeyCode::Tab));
        assert_eq!(ed.focus, Focus::To);
    }

    // ── header field editing ─────────────────────────────────────────────────

    #[test]
    fn typing_in_to_field_appends_text() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.on_key(key(KeyCode::Char('a')));
        ed.on_key(key(KeyCode::Char('b')));
        assert_eq!(ed.to.value, "ab");
    }

    #[test]
    fn backspace_in_to_field_removes_last_char() {
        let mut ed = make_editor("To: hi\nSubject: \n--- body ---\n");
        ed.on_key(key(KeyCode::Backspace));
        assert_eq!(ed.to.value, "h");
    }

    #[test]
    fn delete_in_to_field_removes_char_at_cursor() {
        let mut ed = make_editor("To: ab\nSubject: \n--- body ---\n");
        ed.to.cursor = 0;
        ed.on_key(key(KeyCode::Delete));
        assert_eq!(ed.to.value, "b");
    }

    #[test]
    fn left_moves_cursor_left_in_header() {
        let mut ed = make_editor("To: ab\nSubject: \n--- body ---\n");
        let before = ed.to.cursor;
        ed.on_key(key(KeyCode::Left));
        assert!(ed.to.cursor < before);
    }

    #[test]
    fn right_moves_cursor_right_in_header() {
        let mut ed = make_editor("To: ab\nSubject: \n--- body ---\n");
        ed.to.cursor = 0;
        ed.on_key(key(KeyCode::Right));
        assert_eq!(ed.to.cursor, 1);
    }

    #[test]
    fn home_moves_cursor_to_start_of_header() {
        let mut ed = make_editor("To: abc\nSubject: \n--- body ---\n");
        ed.on_key(key(KeyCode::Home));
        assert_eq!(ed.to.cursor, 0);
    }

    #[test]
    fn end_moves_cursor_to_end_of_header() {
        let mut ed = make_editor("To: abc\nSubject: \n--- body ---\n");
        ed.to.cursor = 0;
        ed.on_key(key(KeyCode::End));
        assert_eq!(ed.to.cursor, ed.to.value.len());
    }

    #[test]
    fn typing_in_cc_field_works() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.focus = Focus::Cc;
        ed.on_key(key(KeyCode::Char('x')));
        assert_eq!(ed.cc.value, "x");
    }

    #[test]
    fn typing_in_subject_field_works() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.focus = Focus::Subject;
        ed.on_key(key(KeyCode::Char('H')));
        ed.on_key(key(KeyCode::Char('i')));
        assert_eq!(ed.subject.value, "Hi");
    }

    // ── Ctrl+C ───────────────────────────────────────────────────────────────

    #[test]
    fn ctrl_c_in_body_insert_mode_switches_to_normal() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.focus = Focus::Body;
        ed.body_state.mode = EditorMode::Insert;
        ed.on_key(ctrl(KeyCode::Char('c')));
        assert_eq!(ed.body_state.mode, EditorMode::Normal);
    }

    #[test]
    fn ctrl_c_clears_suggestions() {
        let mut ed = make_editor("To: \nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.on_key(ctrl(KeyCode::Char('c')));
        assert!(ed.suggestions.is_empty());
    }

    // ── autocomplete ─────────────────────────────────────────────────────────

    fn make_book(entries: &[&str]) -> AddressBook {
        let mut book = AddressBook::default();
        let addrs: Vec<Address> = entries.iter().map(|s| Address::new("", s)).collect();
        book.harvest(&addrs);
        book
    }

    #[test]
    fn update_autocomplete_short_query_yields_no_suggestions() {
        let mut ed = make_editor("To: a\nSubject: \n--- body ---\n");
        let book = make_book(&["alice@x.com"]);
        ed.update_autocomplete(&book);
        assert!(ed.suggestions.is_empty());
    }

    #[test]
    fn update_autocomplete_matching_query_yields_suggestions() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        let book = make_book(&["alice@x.com"]);
        ed.update_autocomplete(&book);
        assert!(!ed.suggestions.is_empty());
        assert!(ed.suggestions[0].contains("alice@x.com"));
    }

    #[test]
    fn update_autocomplete_only_for_to_and_cc() {
        let mut ed = make_editor("To: \nSubject: alice\n--- body ---\n");
        ed.focus = Focus::Subject;
        let book = make_book(&["alice@x.com"]);
        ed.update_autocomplete(&book);
        assert!(ed.suggestions.is_empty());
    }

    #[test]
    fn autocomplete_down_advances_selection() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string(), "alicia@x.com".to_string()];
        ed.ac_selected = 0;
        ed.on_key(key(KeyCode::Down));
        assert_eq!(ed.ac_selected, 1);
    }

    #[test]
    fn autocomplete_down_clamps_at_last() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["a".to_string(), "b".to_string()];
        ed.ac_selected = 1;
        ed.on_key(key(KeyCode::Down));
        assert_eq!(ed.ac_selected, 1);
    }

    #[test]
    fn autocomplete_up_decreases_selection() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["a".to_string(), "b".to_string()];
        ed.ac_selected = 1;
        ed.on_key(key(KeyCode::Up));
        assert_eq!(ed.ac_selected, 0);
    }

    #[test]
    fn autocomplete_up_clamps_at_zero() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["a".to_string()];
        ed.ac_selected = 0;
        ed.on_key(key(KeyCode::Up));
        assert_eq!(ed.ac_selected, 0);
    }

    #[test]
    fn autocomplete_esc_clears_suggestions() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.on_key(key(KeyCode::Esc));
        assert!(ed.suggestions.is_empty());
    }

    #[test]
    fn autocomplete_enter_accepts_suggestion_into_to() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.ac_selected = 0;
        ed.on_key(key(KeyCode::Enter));
        assert!(ed.to.value.contains("alice@x.com"));
        assert!(ed.suggestions.is_empty());
    }

    #[test]
    fn autocomplete_any_other_key_clears_suggestions() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.on_key(key(KeyCode::Char('z')));
        assert!(ed.suggestions.is_empty());
    }

    #[test]
    fn accept_suggestion_replaces_fragment_after_comma() {
        let mut ed = make_editor("To: bob@x.com, ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.ac_selected = 0;
        ed.on_key(key(KeyCode::Enter));
        assert!(ed.to.value.starts_with("bob@x.com,"));
        assert!(ed.to.value.contains("alice@x.com"));
    }

    #[test]
    fn accept_suggestion_replaces_whole_value_without_comma() {
        let mut ed = make_editor("To: ali\nSubject: \n--- body ---\n");
        ed.suggestions = vec!["alice@x.com".to_string()];
        ed.ac_selected = 0;
        ed.on_key(key(KeyCode::Enter));
        assert!(!ed.to.value.contains("ali,"));
        assert!(ed.to.value.contains("alice@x.com"));
    }

    // ── next paragraph (}) ──────────────────────────────────────────────────

    #[test]
    fn next_paragraph_stops_at_blank_line() {
        // from non-blank, stop at the blank separator
        let mut ed = editor_at("line1\nline2\n\nline3\nline4", 0);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 2);
    }

    #[test]
    fn next_paragraph_from_blank_skips_blanks_then_stops_at_next_blank() {
        // from a blank line, skip blanks, skip para, stop at next blank
        let mut ed = editor_at("line1\nline2\n\nline3\nline4\n\nline5", 2);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 5);
    }

    #[test]
    fn next_paragraph_from_middle_of_paragraph_stops_at_blank() {
        let mut ed = editor_at("line1\nline2\n\nline3\nline4", 1);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 2);
    }

    #[test]
    fn next_paragraph_skips_multiple_blank_lines_from_blank() {
        // rows: 0=para1, 1=blank, 2=blank, 3=blank, 4=para2, 5=blank, 6=more
        // starting on a blank, skip all blanks, skip para2, stop at blank (row 5)
        let mut ed = editor_at("para1\n\n\n\npara2\n\nmore", 1);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 5);
    }

    #[test]
    fn next_paragraph_at_last_paragraph_stays_on_last_line() {
        let mut ed = editor_at("only\nline", 0);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 1);
    }

    #[test]
    fn next_paragraph_multiple_jumps() {
        // a(0) blank(1) b(2) blank(3) c(4)
        let mut ed = editor_at("a\n\nb\n\nc", 0);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 1); // first blank
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 3); // second blank
    }

    // ── prev paragraph ({) ──────────────────────────────────────────────────

    #[test]
    fn prev_paragraph_from_non_blank_stops_at_blank() {
        // from last para, stop at blank separator
        let mut ed = editor_at("line1\nline2\n\nline3\nline4", 4);
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 2);
    }

    #[test]
    fn prev_paragraph_from_blank_skips_blanks_then_stops_at_prev_blank() {
        // from blank, skip blanks back, skip para back, stop at prev blank
        let mut ed = editor_at("line1\n\nline2\nline3\n\nline4", 4);
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 1);
    }

    #[test]
    fn prev_paragraph_at_first_line_stays() {
        let mut ed = editor_at("line1\nline2", 0);
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 0);
    }

    #[test]
    fn prev_paragraph_multiple_jumps() {
        // a(0) blank(1) b(2) blank(3) c(4)
        let mut ed = editor_at("a\n\nb\n\nc", 4);
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 3); // second blank
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 1); // first blank
    }

    // ── visual mode paragraph navigation ────────────────────────────────────

    fn editor_visual_at(body: &str, anchor_row: usize, cursor_row: usize) -> Editor {
        use edtui::actions::SwitchMode;
        let draft = format!("To: \nSubject: \n--- body ---\n{body}");
        let mut ed = Editor::new(&draft);
        ed.focus = Focus::Body;
        // Position at anchor, enter visual mode (sets selection.start = anchor).
        ed.body_state.cursor = Index2::new(anchor_row, 0);
        ed.body_state.execute(SwitchMode(EditorMode::Visual));
        // Then move cursor to the starting position.
        ed.body_state.cursor = Index2::new(cursor_row, 0);
        if let Some(sel) = &mut ed.body_state.selection {
            sel.end = ed.body_state.cursor;
        }
        ed
    }

    #[test]
    fn next_paragraph_in_visual_mode_moves_cursor() {
        let mut ed = editor_visual_at("line1\nline2\n\nline3\nline4", 0, 0);
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, 2); // stops at blank line
    }

    #[test]
    fn next_paragraph_in_visual_mode_extends_selection() {
        let mut ed = editor_visual_at("line1\nline2\n\nline3\nline4", 0, 0);
        ed.on_key(key(KeyCode::Char('}')));
        let sel = ed.body_state.selection.unwrap();
        assert_eq!(sel.start.row, 0);
        assert_eq!(sel.end.row, 2);
    }

    #[test]
    fn prev_paragraph_in_visual_mode_moves_cursor() {
        let mut ed = editor_visual_at("line1\nline2\n\nline3\nline4", 4, 4);
        ed.on_key(key(KeyCode::Char('{')));
        assert_eq!(ed.body_state.cursor.row, 2); // stops at blank line
    }

    #[test]
    fn prev_paragraph_in_visual_mode_extends_selection() {
        let mut ed = editor_visual_at("line1\nline2\n\nline3\nline4", 4, 4);
        ed.on_key(key(KeyCode::Char('{')));
        let sel = ed.body_state.selection.unwrap();
        assert_eq!(sel.start.row, 4);
        assert_eq!(sel.end.row, 2);
    }

    // ── insert mode passthrough ──────────────────────────────────────────────

    #[test]
    fn braces_type_normally_in_insert_mode() {
        let mut ed = editor_at("hello", 0);
        ed.body_state.mode = EditorMode::Insert;
        let row_before = ed.body_state.cursor.row;
        ed.on_key(key(KeyCode::Char('}')));
        assert_eq!(ed.body_state.cursor.row, row_before);
    }
}
