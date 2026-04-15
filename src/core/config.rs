// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
pub struct Mailbox {
    pub label: String,
    pub path: String,
}

impl Mailbox {
    pub fn is_drafts(&self) -> bool {
        self.label.to_lowercase() == "drafts"
    }

    pub fn is_trash(&self) -> bool {
        self.label.to_lowercase() == "trash"
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Smtp {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub name: Option<String>,
    pub password: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct Sync {
    pub command: String,
    pub interval: u64,
}

#[derive(Debug, Deserialize, Default)]
pub struct Gpg {
    pub binary: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(rename = "mailbox")]
    pub mailboxes: Vec<Mailbox>,
    pub smtp: Smtp,
    pub sync: Option<Sync>,
    pub gpg: Option<Gpg>,
}

impl Config {
    /// Load and parse the config file.
    ///
    /// Looks for the file at `$XDG_CONFIG_HOME/kingi/config.toml`,
    /// falling back to `~/.config/kingi/config.toml`.
    pub fn load() -> Result<Self> {
        let path = config_dir().join("config.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read config file: {}", path.display()))?;
        let mut config: Config = toml::from_str(&text)
            .with_context(|| format!("cannot parse config file: {}", path.display()))?;
        config.ensure_defaults();
        Ok(config)
    }

    /// Return the configured GPG binary, defaulting to `"gpg"`.
    pub fn gpg_binary(&self) -> &str {
        self.gpg
            .as_ref()
            .and_then(|g| g.binary.as_deref())
            .unwrap_or("gpg")
    }

    /// Ensure that Drafts and Trash mailboxes are always present.
    /// If not explicitly configured, they default to subdirectories
    /// of the kingi config directory.
    fn ensure_defaults(&mut self) {
        let cfg_dir = config_dir();

        if !self.mailboxes.iter().any(|mb| mb.is_drafts()) {
            self.mailboxes.push(Mailbox {
                label: "Drafts".to_string(),
                path: cfg_dir.join("drafts").to_string_lossy().into_owned(),
            });
        }

        if !self.mailboxes.iter().any(|mb| mb.is_trash()) {
            self.mailboxes.push(Mailbox {
                label: "Trash".to_string(),
                path: cfg_dir.join("trash").to_string_lossy().into_owned(),
            });
        }
    }
}

/// Read `signature` from the kingi config directory if it exists.
pub fn load_signature() -> Option<String> {
    std::fs::read_to_string(config_dir().join("signature")).ok()
}

/// The kingi configuration directory (`$XDG_CONFIG_HOME/kingi` or
/// `~/.config/kingi`).
pub fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".config"));
    base.join("kingi")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("kingi_config_test_{}_{}", pid, id));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn with_xdg<F: FnOnce()>(dir: &PathBuf, f: F) {
        unsafe { std::env::set_var("XDG_CONFIG_HOME", dir) };
        f();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    }

    const VALID_TOML: &str = r#"
[[mailbox]]
label = "Inbox"
path = "/home/user/mail/inbox"

[smtp]
host = "smtp.example.com"
port = 587
username = "user@example.com"
password = "secret"
"#;

    #[test]
    fn config_dir_uses_xdg_config_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        with_xdg(&dir, || {
            assert_eq!(config_dir(), dir.join("kingi"));
        });
    }

    #[test]
    fn config_dir_falls_back_to_home() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let old_home = std::env::var("HOME").ok();
        unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
        unsafe { std::env::set_var("HOME", &dir) };
        let result = config_dir();
        if let Some(h) = old_home {
            unsafe { std::env::set_var("HOME", h) };
        }
        assert_eq!(result, dir.join(".config").join("kingi"));
    }

    #[test]
    fn load_signature_returns_none_when_missing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        with_xdg(&dir, || {
            assert!(load_signature().is_none());
        });
    }

    #[test]
    fn load_signature_returns_content() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("signature"), "-- \nBest regards").unwrap();
        with_xdg(&dir, || {
            assert_eq!(load_signature(), Some("-- \nBest regards".to_string()));
        });
    }

    #[test]
    fn config_load_success() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("config.toml"), VALID_TOML).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(config.mailboxes[0].label, "Inbox");
            assert_eq!(config.mailboxes[0].path, "/home/user/mail/inbox");
            assert_eq!(config.smtp.host, "smtp.example.com");
            assert_eq!(config.smtp.port, 587);
            assert_eq!(config.smtp.username, "user@example.com");
            assert_eq!(config.smtp.password, "secret");
            assert!(config.smtp.name.is_none());
            assert!(config.sync.is_none());
            // Drafts and Trash are always appended when missing
            assert!(config.mailboxes.iter().any(|mb| mb.is_drafts()));
            assert!(config.mailboxes.iter().any(|mb| mb.is_trash()));
        });
    }

    #[test]
    fn config_load_with_sync() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        let toml = format!("{VALID_TOML}\n[sync]\ncommand = \"mbsync -a\"\ninterval = 300\n");
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let sync = Config::load().unwrap().sync.unwrap();
            assert_eq!(sync.command, "mbsync -a");
            assert_eq!(sync.interval, 300);
        });
    }

    #[test]
    fn config_load_multiple_mailboxes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        let toml = r#"
[[mailbox]]
label = "Inbox"
path = "/mail/inbox"

[[mailbox]]
label = "Sent"
path = "/mail/sent"

[smtp]
host = "smtp.example.com"
port = 465
username = "user"
password = "pass"
"#;
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(config.mailboxes[0].label, "Inbox");
            assert_eq!(config.mailboxes[1].label, "Sent");
            assert_eq!(config.mailboxes[1].path, "/mail/sent");
            assert!(config.mailboxes.iter().any(|mb| mb.is_drafts()));
            assert!(config.mailboxes.iter().any(|mb| mb.is_trash()));
        });
    }

    #[test]
    fn config_load_smtp_optional_name() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        let toml = r#"
[[mailbox]]
label = "Inbox"
path = "/mail/inbox"

[smtp]
host = "smtp.example.com"
port = 587
username = "user"
name = "Alice"
password = "pass"
"#;
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(config.smtp.name, Some("Alice".to_string()));
        });
    }

    #[test]
    fn config_load_missing_file_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        with_xdg(&dir, || {
            let err = Config::load().unwrap_err();
            assert!(err.to_string().contains("cannot read config file"));
        });
    }

    // ── Mailbox::is_drafts ────────────────────────────────────────────────────

    fn mb(label: &str) -> Mailbox {
        Mailbox {
            label: label.to_string(),
            path: String::new(),
        }
    }

    #[test]
    fn is_drafts_matches_exact_case() {
        assert!(mb("Drafts").is_drafts());
    }

    #[test]
    fn is_drafts_matches_lowercase() {
        assert!(mb("drafts").is_drafts());
    }

    #[test]
    fn is_drafts_matches_uppercase() {
        assert!(mb("DRAFTS").is_drafts());
    }

    #[test]
    fn is_drafts_does_not_match_other_labels() {
        assert!(!mb("Inbox").is_drafts());
        assert!(!mb("Sent").is_drafts());
    }

    // ── Mailbox::is_trash ─────────────────────────────────────────────────────

    #[test]
    fn is_trash_matches_exact_case() {
        assert!(mb("Trash").is_trash());
    }

    #[test]
    fn is_trash_matches_lowercase() {
        assert!(mb("trash").is_trash());
    }

    #[test]
    fn is_trash_matches_uppercase() {
        assert!(mb("TRASH").is_trash());
    }

    #[test]
    fn is_trash_does_not_match_other_labels() {
        assert!(!mb("Inbox").is_trash());
        assert!(!mb("Drafts").is_trash());
    }

    // ── ensure_defaults ────────────────────────────────────────────────────

    #[test]
    fn ensure_defaults_adds_drafts_and_trash() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("config.toml"), VALID_TOML).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let drafts = config.mailboxes.iter().find(|mb| mb.is_drafts()).unwrap();
            let trash = config.mailboxes.iter().find(|mb| mb.is_trash()).unwrap();
            assert_eq!(drafts.path, kingi_dir.join("drafts").to_string_lossy());
            assert_eq!(trash.path, kingi_dir.join("trash").to_string_lossy());
        });
    }

    #[test]
    fn ensure_defaults_does_not_duplicate_configured_drafts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        let toml =
            format!("{VALID_TOML}\n[[mailbox]]\nlabel = \"Drafts\"\npath = \"/mail/drafts\"\n");
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let count = config.mailboxes.iter().filter(|mb| mb.is_drafts()).count();
            assert_eq!(count, 1);
            let drafts = config.mailboxes.iter().find(|mb| mb.is_drafts()).unwrap();
            assert_eq!(drafts.path, "/mail/drafts");
        });
    }

    #[test]
    fn ensure_defaults_does_not_duplicate_configured_trash() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        let toml =
            format!("{VALID_TOML}\n[[mailbox]]\nlabel = \"Trash\"\npath = \"/mail/trash\"\n");
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let count = config.mailboxes.iter().filter(|mb| mb.is_trash()).count();
            assert_eq!(count, 1);
            let trash = config.mailboxes.iter().find(|mb| mb.is_trash()).unwrap();
            assert_eq!(trash.path, "/mail/trash");
        });
    }

    #[test]
    fn config_load_invalid_toml_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("config.toml"), "not valid toml {{{").unwrap();
        with_xdg(&dir, || {
            let err = Config::load().unwrap_err();
            assert!(err.to_string().contains("cannot parse config file"));
        });
    }
}
