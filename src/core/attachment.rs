// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use anyhow::{Context, Result, ensure};
use mail_parser::{Message, MimeHeaders};
use std::io::{Read, Write};
use std::path::Path;

#[derive(Clone)]
pub struct Attachment {
    pub name: String,
    pub data: Vec<u8>,
    encoding_problem: bool,
}

impl Attachment {
    pub fn from_file(path: &Path) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)
                .with_context(|| format!("cannot inspect {}", path.display()))?
                .is_file(),
            "attachment must be a regular file"
        );
        let mut file =
            std::fs::File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        ensure!(
            file.metadata()?.is_file(),
            "attachment must be a regular file"
        );
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.chars().any(char::is_control))
            .ok_or_else(|| anyhow::anyhow!("invalid attachment filename"))?
            .to_string();
        let mut data = Vec::new();
        file.read_to_end(&mut data)
            .with_context(|| format!("cannot read {}", path.display()))?;
        Ok(Self {
            name,
            data,
            encoding_problem: false,
        })
    }

    pub fn mime_part(&self) -> Result<lettre::message::SinglePart> {
        ensure!(!self.encoding_problem, "attachment could not be decoded");
        Ok(lettre::message::Attachment::new(self.name.clone()).body(
            self.data.clone(),
            lettre::message::header::ContentType::parse("application/octet-stream")?,
        ))
    }

    pub fn from_message(msg: &Message) -> Vec<Self> {
        msg.attachments()
            .enumerate()
            .map(|(i, part)| {
                // MIME filenames are untrusted, including Windows-style paths.
                let name: String = part
                    .attachment_name()
                    .unwrap_or("")
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or("")
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                let name = if name.trim().is_empty() || name == "." || name == ".." {
                    format!("attachment-{}", i + 1)
                } else {
                    name
                };
                Self {
                    name,
                    data: part.contents().to_vec(),
                    encoding_problem: part.is_encoding_problem,
                }
            })
            .collect()
    }

    /// Save without replacing existing files or following existing symlinks.
    pub fn save(&self, path: &Path) -> Result<()> {
        ensure!(!self.encoding_problem, "attachment could not be decoded");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| format!("cannot create {}", path.display()))?;
        if let Err(err) = file.write_all(&self.data).and_then(|_| file.sync_all()) {
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(err).with_context(|| format!("cannot save {}", path.display()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mail_parser::MessageParser;

    #[test]
    fn decode_and_save_attachments_without_overwriting() {
        let raw = b"MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\n\r\nBody\r\n--x\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"../../report.pdf\"\r\nContent-Transfer-Encoding: base64\r\n\r\nAAEC/w==\r\n--x\r\nContent-Type: text/plain\r\nContent-Disposition: attachment\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nhello=20world\r\n--x--\r\n";
        let msg = MessageParser::default().parse(raw).unwrap();
        let attachments = Attachment::from_message(&msg);
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].name, "report.pdf");
        assert_eq!(attachments[0].data, [0, 1, 2, 255]);
        assert_eq!(attachments[1].name, "attachment-2");
        assert_eq!(attachments[1].data, b"hello world");

        let path = std::env::temp_dir().join(format!(
            "kingi-attachment-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        attachments[0].save(&path).unwrap();
        assert!(attachments[1].save(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), [0, 1, 2, 255]);
        std::fs::remove_file(path).unwrap();
    }
}
