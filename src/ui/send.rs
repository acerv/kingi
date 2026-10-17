// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::address::Address;
use crate::core::config::Smtp;
use crate::ui::compose::Draft;
use anyhow::anyhow;

#[derive(Clone, PartialEq)]
pub enum SendAction {
    Send,
    SaveDraft,
    Discard,
}

impl SendAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Send => "Send Email",
            Self::SaveDraft => "Save as draft",
            Self::Discard => "Discard",
        }
    }
}

const SMTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

pub fn send_message(smtp: &Smtp, draft: &Draft) -> anyhow::Result<()> {
    send_message_with_timeout(smtp, draft, SMTP_TIMEOUT)
}

fn send_message_with_timeout(
    smtp: &Smtp,
    draft: &Draft,
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{SmtpTransport, Transport};
    use std::sync::mpsc;

    let from = Address::new(smtp.name.as_deref().unwrap_or(""), &smtp.username);
    let email = draft.build_message(&from.full(), false)?;

    let host = smtp.host.clone();
    let port = smtp.port;
    let creds = Credentials::new(smtp.username.clone(), smtp.password.clone());

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let res = (|| -> anyhow::Result<()> {
            let transport = SmtpTransport::starttls_relay(&host)
                .map_err(|e| anyhow!("SMTP relay error: {e}"))?
                .port(port)
                .credentials(creds)
                .timeout(Some(timeout))
                .build();

            transport
                .send(&email)
                .map_err(|e| anyhow!("SMTP send error: {e}"))?;

            Ok(())
        })();
        let _ = tx.send(res);
    });

    match rx.recv_timeout(timeout) {
        Ok(res) => res,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(anyhow!("SMTP send error: connection timed out"))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(anyhow!("SMTP send error: worker thread terminated"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::{Duration, Instant};

    #[test]
    fn send_message_times_out_when_connection_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Accept one connection and hold it open without speaking SMTP.
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(2));
        });

        let smtp = Smtp {
            host: "127.0.0.1".to_string(),
            port,
            username: "me@example.com".to_string(),
            name: None,
            password: "secret".to_string(),
        };
        let draft = Draft::parse("To: bob@example.com\nSubject: Hi\n--- body ---\nHello");

        let start = Instant::now();
        let err = send_message_with_timeout(&smtp, &draft, Duration::from_millis(150)).unwrap_err();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "send_message took too long: {:?}",
            start.elapsed()
        );
        assert!(
            err.to_string().contains("SMTP send error"),
            "unexpected error: {err:#}"
        );
        let _ = server.join();
    }
}
