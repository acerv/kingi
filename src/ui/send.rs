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

pub fn send_message(smtp: &Smtp, draft: &Draft) -> anyhow::Result<()> {
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{SmtpTransport, Transport};

    let from = Address::new(smtp.name.as_deref().unwrap_or(""), &smtp.username);
    let email = draft.build_message(&from.full(), false)?;

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
