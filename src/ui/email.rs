// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::address::Address;
use crate::core::attachment::Attachment;
use crate::core::gpg::{self, CryptoStatus, InlinePgpType, PgpMimeType, VerifyResult};
use crate::core::thread::Email;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use time::OffsetDateTime;
use time::macros::format_description;

pub struct EmailView {
    path: std::path::PathBuf,
    message_id: String,
    subject: String,
    from: Vec<Address>,
    to: Vec<Address>,
    cc: Vec<Address>,
    date: String,
    body_lines: Vec<Line<'static>>,
    scroll: u16,
    raw_body: String,
    crypto_status: CryptoStatus,
    attachments: Vec<Attachment>,
    attachment_selected: Option<usize>,
    save_path: Option<String>,
    attachment_notice: Option<String>,
}

impl EmailView {
    pub fn new(
        email: &Email,
        gpg_binary: &str,
        before_gpg: &mut dyn FnMut(),
        after_gpg: &mut dyn FnMut(),
    ) -> Result<Self> {
        let msg = email.to_message()?;

        let message_id;
        if let Some(id) = msg.message_id() {
            if id.is_empty() {
                return Err(anyhow::anyhow!("Message-ID is empty"));
            }

            message_id = id.to_string();
        } else {
            return Err(anyhow::anyhow!("Message-ID is None"));
        }

        let from = parse_addr_list(msg.from());
        let to = parse_addr_list(msg.to());
        let cc = parse_addr_list(msg.cc());
        let date = format_date(email.timestamp);

        // Detect and handle PGP content.
        let mut attachments = Attachment::from_message(&msg);
        let (raw_body, crypto_status) =
            decrypt_or_verify(&msg, gpg_binary, before_gpg, after_gpg, &mut attachments);

        let display_body = if raw_body.is_empty() {
            "— no text body —".to_string()
        } else {
            raw_body.clone()
        };
        let body_lines = highlight_body(&display_body);
        let subject = if email.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            email.subject.clone()
        };

        Ok(Self {
            path: email.path().to_path_buf(),
            message_id,
            subject,
            from,
            to,
            cc,
            date,
            body_lines,
            scroll: 0,
            raw_body,
            crypto_status,
            attachments,
            attachment_selected: None,
            save_path: None,
            attachment_notice: None,
        })
    }

    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn scroll_up(&mut self, steps: u16) {
        self.scroll = self.scroll.saturating_sub(steps);
    }

    pub fn scroll_down(&mut self, steps: u16) {
        let max = self.body_lines.len().saturating_sub(1) as u16;
        self.scroll = self.scroll.saturating_add(steps).min(max);
    }

    pub fn first_line(&mut self) {
        self.scroll = 0;
    }

    pub fn last_line(&mut self) {
        self.scroll = self
            .scroll
            .saturating_add(self.body_lines.len().saturating_sub(1) as u16);
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn message_id(&self) -> &str {
        &self.message_id
    }

    pub fn raw_body(&self) -> &str {
        &self.raw_body
    }

    /// Handle the attachment dialog before the usual email key bindings.
    pub fn attachment_key(&mut self, key: KeyEvent) -> Result<bool> {
        if let Some(ref mut path) = self.save_path {
            match key.code {
                KeyCode::Esc => self.save_path = None,
                KeyCode::Backspace => {
                    path.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    path.push(c);
                }
                KeyCode::Enter => {
                    let attachment = &self.attachments[self.attachment_selected.unwrap()];
                    attachment.save(std::path::Path::new(path))?;
                    self.attachment_notice = Some(format!("Saved to {path}"));
                    self.save_path = None;
                }
                _ => {}
            }
            return Ok(true);
        }
        if let Some(ref mut selected) = self.attachment_selected {
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    self.attachment_selected = None;
                    self.attachment_notice = None;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(self.attachments.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Enter | KeyCode::Char('s') if !self.attachments.is_empty() => {
                    self.save_path = Some(self.attachments[*selected].name.clone());
                    self.attachment_notice = None;
                }
                _ => {}
            }
            return Ok(true);
        }
        if key.code == KeyCode::Char('a') && key.modifiers == KeyModifiers::NONE {
            self.attachment_selected = Some(0);
            return Ok(true);
        }
        Ok(false)
    }

    fn draw_attachments(&self, frame: &mut ratatui::Frame) {
        let Some(selected) = self.attachment_selected else {
            return;
        };
        let labels: Vec<String> = if self.attachments.is_empty() {
            vec!["No attachments".to_string()]
        } else {
            self.attachments
                .iter()
                .map(|a| format!("{} ({} bytes)", a.name, a.data.len()))
                .collect()
        };
        let title = self
            .attachment_notice
            .as_deref()
            .unwrap_or(" Attachments — Enter: save, Esc: close ");
        crate::ui::draw::draw_list_popup(frame, title, &labels, selected);

        if let Some(path) = &self.save_path {
            let area = frame.area();
            let w = 80u16.min(area.width);
            let h = 5u16.min(area.height);
            let popup = ratatui::layout::Rect::new(
                area.x + area.width.saturating_sub(w) / 2,
                area.y + area.height.saturating_sub(h) / 2,
                w,
                h,
            );
            frame.render_widget(ratatui::widgets::Clear, popup);
            frame.render_widget(
                Paragraph::new(format!("{path}_\nEnter: save (no overwrite) | Esc: cancel"))
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Save attachment to "),
                    ),
                popup,
            );
        }
    }
}

/// Render the email view into `area`.
///
/// The area is split into a header block (From/To/Cc/Date) and a scrollable
/// body block below it.
pub fn draw(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, view: &mut EmailView) {
    const LABEL: usize = 7;
    let inner_width = area.width as usize;
    let value_width = inner_width.saturating_sub(LABEL).max(1);

    let from_str = format_addr_list(&view.from);
    let to_str = format_addr_list(&view.to);
    let cc_str = format_addr_list(&view.cc);
    let from_lines = wrap_header_field("From : ", &from_str, value_width);
    let to_lines = wrap_header_field("To   : ", &to_str, value_width);
    let cc_lines = wrap_header_field("Cc   : ", &cc_str, value_width);
    let subject_lines = wrap_header_field("Subj : ", view.subject(), value_width);

    let crypto_line = crypto_status_line(&view.crypto_status);
    let crypto_height = if crypto_line.is_some() { 1 } else { 0 };
    let attachment_height = usize::from(!view.attachments.is_empty());

    let header_height = (from_lines.len()
        + to_lines.len()
        + cc_lines.len()
        + subject_lines.len()
        + 1
        + 1
        + crypto_height
        + attachment_height) as u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(area);

    let mut header_text: Vec<Line> = Vec::new();
    header_text.extend(from_lines);
    header_text.extend(to_lines);
    header_text.extend(cc_lines);
    header_text.push(Line::from(vec![
        Span::styled("Date : ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(view.date.clone()),
    ]));
    header_text.extend(subject_lines);
    if let Some(line) = crypto_line {
        header_text.push(line);
    }
    if !view.attachments.is_empty() {
        header_text.push(Line::from(Span::styled(
            format!("Attachments: {} (a to list/save)", view.attachments.len()),
            Style::default().fg(Color::Cyan),
        )));
    }

    let header = Paragraph::new(header_text).block(Block::default().borders(Borders::BOTTOM));
    frame.render_widget(header, chunks[0]);

    // Calculate viewport height
    let body_height = chunks[1].height as usize;
    let max_scroll = view.body_lines.len().saturating_sub(body_height) as u16;

    // Clamp scroll so last line stays at bottom when content fits
    view.scroll = view.scroll.min(max_scroll);

    let body = Paragraph::new(view.body_lines.to_vec())
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((view.scroll, 0));
    frame.render_widget(body, chunks[1]);
    view.draw_attachments(frame);
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Try to decrypt or verify PGP content, falling back to the plain body.
///
/// The `before_gpg` / `after_gpg` callbacks suspend and restore the TUI so
/// that `pinentry-curses` can access the terminal for passphrase prompts.
fn decrypt_or_verify(
    msg: &mail_parser::Message,
    gpg_binary: &str,
    before_gpg: &mut dyn FnMut(),
    after_gpg: &mut dyn FnMut(),
    attachments: &mut Vec<Attachment>,
) -> (String, CryptoStatus) {
    // 1. Check for PGP/MIME (multipart/encrypted or multipart/signed).
    if let Some(pgp_type) = gpg::detect_pgp_mime(msg) {
        match pgp_type {
            PgpMimeType::Encrypted => {
                // The outer MIME parts are ciphertext, not user attachments.
                attachments.clear();
                before_gpg();
                let result = gpg::decrypt_pgp_mime(msg, gpg_binary);
                after_gpg();
                match result {
                    Ok((decrypted, status)) => {
                        *attachments = Attachment::from_message(&decrypted);
                        return (extract_body(&decrypted), status);
                    }
                    Err(e) => {
                        return (
                            extract_body(msg),
                            CryptoStatus::DecryptFailed(e.to_string()),
                        );
                    }
                }
            }
            PgpMimeType::Signed => {
                before_gpg();
                let result = gpg::verify_pgp_mime(msg, gpg_binary);
                after_gpg();
                match result {
                    Ok((body, status)) => return (body, status),
                    Err(e) => {
                        return (extract_body(msg), CryptoStatus::VerifyFailed(e.to_string()));
                    }
                }
            }
        }
    }

    // 2. Check for inline PGP in the text body.
    let body = extract_body(msg);

    if let Some(inline_type) = gpg::detect_inline_pgp(&body) {
        match inline_type {
            InlinePgpType::Encrypted => {
                before_gpg();
                let result = gpg::decrypt_inline_pgp(&body, gpg_binary);
                after_gpg();
                match result {
                    Ok((decrypted, status)) => return (decrypted, status),
                    Err(e) => return (body, CryptoStatus::DecryptFailed(e.to_string())),
                }
            }
            InlinePgpType::Signed => {
                before_gpg();
                let result = gpg::verify_inline_pgp(&body, gpg_binary);
                after_gpg();
                match result {
                    Ok((verified, status)) => return (verified, status),
                    Err(e) => return (body, CryptoStatus::VerifyFailed(e.to_string())),
                }
            }
        }
    }

    // 3. No PGP content detected.
    (body, CryptoStatus::None)
}

fn extract_body(msg: &mail_parser::Message) -> String {
    let has_plain = msg.text_bodies().any(|p| !p.is_text_html());
    if has_plain {
        msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default()
    } else if let Some(html) = msg.body_html(0) {
        html2text::from_read(html.as_bytes(), 80).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Build a status line for the crypto status, if any.
fn crypto_status_line(status: &CryptoStatus) -> Option<Line<'static>> {
    match status {
        CryptoStatus::None => None,
        CryptoStatus::Decrypted => Some(Line::from(Span::styled(
            "[Decrypted]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))),
        CryptoStatus::Signed(vr) => Some(verify_line("[Signed", vr)),
        CryptoStatus::DecryptedAndSigned(vr) => Some(verify_line("[Decrypted + Signed", vr)),
        CryptoStatus::DecryptFailed(err) => Some(Line::from(Span::styled(
            format!("[Decryption failed: {err}]"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))),
        CryptoStatus::VerifyFailed(err) => Some(Line::from(Span::styled(
            format!("[Verification failed: {err}]"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))),
    }
}

fn verify_line(prefix: &str, vr: &VerifyResult) -> Line<'static> {
    let (text, color) = match vr {
        VerifyResult::Good { signer } => (format!("{prefix}: {signer}]"), Color::Green),
        VerifyResult::Bad { signer } => (format!("{prefix}: BAD {signer}]"), Color::Red),
        VerifyResult::Unknown => (format!("{prefix}: unknown signer]"), Color::Yellow),
    };
    Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

/// Parse a `mail_parser` address header into a list of `Address` values.
fn parse_addr_list(list: Option<&mail_parser::Address>) -> Vec<Address> {
    list.map(|addrs| {
        addrs
            .iter()
            .map(|a| {
                Address::new(
                    a.name().unwrap_or_default(),
                    a.address().unwrap_or_default(),
                )
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Format a list of addresses into a comma-separated display string.
fn format_addr_list(addrs: &[Address]) -> String {
    if addrs.is_empty() {
        "—".to_string()
    } else {
        addrs
            .iter()
            .map(|a| a.full())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_date(ts: Option<i64>) -> String {
    let fmt = format_description!("[year]-[month]-[day] [hour]:[minute]");
    match ts.and_then(|t| OffsetDateTime::from_unix_timestamp(t).ok()) {
        Some(dt) => dt.format(fmt).unwrap_or_else(|_| "—".to_string()),
        None => "—".to_string(),
    }
}

fn highlight_body(body: &str) -> Vec<Line<'static>> {
    let mut in_diff = false;
    body.lines()
        .map(|raw| {
            let expanded = expand_tabs(raw);
            if expanded.starts_with("diff ")
                || expanded.starts_with("--- ")
                || expanded.starts_with("+++ ")
            {
                in_diff = true;
            } else if in_diff
                && !expanded.starts_with("@@")
                && !expanded.starts_with('+')
                && !expanded.starts_with('-')
                && !expanded.starts_with(' ')
                && !expanded.is_empty()
            {
                in_diff = false;
            }
            highlight_line_owned(expanded, in_diff)
        })
        .collect()
}

fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            let spaces = 8 - (col % 8);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

fn highlight_line_owned(raw: String, in_diff: bool) -> Line<'static> {
    let style = if raw.starts_with("--- ") && in_diff {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if raw.starts_with("+++ ") && in_diff {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if raw.starts_with("@@") && in_diff {
        Style::default().fg(Color::Cyan)
    } else if raw.starts_with('-') && in_diff {
        Style::default().fg(Color::Red)
    } else if raw.starts_with('+') && in_diff {
        Style::default().fg(Color::Green)
    } else if raw.starts_with('>') {
        Style::default().fg(Color::Blue)
    } else {
        Style::default()
    };
    Line::from(Span::styled(raw, style))
}

fn wrap_header_field<'a>(label: &'a str, value: &'a str, value_width: usize) -> Vec<Line<'a>> {
    let label_style = Style::default().add_modifier(Modifier::BOLD);
    let mut chars = value.chars();

    let first: String = chars.by_ref().take(value_width).collect();
    let mut lines = vec![Line::from(vec![
        Span::styled(label, label_style),
        Span::raw(first),
    ])];

    let indent = " ".repeat(label.chars().count());
    loop {
        let chunk: String = chars.by_ref().take(value_width).collect();
        if chunk.is_empty() {
            break;
        }
        lines.push(Line::from(vec![
            Span::raw(indent.clone()),
            Span::raw(chunk),
        ]));
    }
    lines
}

#[cfg(test)]
impl EmailView {
    pub(crate) fn new_stub(subject: &str) -> Self {
        Self {
            path: std::path::PathBuf::new(),
            message_id: "stub@test".to_string(),
            subject: subject.to_string(),
            from: Vec::new(),
            to: Vec::new(),
            cc: Vec::new(),
            date: String::new(),
            body_lines: Vec::new(),
            scroll: 0,
            raw_body: String::new(),
            crypto_status: CryptoStatus::None,
            attachments: Vec::new(),
            attachment_selected: None,
            save_path: None,
            attachment_notice: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::thread::Email;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("kingi_email2_test_{}", id));
        std::fs::create_dir_all(dir.join("new")).unwrap();
        dir
    }

    fn write_email(dir: &PathBuf, name: &str, content: &str) -> PathBuf {
        let path = dir.join("new").join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn full_email(from: &str, to: &str, cc: &str, subject: &str, body: &str) -> String {
        format!(
            "Message-ID: <test@example.com>\r\nFrom: {from}\r\nTo: {to}\r\nCc: {cc}\r\nSubject: {subject}\r\nDate: Mon, 01 Jan 2024 12:00:00 +0000\r\n\r\n{body}"
        )
    }

    fn make_view(dir: &PathBuf, content: &str) -> EmailView {
        let path = write_email(dir, "msg", content);
        let email = Email::from_file(&path).unwrap();
        EmailView::new(&email, "gpg", &mut || {}, &mut || {}).unwrap()
    }

    fn rendered_lines(view: &mut EmailView, w: u16, h: u16) -> Vec<String> {
        use ratatui::{Terminal, backend::TestBackend};
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(frame, ratatui::layout::Rect::new(0, 0, w, h), view))
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

    #[test]
    fn attachment_dialog_lists_saves_and_keeps_errors_retryable() {
        let dir = temp_dir();
        let mime = "Content-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=report.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC/w==\r\n--x--\r\n";
        let content = format!(
            "Message-ID: <attachment@test>\r\nFrom: a@x.com\r\nSubject: Document\r\nMIME-Version: 1.0\r\n{mime}"
        );
        let mut view = make_view(&dir, &content);
        assert_eq!(view.raw_body(), "Body");
        assert!(
            rendered_lines(&mut view, 100, 30)
                .iter()
                .any(|line| line.contains("Attachments: 1"))
        );
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert!(view.attachment_key(key(KeyCode::Char('a'))).unwrap());
        assert!(
            rendered_lines(&mut view, 100, 30)
                .iter()
                .any(|line| line.contains("report.pdf (4 bytes)"))
        );
        view.attachment_key(key(KeyCode::Enter)).unwrap();
        assert_eq!(view.save_path.as_deref(), Some("report.pdf"));
        let path = dir.join("saved.pdf");
        view.save_path = Some(path.to_str().unwrap().to_string());
        view.attachment_key(key(KeyCode::Enter)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0, 1, 2, 255]);
        assert!(view.attachment_notice.is_some());
        view.attachment_key(key(KeyCode::Enter)).unwrap();
        view.save_path = Some(path.to_str().unwrap().to_string());
        assert!(view.attachment_key(key(KeyCode::Enter)).is_err());
        assert!(view.save_path.is_some());
        view.attachment_key(key(KeyCode::Esc)).unwrap();
        view.attachment_key(key(KeyCode::Esc)).unwrap();
        assert!(view.attachment_selected.is_none());
        assert!(!view.attachment_key(key(KeyCode::Char('j'))).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn attachment_dialog_handles_messages_without_attachments() {
        let mut view = EmailView::new_stub("Plain email");
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        view.attachment_key(key(KeyCode::Char('a'))).unwrap();
        view.attachment_key(key(KeyCode::Down)).unwrap();
        view.attachment_key(key(KeyCode::Enter)).unwrap();
        assert!(view.save_path.is_none());
        assert!(
            rendered_lines(&mut view, 80, 20)
                .iter()
                .any(|line| line.contains("No attachments"))
        );
        view.attachment_key(key(KeyCode::Esc)).unwrap();
        assert!(view.attachment_selected.is_none());
    }

    // ── subject ──────────────────────────────────────────────────────────────

    #[test]
    fn subject_returns_subject() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "My Topic", "body"),
        );
        assert_eq!(view.subject(), "My Topic");
    }

    #[test]
    fn subject_returns_placeholder_when_empty() {
        let dir = temp_dir();
        let path = write_email(
            &dir,
            "msg",
            "Message-ID: <x@x>\r\nFrom: a@x.com\r\nTo: b@x.com\r\nDate: Mon, 01 Jan 2024 12:00:00 +0000\r\n\r\nbody",
        );
        let email = Email::from_file(&path).unwrap();
        let view = EmailView::new(&email, "gpg", &mut || {}, &mut || {}).unwrap();
        assert_eq!(view.subject(), "(no subject)");
    }

    // ── new ──────────────────────────────────────────────────────────────────

    #[test]
    fn new_stores_message_id() {
        let dir = temp_dir();
        let path = write_email(
            &dir,
            "msg",
            "Message-ID: <unique-id@example.com>\r\nFrom: a@x.com\r\nTo: b@x.com\r\nDate: Mon, 01 Jan 2024 12:00:00 +0000\r\n\r\nbody",
        );
        let email = Email::from_file(&path).unwrap();
        let view = EmailView::new(&email, "gpg", &mut || {}, &mut || {}).unwrap();
        assert_eq!(view.message_id(), "unique-id@example.com");
    }

    #[test]
    fn new_parses_from() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email(
                "Alice <alice@example.com>",
                "bob@example.com",
                "",
                "Hi",
                "body",
            ),
        );
        let from = format_addr_list(&view.from);
        assert!(from.contains("Alice"), "got: {}", from);
    }

    #[test]
    fn new_parses_to() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email(
                "alice@example.com",
                "Bob <bob@example.com>",
                "",
                "Hi",
                "body",
            ),
        );
        let to = format_addr_list(&view.to);
        assert!(to.contains("Bob"), "got: {}", to);
    }

    #[test]
    fn new_parses_cc() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "Carol <carol@x.com>", "Hi", "body"),
        );
        let cc = format_addr_list(&view.cc);
        assert!(cc.contains("Carol"), "got: {}", cc);
    }

    #[test]
    fn new_uses_subject_as_title() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "My Subject", "body"),
        );
        assert_eq!(view.subject, "My Subject");
    }

    #[test]
    fn new_formats_date() {
        let dir = temp_dir();
        let view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", "body"));
        assert_eq!(view.date, "2024-01-01 12:00");
    }

    #[test]
    fn new_missing_date_formats_dash() {
        let dir = temp_dir();
        let path = write_email(
            &dir,
            "msg",
            "Message-ID: <x@x>\r\nFrom: a@x.com\r\nTo: b@x.com\r\nSubject: Hi\r\n\r\nbody",
        );
        let email = Email::from_file(&path).unwrap();
        let view = EmailView::new(&email, "gpg", &mut || {}, &mut || {}).unwrap();
        assert_eq!(view.date, "—");
    }

    #[test]
    fn new_populates_body_lines() {
        let dir = temp_dir();
        let view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "Hi", "line one\nline two"),
        );
        assert!(!view.body_lines.is_empty());
    }

    #[test]
    fn new_no_body_uses_placeholder() {
        let dir = temp_dir();
        let path = write_email(
            &dir,
            "msg",
            "Message-ID: <x@x>\r\nFrom: a@x.com\r\nTo: b@x.com\r\nSubject: Hi\r\nDate: Mon, 01 Jan 2024 12:00:00 +0000\r\n\r\n",
        );
        let email = Email::from_file(&path).unwrap();
        let view = EmailView::new(&email, "gpg", &mut || {}, &mut || {}).unwrap();
        let text: String = view
            .body_lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(text.contains("no text body"), "got: {text}");
    }

    #[test]
    fn new_bad_path_returns_error() {
        use std::path::PathBuf;
        let email = Email::new(
            "x",
            None,
            "",
            "",
            None,
            PathBuf::from("/nonexistent/path/msg"),
        );
        assert!(EmailView::new(&email, "gpg", &mut || {}, &mut || {}).is_err());
    }

    // ── scroll ────────────────────────────────────────────────────────────────

    #[test]
    fn scroll_up_at_zero_stays() {
        let dir = temp_dir();
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", "body"));
        view.scroll_up(1);
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn scroll_down_capped_at_zero_when_no_max() {
        let dir = temp_dir();
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", "body"));
        view.scroll_down(1); // scroll_max is 0 by default
        assert_eq!(view.scroll, 0);
    }

    #[test]
    fn scroll_down_increments_within_max() {
        let dir = temp_dir();
        let body = (0..10).map(|i| format!("line {i}\n")).collect::<String>();
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", &body));
        view.scroll_down(3);
        assert_eq!(view.scroll, 3);
    }

    #[test]
    fn scroll_down_clamps_at_last_line() {
        let dir = temp_dir();
        // 3 body lines → max index = 2
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", "a\nb\nc"));
        view.scroll_down(100);
        assert_eq!(view.scroll, 2);
    }

    #[test]
    fn scroll_up_decrements_by_steps() {
        let dir = temp_dir();
        let body = (0..10).map(|i| format!("line {i}\n")).collect::<String>();
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", &body));
        view.scroll = 7;
        view.scroll_up(3);
        assert_eq!(view.scroll, 4);
    }

    #[test]
    fn scroll_up_clamps_at_zero() {
        let dir = temp_dir();
        let mut view = make_view(&dir, &full_email("a@x.com", "b@x.com", "", "Hi", "body"));
        view.scroll = 2;
        view.scroll_up(10);
        assert_eq!(view.scroll, 0);
    }

    // ── draw ─────────────────────────────────────────────────────────────────

    #[test]
    fn draw_shows_subject_in_title() {
        let dir = temp_dir();
        let mut view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "Important topic", "body"),
        );
        let content: String = rendered_lines(&mut view, 80, 20).join("\n");
        assert!(content.contains("Important topic"), "got:\n{content}");
    }

    #[test]
    fn draw_shows_from_header() {
        let dir = temp_dir();
        let mut view = make_view(
            &dir,
            &full_email("Alice <alice@example.com>", "b@x.com", "", "Hi", "body"),
        );
        let content: String = rendered_lines(&mut view, 80, 20).join("\n");
        assert!(content.contains("Alice"), "got:\n{content}");
    }

    #[test]
    fn draw_shows_body_content() {
        let dir = temp_dir();
        let mut view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "Hi", "Hello from the body"),
        );
        let content: String = rendered_lines(&mut view, 80, 20).join("\n");
        assert!(content.contains("Hello from the body"), "got:\n{content}");
    }

    #[test]
    fn draw_long_body_scrollable() {
        let dir = temp_dir();
        let long_body: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let mut view = make_view(
            &dir,
            &full_email("a@x.com", "b@x.com", "", "Hi", &long_body),
        );
        // scroll_down is limited to body_lines.len() - 1
        view.scroll_down(1000);
        assert!(view.scroll > 0);
        rendered_lines(&mut view, 80, 20); // must not panic with scroll set
    }

    // ── highlight_body ──────────────────────────────────────────────────────

    fn line_fg(line: &Line) -> Option<Color> {
        line.spans.first().and_then(|s| s.style.fg)
    }

    #[test]
    fn highlight_diff_lines_inside_hunk() {
        let body = "some text\n\
                     diff --git a/foo b/foo\n\
                     --- a/foo\n\
                     +++ b/foo\n\
                     @@ -1,3 +1,3 @@\n\
                     -old line\n\
                     +new line\n\
                      context\n";
        let lines = highlight_body(body);
        assert_eq!(line_fg(&lines[2]), Some(Color::Red), "--- header");
        assert_eq!(line_fg(&lines[3]), Some(Color::Green), "+++ header");
        assert_eq!(line_fg(&lines[4]), Some(Color::Cyan), "@@ hunk");
        assert_eq!(line_fg(&lines[5]), Some(Color::Red), "- removal");
        assert_eq!(line_fg(&lines[6]), Some(Color::Green), "+ addition");
    }

    #[test]
    fn highlight_dash_outside_diff_is_plain() {
        let body = "- item one\n- item two\n+ not a diff\n";
        let lines = highlight_body(body);
        assert_eq!(line_fg(&lines[0]), None, "- outside diff");
        assert_eq!(line_fg(&lines[1]), None, "- outside diff");
        assert_eq!(line_fg(&lines[2]), None, "+ outside diff");
    }

    #[test]
    fn highlight_diff_ends_at_non_diff_line() {
        let body = "diff --git a/f b/f\n\
                     --- a/f\n\
                     +++ b/f\n\
                     @@ -1 +1 @@\n\
                     -old\n\
                     +new\n\
                     This is a normal paragraph.\n\
                     - this is a list item\n";
        let lines = highlight_body(body);
        assert_eq!(line_fg(&lines[4]), Some(Color::Red), "- in diff");
        assert_eq!(line_fg(&lines[5]), Some(Color::Green), "+ in diff");
        assert_eq!(line_fg(&lines[7]), None, "- after diff ended");
    }

    #[test]
    fn highlight_quoted_text_always_colored() {
        let body = "> quoted reply\n- list item\n";
        let lines = highlight_body(body);
        assert_eq!(line_fg(&lines[0]), Some(Color::Blue), "> quote");
        assert_eq!(line_fg(&lines[1]), None, "- outside diff");
    }

    // ── extract_body ────────────────────────────────────────────────────────

    fn parse_msg(raw: &str) -> mail_parser::Message<'_> {
        mail_parser::MessageParser::default()
            .parse(raw.as_bytes())
            .unwrap()
    }

    #[test]
    fn extract_body_plain_text() {
        let raw = concat!(
            "From: a@x.com\r\n",
            "To: b@x.com\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "Hello plain",
        );
        let msg = parse_msg(raw);
        let body = extract_body(&msg);
        assert!(body.contains("Hello plain"), "got: {body}");
    }

    #[test]
    fn extract_body_html_shows_links() {
        let raw = concat!(
            "From: a@x.com\r\n",
            "To: b@x.com\r\n",
            "Content-Type: text/html\r\n",
            "\r\n",
            "<p>Click <a href=\"https://example.com\">here</a></p>",
        );
        let msg = parse_msg(raw);
        let body = extract_body(&msg);
        assert!(body.contains("[1]: https://example.com"), "got: {body}");
    }

    #[test]
    fn extract_body_prefers_plain_over_html() {
        let raw = concat!(
            "From: a@x.com\r\n",
            "To: b@x.com\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/alternative; boundary=\"bnd\"\r\n",
            "\r\n",
            "--bnd\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "Plain version\r\n",
            "--bnd\r\n",
            "Content-Type: text/html\r\n",
            "\r\n",
            "<p>HTML <a href=\"https://example.com\">link</a></p>\r\n",
            "--bnd--",
        );
        let msg = parse_msg(raw);
        let body = extract_body(&msg);
        assert!(body.contains("Plain version"), "got: {body}");
    }
}
