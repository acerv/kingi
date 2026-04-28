// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::config;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

#[derive(Debug, PartialEq)]
pub enum HelpAction {
    Continue,
    Close,
}

pub struct HelpView {
    scroll: u16,
    total_lines: u16,
    visible_height: u16,
    search: String,
    searching: bool,
}

impl HelpView {
    pub fn new() -> Self {
        Self {
            scroll: 0,
            total_lines: 0,
            visible_height: 0,
            search: String::new(),
            searching: false,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> HelpAction {
        if self.searching {
            match key.code {
                KeyCode::Esc => {
                    self.searching = false;
                    self.search.clear();
                    self.scroll = 0;
                }
                KeyCode::Enter => {
                    self.searching = false;
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.scroll = 0;
                }
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.scroll = 0;
                }
                _ => {}
            }
            return HelpAction::Continue;
        }

        match (key.modifiers, key.code) {
            (_, KeyCode::Esc) => {
                if !self.search.is_empty() {
                    self.search.clear();
                    self.scroll = 0;
                    return HelpAction::Continue;
                }
                return HelpAction::Close;
            }
            (_, KeyCode::Char('q') | KeyCode::Char('?')) => return HelpAction::Close,
            (_, KeyCode::Char('j') | KeyCode::Down) => self.scroll_down(1),
            (_, KeyCode::Char('k') | KeyCode::Up) => self.scroll_up(1),
            (KeyModifiers::CONTROL, KeyCode::Char('d')) | (_, KeyCode::PageDown) => {
                self.scroll_down(15)
            }
            (KeyModifiers::CONTROL, KeyCode::Char('u')) | (_, KeyCode::PageUp) => {
                self.scroll_up(15)
            }
            (_, KeyCode::Char('g')) => self.scroll = 0,
            (_, KeyCode::Char('G')) => {
                self.scroll = self
                    .total_lines
                    .saturating_sub(self.visible_height);
            }
            (_, KeyCode::Char('/')) => {
                self.searching = true;
            }
            _ => {}
        }
        HelpAction::Continue
    }

    pub fn draw(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let (lines, _) = self.build_lines();
        self.total_lines = lines.len() as u16;

        let (full_lines, full_width) = self.full_size();

        let title = if self.searching {
            format!(" Help — /{}_ ", self.search)
        } else if !self.search.is_empty() {
            format!(" Help — search: {} (Esc to clear) ", self.search)
        } else {
            " Help — ? to close, / to search ".to_string()
        };

        let title_len = title.len() as u16;
        let popup_w = (full_width + 4).max(title_len + 2).min(area.width);
        let popup_h = (full_lines + 2).min(area.height);

        let x = area.width.saturating_sub(popup_w) / 2;
        let y = area.height.saturating_sub(popup_h) / 2;
        let popup_area = Rect::new(x, y, popup_w, popup_h);

        frame.render_widget(Clear, popup_area);

        let block = Block::default().borders(Borders::ALL).title(Span::styled(
            title,
            Style::default().add_modifier(Modifier::BOLD),
        ));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let padded = Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        );
        self.visible_height = padded.height;

        if self.total_lines <= self.visible_height {
            self.scroll = 0;
        }

        let paragraph = Paragraph::new(lines).scroll((self.scroll, 0));
        frame.render_widget(paragraph, padded);
    }

    fn scroll_down(&mut self, n: u16) {
        if self.total_lines <= self.visible_height {
            return;
        }
        self.scroll = self
            .scroll
            .saturating_add(n)
            .min(self.total_lines.saturating_sub(self.visible_height));
    }

    fn scroll_up(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    fn full_size(&self) -> (u16, u16) {
        let sections = self.sections();
        let key_col = 16usize;
        let mut max_w = 0usize;
        let mut total = 0u16;
        for (i, (header, entries)) in sections.iter().enumerate() {
            if i > 0 {
                total += 1;
            }
            total += 2 + entries.len() as u16;
            max_w = max_w.max(header.len());
            for (_, desc) in entries {
                max_w = max_w.max(key_col + desc.len());
            }
        }
        (total, max_w as u16)
    }

    fn build_lines(&self) -> (Vec<Line<'static>>, u16) {
        let query = if self.search.is_empty() {
            None
        } else {
            Some(self.search.to_lowercase())
        };

        let sections = self.sections();
        let key_col = 16usize;

        let filtered_sections: Vec<(&str, Vec<(&str, &str)>)> = sections
            .iter()
            .filter_map(|(header, entries)| {
                let kept: Vec<(&str, &str)> = if let Some(ref q) = query {
                    entries
                        .iter()
                        .filter(|(k, d)| {
                            k.to_lowercase().contains(q) || d.to_lowercase().contains(q)
                        })
                        .copied()
                        .collect()
                } else {
                    entries.to_vec()
                };
                if query.is_some() && kept.is_empty() {
                    None
                } else {
                    Some((*header, kept))
                }
            })
            .collect();

        let mut max_w = 0usize;
        for (header, entries) in &filtered_sections {
            max_w = max_w.max(header.len());
            for (_, desc) in entries {
                max_w = max_w.max(key_col + desc.len());
            }
        }

        let mut lines = Vec::new();
        for (i, (header, entries)) in filtered_sections.iter().enumerate() {
            if i > 0 {
                lines.push(Line::from(""));
            }

            lines.push(Line::from(Span::styled(
                header.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "─".repeat(max_w),
                Style::default().fg(Color::DarkGray),
            )));

            for (key, desc) in entries {
                let key_span = Span::styled(
                    format!("{:<key_col$}", key),
                    Style::default().fg(Color::Cyan),
                );
                let desc_span = Span::raw(desc.to_string());
                lines.push(Line::from(vec![key_span, desc_span]));
            }
        }

        (lines, max_w as u16)
    }

    fn sections(&self) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
        let mut thread_keys: Vec<(&str, &str)> = vec![
            ("j/k/↑/↓", "Move selection"),
            ("Ctrl+D/Ctrl+U", "Page down / up"),
            ("g / G", "First / last email"),
            ("J / K", "Next / previous mailbox"),
            ("Enter", "Open email"),
            ("r / R", "Reply / reply quoted"),
        ];
        self.append_quick_reply_keys(&mut thread_keys);
        thread_keys.extend([
            ("f", "Forward"),
            ("C", "Compose new email"),
            ("v", "Toggle read / unread"),
            ("Ctrl+A", "Mark all as read"),
            ("Space", "Toggle flagged"),
            ("m / M", "Move email / thread"),
            ("D", "Delete email"),
            ("s", "Toggle sort order"),
            ("V", "Unread-only filter"),
            ("/ / Esc", "Search / clear search"),
            ("Ctrl+S", "Force sync"),
            ("?", "Help"),
            ("Q", "Quit"),
        ]);

        let mut email_keys: Vec<(&str, &str)> = vec![
            ("j/k/↑/↓", "Scroll line"),
            ("Ctrl+D/Ctrl+U", "Page down / up"),
            ("g / G", "Top / bottom"),
            ("J / K", "Next / previous email"),
            ("Y", "Copy body to clipboard"),
            ("r / R", "Reply / reply quoted"),
        ];
        self.append_quick_reply_keys(&mut email_keys);
        email_keys.extend([
            ("f", "Forward"),
            ("m / M", "Move email / thread"),
            ("D", "Delete and close tab"),
            ("?", "Help"),
            ("q", "Close tab"),
        ]);

        let compose_keys = vec![
            ("Ctrl+Q", "Send dialog"),
            ("j/k", "Navigate dialog options"),
            ("Enter", "Confirm"),
            ("Esc", "Back to editor"),
        ];

        let global_keys = vec![
            ("Ctrl+N/Ctrl+P", "Next / previous tab"),
            ("?", "Help"),
        ];

        vec![
            ("Thread list", thread_keys),
            ("Email tab", email_keys),
            ("Compose", compose_keys),
            ("Global", global_keys),
        ]
    }

    fn append_quick_reply_keys<'a>(&self, keys: &mut Vec<(&'a str, &'a str)>) {
        for n in 1..=9u8 {
            if config::load_reply_template(n).is_some() {
                let key: &'static str = match n {
                    1 => "1",
                    2 => "2",
                    3 => "3",
                    4 => "4",
                    5 => "5",
                    6 => "6",
                    7 => "7",
                    8 => "8",
                    9 => "9",
                    _ => unreachable!(),
                };
                keys.push((key, "Quick reply with template"));
            }
        }
        if config::load_reply_template(0).is_some() {
            keys.push(("0", "Quick reply with template"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventKind, KeyEventState};

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

    fn lines_text(hv: &HelpView) -> String {
        let (lines, _) = hv.build_lines();
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn question_mark_closes() {
        let mut hv = HelpView::new();
        assert_eq!(hv.on_key(key(KeyCode::Char('?'))), HelpAction::Close);
    }

    #[test]
    fn q_closes() {
        let mut hv = HelpView::new();
        assert_eq!(hv.on_key(key(KeyCode::Char('q'))), HelpAction::Close);
    }

    #[test]
    fn esc_closes_when_no_search() {
        let mut hv = HelpView::new();
        assert_eq!(hv.on_key(key(KeyCode::Esc)), HelpAction::Close);
    }

    #[test]
    fn esc_clears_search_first() {
        let mut hv = HelpView::new();
        hv.search = "test".to_string();
        assert_eq!(hv.on_key(key(KeyCode::Esc)), HelpAction::Continue);
        assert!(hv.search.is_empty());
    }

    #[test]
    fn j_scrolls_down() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.on_key(key(KeyCode::Char('j')));
        assert_eq!(hv.scroll, 1);
    }

    #[test]
    fn k_scrolls_up() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.scroll = 5;
        hv.on_key(key(KeyCode::Char('k')));
        assert_eq!(hv.scroll, 4);
    }

    #[test]
    fn k_does_not_scroll_below_zero() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('k')));
        assert_eq!(hv.scroll, 0);
    }

    #[test]
    fn ctrl_d_scrolls_page_down() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.on_key(ctrl(KeyCode::Char('d')));
        assert_eq!(hv.scroll, 15);
    }

    #[test]
    fn ctrl_u_scrolls_page_up() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.scroll = 20;
        hv.on_key(ctrl(KeyCode::Char('u')));
        assert_eq!(hv.scroll, 5);
    }

    #[test]
    fn g_scrolls_to_top() {
        let mut hv = HelpView::new();
        hv.scroll = 10;
        hv.on_key(key(KeyCode::Char('g')));
        assert_eq!(hv.scroll, 0);
    }

    #[test]
    fn capital_g_scrolls_to_bottom() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.on_key(key(KeyCode::Char('G')));
        assert_eq!(hv.scroll, 30);
    }

    #[test]
    fn slash_enters_search_mode() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('/')));
        assert!(hv.searching);
    }

    #[test]
    fn search_filters_in_realtime() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('/')));
        hv.on_key(key(KeyCode::Char('q')));
        hv.on_key(key(KeyCode::Char('u')));
        hv.on_key(key(KeyCode::Char('i')));
        hv.on_key(key(KeyCode::Char('t')));
        assert!(hv.searching);
        let text = lines_text(&hv);
        assert!(text.contains("Quit"), "Quit should match: {text}");
        assert!(!text.contains("Forward"), "Forward should be filtered: {text}");
    }

    #[test]
    fn search_enter_keeps_filter() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('/')));
        hv.on_key(key(KeyCode::Char('r')));
        hv.on_key(key(KeyCode::Enter));
        assert!(!hv.searching);
        assert_eq!(hv.search, "r");
    }

    #[test]
    fn search_esc_cancels_and_clears() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('/')));
        hv.on_key(key(KeyCode::Char('x')));
        hv.on_key(key(KeyCode::Esc));
        assert!(!hv.searching);
        assert!(hv.search.is_empty());
    }

    #[test]
    fn search_empty_enter_clears_filter() {
        let mut hv = HelpView::new();
        hv.search = "old".to_string();
        hv.on_key(key(KeyCode::Char('/')));
        // backspace to clear
        hv.on_key(key(KeyCode::Backspace));
        hv.on_key(key(KeyCode::Backspace));
        hv.on_key(key(KeyCode::Backspace));
        hv.on_key(key(KeyCode::Enter));
        assert!(hv.search.is_empty());
    }

    #[test]
    fn build_lines_has_all_sections() {
        let hv = HelpView::new();
        let text = lines_text(&hv);
        assert!(text.contains("Thread list"), "missing Thread list");
        assert!(text.contains("Email tab"), "missing Email tab");
        assert!(text.contains("Compose"), "missing Compose");
        assert!(text.contains("Global"), "missing Global");
    }

    #[test]
    fn search_filters_content() {
        let mut hv = HelpView::new();
        hv.search = "quit".to_string();
        let text = lines_text(&hv);
        assert!(text.contains("Quit"), "Quit should match");
        assert!(!text.contains("Forward"), "Forward should be filtered out");
    }

    #[test]
    fn keys_during_search_do_not_close() {
        let mut hv = HelpView::new();
        hv.on_key(key(KeyCode::Char('/')));
        assert_eq!(
            hv.on_key(key(KeyCode::Char('q'))),
            HelpAction::Continue
        );
        assert!(hv.searching);
    }

    #[test]
    fn pagedown_scrolls() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.on_key(key(KeyCode::PageDown));
        assert_eq!(hv.scroll, 15);
    }

    #[test]
    fn pageup_scrolls() {
        let mut hv = HelpView::new();
        hv.total_lines = 50;
        hv.visible_height = 20;
        hv.scroll = 20;
        hv.on_key(key(KeyCode::PageUp));
        assert_eq!(hv.scroll, 5);
    }

    #[test]
    fn no_scroll_when_content_fits() {
        let mut hv = HelpView::new();
        hv.total_lines = 10;
        hv.visible_height = 50;
        hv.on_key(key(KeyCode::Char('j')));
        assert_eq!(hv.scroll, 0);
        hv.on_key(ctrl(KeyCode::Char('d')));
        assert_eq!(hv.scroll, 0);
        hv.on_key(key(KeyCode::Char('G')));
        assert_eq!(hv.scroll, 0);
    }

    #[test]
    fn scroll_clamped_to_max() {
        let mut hv = HelpView::new();
        hv.total_lines = 30;
        hv.visible_height = 20;
        hv.on_key(key(KeyCode::Char('G')));
        assert_eq!(hv.scroll, 10);
    }

    #[test]
    fn slash_preserves_existing_search() {
        let mut hv = HelpView::new();
        hv.search = "reply".to_string();
        hv.on_key(key(KeyCode::Char('/')));
        assert!(hv.searching);
        assert_eq!(hv.search, "reply");
    }
}
