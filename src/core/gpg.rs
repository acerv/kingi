// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use anyhow::{Context, Result, anyhow};
use mail_parser::{Message, MessageParser, MimeHeaders, PartType};
use std::io::Write;
use std::process::{Command, Stdio};

/// Crypto status displayed in the email view header.
#[derive(Debug, PartialEq)]
pub enum CryptoStatus {
    None,
    Decrypted,
    Signed(VerifyResult),
    DecryptedAndSigned(VerifyResult),
    DecryptFailed(String),
    VerifyFailed(String),
}

/// Result of a PGP signature verification.
#[derive(Debug, PartialEq)]
pub enum VerifyResult {
    Good { signer: String },
    Bad { signer: String },
    Unknown,
}

/// PGP/MIME message type detected from Content-Type.
#[derive(Debug, PartialEq)]
pub enum PgpMimeType {
    Encrypted,
    Signed,
}

/// Inline PGP type detected from body markers.
#[derive(Debug, PartialEq)]
pub enum InlinePgpType {
    Encrypted,
    Signed,
}

// ── Detection ────────────────────────────────────────────────────────────────

/// Check the root Content-Type for PGP/MIME (RFC 3156).
///
/// Returns `Some(Encrypted)` for `multipart/encrypted; protocol="application/pgp-encrypted"`
/// and `Some(Signed)` for `multipart/signed; protocol="application/pgp-signature"`.
pub fn detect_pgp_mime(msg: &Message) -> Option<PgpMimeType> {
    let ct = msg.content_type()?;
    if !ct.c_type.eq_ignore_ascii_case("multipart") {
        return None;
    }
    let sub = ct.c_subtype.as_ref()?;
    let proto = ct.attribute("protocol")?;
    if sub.eq_ignore_ascii_case("encrypted")
        && proto.eq_ignore_ascii_case("application/pgp-encrypted")
    {
        Some(PgpMimeType::Encrypted)
    } else if sub.eq_ignore_ascii_case("signed")
        && proto.eq_ignore_ascii_case("application/pgp-signature")
    {
        Some(PgpMimeType::Signed)
    } else {
        None
    }
}

/// Check the root Content-Type of a header-only parse to determine if the
/// email is PGP/MIME encrypted.  Used for the thread-list lock icon.
pub fn is_pgp_mime_encrypted(msg: &Message) -> bool {
    matches!(detect_pgp_mime(msg), Some(PgpMimeType::Encrypted))
}

/// Scan the body text for inline PGP markers.
pub fn detect_inline_pgp(body: &str) -> Option<InlinePgpType> {
    if body.contains("-----BEGIN PGP MESSAGE-----") {
        Some(InlinePgpType::Encrypted)
    } else if body.contains("-----BEGIN PGP SIGNED MESSAGE-----") {
        Some(InlinePgpType::Signed)
    } else {
        None
    }
}

// ── GPG invocation helpers ───────────────────────────────────────────────────

/// Resolve the current TTY path for `GPG_TTY` so that `gpg-agent` knows
/// which terminal to send `pinentry` to.
fn gpg_tty() -> Option<String> {
    if let Ok(val) = std::env::var("GPG_TTY") {
        return Some(val);
    }
    std::fs::read_link("/proc/self/fd/0")
        .ok()
        .and_then(|p| p.to_str().map(String::from))
}

/// Apply `GPG_TTY` to a [`Command`].
fn apply_gpg_tty(cmd: &mut Command) {
    if let Some(tty) = gpg_tty() {
        cmd.env("GPG_TTY", tty);
    }
}

/// Run gpg with the given args, feeding `input` on stdin.
/// Returns `(stdout, stderr)`.
fn run_gpg(gpg_binary: &str, args: &[&str], input: &[u8]) -> Result<(Vec<u8>, String)> {
    let mut cmd = Command::new(gpg_binary);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    apply_gpg_tty(&mut cmd);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {gpg_binary}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input)
            .context("failed to write to gpg stdin")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait on gpg process")?;

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    if !output.status.success() {
        return Err(anyhow!("{}", friendly_gpg_error(&stderr)));
    }

    Ok((output.stdout, stderr))
}

/// Parse `[GNUPG:]` status lines from stderr to extract verification result.
fn parse_gnupg_status(stderr: &str) -> Option<VerifyResult> {
    for line in stderr.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[GNUPG:] GOODSIG ") {
            // Format: GOODSIG <keyid> <user-id>
            let signer = rest
                .split_once(' ')
                .map(|(_, uid)| uid.to_string())
                .unwrap_or_else(|| rest.to_string());
            return Some(VerifyResult::Good { signer });
        }
        if let Some(rest) = line.strip_prefix("[GNUPG:] BADSIG ") {
            let signer = rest
                .split_once(' ')
                .map(|(_, uid)| uid.to_string())
                .unwrap_or_else(|| rest.to_string());
            return Some(VerifyResult::Bad { signer });
        }
    }
    None
}

// ── Decryption ───────────────────────────────────────────────────────────────

/// Decrypt a PGP/MIME encrypted message (RFC 3156).
///
/// Structure: part 0 = multipart/encrypted container,
///            part 1 = application/pgp-encrypted (version),
///            part 2 = application/octet-stream (ciphertext).
pub fn decrypt_pgp_mime(msg: &Message, gpg_binary: &str) -> Result<(String, CryptoStatus)> {
    let ciphertext = extract_part_bytes(msg, 2)
        .ok_or_else(|| anyhow!("missing encrypted data part in PGP/MIME message"))?;

    let (plaintext_bytes, stderr) = run_gpg(
        gpg_binary,
        &["--decrypt", "--batch", "--status-fd", "2"],
        ciphertext,
    )?;

    // The decrypted output is itself a MIME message; re-parse to extract body.
    let body = extract_body_from_bytes(&plaintext_bytes)?;

    let verify = parse_gnupg_status(&stderr);
    let status = match verify {
        Some(vr) => CryptoStatus::DecryptedAndSigned(vr),
        None => CryptoStatus::Decrypted,
    };

    Ok((body, status))
}

/// Decrypt an inline PGP encrypted message.
///
/// Extracts the PGP block, decrypts it, and replaces it in the original body.
pub fn decrypt_inline_pgp(body: &str, gpg_binary: &str) -> Result<(String, CryptoStatus)> {
    let (before, block, after) = extract_inline_block(
        body,
        "-----BEGIN PGP MESSAGE-----",
        "-----END PGP MESSAGE-----",
    )
    .ok_or_else(|| anyhow!("no PGP message block found in body"))?;

    let (plaintext_bytes, stderr) = run_gpg(
        gpg_binary,
        &["--decrypt", "--batch", "--status-fd", "2"],
        block.as_bytes(),
    )?;

    let plaintext = String::from_utf8_lossy(&plaintext_bytes);
    let result = format!("{before}{plaintext}{after}");

    let verify = parse_gnupg_status(&stderr);
    let status = match verify {
        Some(vr) => CryptoStatus::DecryptedAndSigned(vr),
        None => CryptoStatus::Decrypted,
    };

    Ok((result, status))
}

// ── Verification ─────────────────────────────────────────────────────────────

/// Verify a PGP/MIME signed message (RFC 3156).
///
/// Structure: part 0 = multipart/signed container,
///            part 1 = the signed content (text/plain or nested),
///            part 2 = application/pgp-signature (detached sig).
pub fn verify_pgp_mime(msg: &Message, gpg_binary: &str) -> Result<(String, CryptoStatus)> {
    // Extract the signed content body for display.
    let body = extract_signed_body(msg)?;

    // For detached signature verification we need the raw signed part and the
    // signature.  The signed data is the raw bytes of part 1 (including its
    // MIME headers), and the signature is part 2.
    let sig_bytes = extract_part_bytes(msg, 2)
        .ok_or_else(|| anyhow!("missing signature part in PGP/MIME signed message"))?;

    // Extract the raw bytes of the signed part from the original message.
    let signed_bytes = extract_raw_part_bytes(msg, 1)
        .ok_or_else(|| anyhow!("missing signed data part in PGP/MIME message"))?;

    // Write signed data to a temp file, signature to another, then verify.
    let tmp_dir = std::env::temp_dir();
    let signed_path = tmp_dir.join(format!("kingi-signed-{}", std::process::id()));
    let sig_path = tmp_dir.join(format!("kingi-sig-{}", std::process::id()));

    std::fs::write(&signed_path, signed_bytes).context("failed to write signed data")?;
    std::fs::write(&sig_path, sig_bytes).context("failed to write signature")?;

    let mut cmd = Command::new(gpg_binary);
    cmd.args([
        "--verify",
        "--batch",
        "--status-fd",
        "2",
        sig_path.to_str().unwrap_or(""),
        signed_path.to_str().unwrap_or(""),
    ])
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    apply_gpg_tty(&mut cmd);
    let result = cmd
        .output()
        .with_context(|| format!("failed to spawn {gpg_binary}"));

    let _ = std::fs::remove_file(&signed_path);
    let _ = std::fs::remove_file(&sig_path);

    let output = result?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    let verify = parse_gnupg_status(&stderr).unwrap_or(VerifyResult::Unknown);
    Ok((body, CryptoStatus::Signed(verify)))
}

/// Verify an inline PGP signed message (clearsigned).
///
/// Extracts the cleartext and verifies the signature.
pub fn verify_inline_pgp(body: &str, gpg_binary: &str) -> Result<(String, CryptoStatus)> {
    let (before, block, after) = extract_inline_block(
        body,
        "-----BEGIN PGP SIGNED MESSAGE-----",
        "-----END PGP SIGNATURE-----",
    )
    .ok_or_else(|| anyhow!("no PGP signed message block found in body"))?;

    // gpg --verify reads the clearsigned block from stdin and outputs status on
    // stderr.  For clearsigned messages we need to extract the cleartext body
    // ourselves — it sits between the Hash: header and the signature block.
    let cleartext = extract_clearsigned_body(block);

    let output = Command::new(gpg_binary)
        .args(["--verify", "--batch", "--status-fd", "2"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(block.as_bytes());
            }
            child.wait_with_output()
        })
        .with_context(|| format!("failed to run {gpg_binary}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let verify = parse_gnupg_status(&stderr).unwrap_or(VerifyResult::Unknown);
    let result = format!("{before}{cleartext}{after}");

    Ok((result, CryptoStatus::Signed(verify)))
}

// ── Internal helpers ─────────────────────────────────────────────────────────

/// Parse gpg stderr for known `[GNUPG:]` status codes and return a
/// human-readable error message.
fn friendly_gpg_error(stderr: &str) -> String {
    // Collect missing key IDs from NO_SECKEY lines.
    // Format: [GNUPG:] NO_SECKEY <keyid>
    let missing_keys: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.trim().strip_prefix("[GNUPG:] NO_SECKEY "))
        .collect();

    if !missing_keys.is_empty() {
        return format!(
            "No secret key available (need: {})",
            missing_keys.join(", ")
        );
    }

    for line in stderr.lines() {
        let line = line.trim();
        if line.contains("[GNUPG:] BAD_PASSPHRASE") {
            return "Bad passphrase".to_string();
        }
        if line.contains("[GNUPG:] MISSING_PASSPHRASE") {
            return "No passphrase provided (is gpg-agent running?)".to_string();
        }
        if line.contains("[GNUPG:] NO_PUBKEY") {
            if let Some(rest) = line.strip_prefix("[GNUPG:] NO_PUBKEY ") {
                return format!("Public key not found ({})", rest.trim());
            }
            return "Public key not found".to_string();
        }
        if line.contains("[GNUPG:] INV_RECP") {
            return "Invalid recipient".to_string();
        }
        if line.contains("[GNUPG:] DECRYPTION_FAILED") {
            return "Decryption failed".to_string();
        }
        if line.contains("[GNUPG:] NODATA") {
            return "No valid encrypted data found".to_string();
        }
        if line.contains("[GNUPG:] KEYEXPIRED") {
            return "Key has expired".to_string();
        }
        if line.contains("[GNUPG:] KEYREVOKED") {
            return "Key has been revoked".to_string();
        }
    }

    // Fallback: use the last non-empty line from stderr, stripped of [GNUPG:] prefix.
    stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().strip_prefix("[GNUPG:] ").unwrap_or(l.trim()))
        .unwrap_or("Unknown GPG error")
        .to_string()
}

/// Extract the decoded body bytes from a specific MIME part index.
fn extract_part_bytes<'a>(msg: &'a Message, part_idx: usize) -> Option<&'a [u8]> {
    let part = msg.parts.get(part_idx)?;
    match &part.body {
        PartType::Binary(data) | PartType::InlineBinary(data) => Some(data.as_ref()),
        PartType::Text(text) => Some(text.as_bytes()),
        _ => None,
    }
}

/// Extract the raw bytes of a MIME part from the original message using offsets.
fn extract_raw_part_bytes<'a>(msg: &'a Message, part_idx: usize) -> Option<&'a [u8]> {
    let part = msg.parts.get(part_idx)?;
    let start = part.offset_header as usize;
    let end = part.offset_end as usize;
    if start < end && end <= msg.raw_message.len() {
        Some(&msg.raw_message[start..end])
    } else {
        None
    }
}

/// Parse decrypted bytes as a MIME message and extract the text body.
fn extract_body_from_bytes(bytes: &[u8]) -> Result<String> {
    let parsed = MessageParser::default()
        .parse(bytes)
        .ok_or_else(|| anyhow!("failed to parse decrypted message"))?;
    Ok(parsed
        .body_text(0)
        .map(|t| t.into_owned())
        .unwrap_or_default())
}

/// Extract the text body from the signed content part of a PGP/MIME message.
fn extract_signed_body(msg: &Message) -> Result<String> {
    // Part 1 may be a text/plain part directly, or a nested multipart.
    let part = msg
        .parts
        .get(1)
        .ok_or_else(|| anyhow!("missing signed content part"))?;
    match &part.body {
        PartType::Text(text) => Ok(text.to_string()),
        PartType::Multipart(_) => {
            // The signed content is a nested multipart — look for text bodies
            // after part 1 that aren't the signature part (last part).
            for p in &msg.parts[2..msg.parts.len().saturating_sub(1)] {
                if let PartType::Text(text) = &p.body {
                    return Ok(text.to_string());
                }
            }
            Ok(String::new())
        }
        _ => Ok(String::new()),
    }
}

/// Extract `(before, block, after)` from body text given start/end markers.
fn extract_inline_block<'a>(
    body: &'a str,
    begin_marker: &str,
    end_marker: &str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let start = body.find(begin_marker)?;
    let end_start = body[start..].find(end_marker)?;
    let end = start + end_start + end_marker.len();
    Some((&body[..start], &body[start..end], &body[end..]))
}

/// Extract the cleartext body from a clearsigned PGP message.
///
/// The format is:
/// ```text
/// -----BEGIN PGP SIGNED MESSAGE-----
/// Hash: SHA256
///
/// <cleartext body>
/// -----BEGIN PGP SIGNATURE-----
/// ...
/// -----END PGP SIGNATURE-----
/// ```
fn extract_clearsigned_body(block: &str) -> String {
    // Skip the header (everything up to the first blank line after BEGIN).
    let body_start = block
        .find("\n\n")
        .or_else(|| block.find("\r\n\r\n"))
        .map(|pos| {
            if block[pos..].starts_with("\r\n\r\n") {
                pos + 4
            } else {
                pos + 2
            }
        })
        .unwrap_or(0);

    // End at the PGP SIGNATURE marker.
    let body_end = block
        .find("-----BEGIN PGP SIGNATURE-----")
        .unwrap_or(block.len());

    block[body_start..body_end].trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── detect_pgp_mime ──────────────────────────────────────────────────────

    fn parse_msg(raw: &str) -> Message<'static> {
        MessageParser::default()
            .parse(raw.as_bytes())
            .unwrap()
            .into_owned()
    }

    #[test]
    fn detect_pgp_mime_encrypted() {
        let raw = "Content-Type: multipart/encrypted;\r\n \
                    protocol=\"application/pgp-encrypted\";\r\n \
                    boundary=\"abc\"\r\n\r\n\
                    --abc\r\n\
                    Content-Type: application/pgp-encrypted\r\n\r\n\
                    Version: 1\r\n\r\n\
                    --abc\r\n\
                    Content-Type: application/octet-stream\r\n\r\n\
                    encrypted data\r\n\
                    --abc--\r\n";
        let msg = parse_msg(raw);
        assert_eq!(detect_pgp_mime(&msg), Some(PgpMimeType::Encrypted));
        assert!(is_pgp_mime_encrypted(&msg));
    }

    #[test]
    fn detect_pgp_mime_signed() {
        let raw = "Content-Type: multipart/signed;\r\n \
                    protocol=\"application/pgp-signature\";\r\n \
                    micalg=pgp-sha256;\r\n \
                    boundary=\"abc\"\r\n\r\n\
                    --abc\r\n\
                    Content-Type: text/plain\r\n\r\n\
                    hello\r\n\r\n\
                    --abc\r\n\
                    Content-Type: application/pgp-signature\r\n\r\n\
                    sig data\r\n\
                    --abc--\r\n";
        let msg = parse_msg(raw);
        assert_eq!(detect_pgp_mime(&msg), Some(PgpMimeType::Signed));
        assert!(!is_pgp_mime_encrypted(&msg));
    }

    #[test]
    fn detect_pgp_mime_plain_returns_none() {
        let raw = "Content-Type: text/plain\r\n\r\nhello\r\n";
        let msg = parse_msg(raw);
        assert_eq!(detect_pgp_mime(&msg), None);
        assert!(!is_pgp_mime_encrypted(&msg));
    }

    // ── detect_inline_pgp ────────────────────────────────────────────────────

    #[test]
    fn detect_inline_encrypted() {
        let body = "before\n-----BEGIN PGP MESSAGE-----\ndata\n-----END PGP MESSAGE-----\nafter";
        assert_eq!(detect_inline_pgp(body), Some(InlinePgpType::Encrypted));
    }

    #[test]
    fn detect_inline_signed() {
        let body = "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\nhello\n-----BEGIN PGP SIGNATURE-----\nsig\n-----END PGP SIGNATURE-----";
        assert_eq!(detect_inline_pgp(body), Some(InlinePgpType::Signed));
    }

    #[test]
    fn detect_inline_plain_returns_none() {
        assert_eq!(detect_inline_pgp("just a normal email body"), None);
    }

    // ── parse_gnupg_status ───────────────────────────────────────────────────

    #[test]
    fn parse_good_signature() {
        let stderr = "[GNUPG:] SIG_ID abc 2024-01-01\n\
                       [GNUPG:] GOODSIG ABCD1234 Alice <alice@example.com>\n\
                       [GNUPG:] VALIDSIG ABCD1234\n";
        let result = parse_gnupg_status(stderr);
        assert_eq!(
            result,
            Some(VerifyResult::Good {
                signer: "Alice <alice@example.com>".to_string()
            })
        );
    }

    #[test]
    fn parse_bad_signature() {
        let stderr = "[GNUPG:] BADSIG ABCD1234 Bob <bob@example.com>\n";
        let result = parse_gnupg_status(stderr);
        assert_eq!(
            result,
            Some(VerifyResult::Bad {
                signer: "Bob <bob@example.com>".to_string()
            })
        );
    }

    #[test]
    fn parse_no_status_returns_none() {
        assert_eq!(parse_gnupg_status("some other output"), None);
    }

    // ── extract_inline_block ─────────────────────────────────────────────────

    #[test]
    fn extract_block_with_surrounding_text() {
        let body = "before\n-----BEGIN PGP MESSAGE-----\ndata\n-----END PGP MESSAGE-----\nafter";
        let (before, block, after) = extract_inline_block(
            body,
            "-----BEGIN PGP MESSAGE-----",
            "-----END PGP MESSAGE-----",
        )
        .unwrap();
        assert_eq!(before, "before\n");
        assert_eq!(
            block,
            "-----BEGIN PGP MESSAGE-----\ndata\n-----END PGP MESSAGE-----"
        );
        assert_eq!(after, "\nafter");
    }

    #[test]
    fn extract_block_missing_returns_none() {
        assert!(
            extract_inline_block(
                "no pgp here",
                "-----BEGIN PGP MESSAGE-----",
                "-----END PGP MESSAGE-----"
            )
            .is_none()
        );
    }

    // ── extract_clearsigned_body ─────────────────────────────────────────────

    #[test]
    fn clearsigned_body_extracted() {
        let block = "-----BEGIN PGP SIGNED MESSAGE-----\n\
                      Hash: SHA256\n\n\
                      Hello, world!\n\
                      Second line.\n\
                      -----BEGIN PGP SIGNATURE-----\n\
                      iQEzBAEB...\n\
                      -----END PGP SIGNATURE-----";
        let body = extract_clearsigned_body(block);
        assert_eq!(body, "Hello, world!\nSecond line.");
    }

    #[test]
    fn clearsigned_body_empty_when_no_content() {
        let block = "-----BEGIN PGP SIGNED MESSAGE-----\n\
                      Hash: SHA256\n\n\
                      -----BEGIN PGP SIGNATURE-----\n\
                      sig\n\
                      -----END PGP SIGNATURE-----";
        let body = extract_clearsigned_body(block);
        assert!(body.is_empty());
    }

    // ── extract_body_from_bytes ──────────────────────────────────────────────

    #[test]
    fn body_from_plain_text_bytes() {
        let raw = b"Content-Type: text/plain\r\n\r\nDecrypted content here\r\n";
        let body = extract_body_from_bytes(raw).unwrap();
        assert_eq!(body, "Decrypted content here\r\n");
    }

    #[test]
    fn body_from_empty_message() {
        let raw = b"Content-Type: text/plain\r\n\r\n";
        let body = extract_body_from_bytes(raw).unwrap();
        assert!(body.is_empty());
    }

    // ── friendly_gpg_error ──────────────────────────────────────────────────

    #[test]
    fn friendly_error_no_secret_key_single() {
        let stderr = "[GNUPG:] ENC_TO ABCD1234 1 0\n\
                       [GNUPG:] NO_SECKEY ABCD1234\n\
                       [GNUPG:] FAILURE gpg-exit 33554433\n";
        assert_eq!(
            friendly_gpg_error(stderr),
            "No secret key available (need: ABCD1234)"
        );
    }

    #[test]
    fn friendly_error_no_secret_key_multiple() {
        let stderr = "[GNUPG:] ENC_TO AAAA1111 1 0\n\
                       [GNUPG:] NO_SECKEY AAAA1111\n\
                       [GNUPG:] ENC_TO BBBB2222 1 0\n\
                       [GNUPG:] NO_SECKEY BBBB2222\n\
                       [GNUPG:] FAILURE gpg-exit 33554433\n";
        assert_eq!(
            friendly_gpg_error(stderr),
            "No secret key available (need: AAAA1111, BBBB2222)"
        );
    }

    #[test]
    fn friendly_error_bad_passphrase() {
        let stderr = "[GNUPG:] BAD_PASSPHRASE ABCD1234\n\
                       [GNUPG:] FAILURE gpg-exit 2\n";
        assert_eq!(friendly_gpg_error(stderr), "Bad passphrase");
    }

    #[test]
    fn friendly_error_missing_passphrase() {
        let stderr = "[GNUPG:] MISSING_PASSPHRASE\n\
                       [GNUPG:] FAILURE gpg-exit 2\n";
        assert_eq!(
            friendly_gpg_error(stderr),
            "No passphrase provided (is gpg-agent running?)"
        );
    }

    #[test]
    fn friendly_error_no_pubkey() {
        let stderr = "[GNUPG:] NO_PUBKEY ABCD1234\n";
        assert_eq!(
            friendly_gpg_error(stderr),
            "Public key not found (ABCD1234)"
        );
    }

    #[test]
    fn friendly_error_fallback_strips_prefix() {
        let stderr = "[GNUPG:] FAILURE gpg-exit 33554433\n";
        assert_eq!(friendly_gpg_error(stderr), "FAILURE gpg-exit 33554433");
    }

    #[test]
    fn friendly_error_fallback_unknown() {
        assert_eq!(friendly_gpg_error(""), "Unknown GPG error");
    }
}
