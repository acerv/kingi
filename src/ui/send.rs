// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::address::Address;
use crate::core::config::Smtp;
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

pub fn send_message(
    smtp: &Smtp,
    to: &[Address],
    cc: &[Address],
    subject: &str,
    body: &str,
    in_reply_to: Option<&str>,
) -> anyhow::Result<()> {
    use lettre::message::header::ContentType;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let from = Address::new(smtp.name.as_deref().unwrap_or(""), &smtp.username);
    let mut builder = Message::builder().from(
        from.full()
            .parse()
            .map_err(|e| anyhow!("invalid From address: {e}"))?,
    );
    for addr in to {
        builder = builder.to(addr
            .full()
            .parse()
            .map_err(|e| anyhow!("invalid To address {addr}: {e}"))?);
    }
    for addr in cc {
        builder = builder.cc(addr
            .full()
            .parse()
            .map_err(|e| anyhow!("invalid Cc address {addr}: {e}"))?);
    }
    if let Some(irt) = in_reply_to {
        builder = builder.in_reply_to(irt.to_string());
    }
    let email = builder
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())
        .map_err(|e| anyhow!("failed to build message: {e}"))?;

    let creds = Credentials::new(smtp.username.clone(), smtp.password.clone());
    let transport = SmtpTransport::starttls_relay(&smtp.host)
        .map_err(|e| anyhow!("SMTP relay error: {e}"))?
        .port(smtp.port)
        .credentials(creds)
        .build();

    transport
        .send(&email)
        .map_err(|e| anyhow!("SMTP send error: {e}"))?;

    Ok(())
}
