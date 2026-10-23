// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
pub struct Mailbox {
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub markers: bool,
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
    pub timeout: Option<u64>,
}

impl Sync {
    /// Return the sync timeout duration, defaulting to 60 seconds.
    pub fn timeout_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.timeout.unwrap_or(60))
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Gpg {
    pub binary: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Status {
    #[serde(default)]
    pub merged_markers: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AccountFile {
    #[serde(rename = "mailbox")]
    mailboxes: Vec<Mailbox>,
    smtp: Smtp,
}

#[derive(Debug, Clone)]
pub struct Account {
    pub name: String,
    pub dir: PathBuf,
    pub mailboxes: Vec<Mailbox>,
    pub smtp: Smtp,
}

impl Account {
    /// Ensure that Drafts and Trash mailboxes are always present for this
    /// account. If not explicitly configured, they default to subdirectories
    /// of the account directory.
    pub fn ensure_defaults(&mut self) {
        if !self.mailboxes.iter().any(|mb| mb.is_drafts()) {
            self.mailboxes.push(Mailbox {
                label: "Drafts".to_string(),
                path: self.dir.join("drafts").to_string_lossy().into_owned(),
                markers: false,
            });
        }

        if !self.mailboxes.iter().any(|mb| mb.is_trash()) {
            self.mailboxes.push(Mailbox {
                label: "Trash".to_string(),
                path: self.dir.join("trash").to_string_lossy().into_owned(),
                markers: false,
            });
        }
    }

    /// Read `signature` from this account's directory, falling back to the
    /// global config directory if present.
    pub fn load_signature(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join("signature"))
            .ok()
            .or_else(load_signature)
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    pub sync: Option<Sync>,
    pub gpg: Option<Gpg>,
    pub status: Option<Status>,
    #[serde(skip)]
    pub accounts: Vec<Account>,
}

impl Config {
    /// Load and parse the global config file and all account directories.
    ///
    /// Looks for global settings at `$XDG_CONFIG_HOME/kingi/config.toml`
    /// (optional if absent) and accounts in
    /// `$XDG_CONFIG_HOME/kingi/accounts/<name>/config.toml`.
    pub fn load() -> Result<Self> {
        let cfg_dir = config_dir();
        let path = cfg_dir.join("config.toml");
        let mut config: Config = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .with_context(|| format!("cannot read config file: {}", path.display()))?;
            toml::from_str(&text)
                .with_context(|| format!("cannot parse config file: {}", path.display()))?
        } else {
            Config::default()
        };

        let accounts_dir = cfg_dir.join("accounts");
        let entries = std::fs::read_dir(&accounts_dir).with_context(|| {
            format!(
                "cannot read accounts directory: {}",
                accounts_dir.display()
            )
        })?;

        let mut dirs: Vec<(String, PathBuf)> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect();
        dirs.sort_by(|a, b| a.0.cmp(&b.0));

        for (name, dir) in dirs {
            let account_path = dir.join("config.toml");
            let text = std::fs::read_to_string(&account_path).with_context(|| {
                format!("cannot read account file: {}", account_path.display())
            })?;
            let parsed: AccountFile = toml::from_str(&text).with_context(|| {
                format!("cannot parse account file: {}", account_path.display())
            })?;
            let mut account = Account {
                name,
                dir,
                mailboxes: parsed.mailboxes,
                smtp: parsed.smtp,
            };
            account.ensure_defaults();
            config.accounts.push(account);
        }

        if config.accounts.is_empty() {
            anyhow::bail!("no accounts found in {}", accounts_dir.display());
        }

        Ok(config)
    }

    /// Return the configured GPG binary, defaulting to `"gpg"`.
    pub fn gpg_binary(&self) -> &str {
        self.gpg
            .as_ref()
            .and_then(|g| g.binary.as_deref())
            .unwrap_or("gpg")
    }

    /// Return custom merged-patch regex markers, if configured.
    pub fn merged_markers(&self) -> Vec<String> {
        self.status
            .as_ref()
            .map(|s| s.merged_markers.clone())
            .unwrap_or_default()
    }
}

/// Read `signature` from the kingi config directory if it exists.
pub fn load_signature() -> Option<String> {
    std::fs::read_to_string(config_dir().join("signature")).ok()
}

/// Read `reply-N` template from the kingi config directory if it exists.
pub fn load_reply_template(n: u8) -> Option<String> {
    std::fs::read_to_string(config_dir().join(format!("reply-{n}"))).ok()
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

    const VALID_ACCOUNT_TOML: &str = r#"
[[mailbox]]
label = "Inbox"
path = "/home/user/mail/inbox"

[smtp]
host = "smtp.example.com"
port = 587
username = "user@example.com"
password = "secret"
"#;

    fn write_account(kingi_dir: &PathBuf, name: &str, toml: &str) -> PathBuf {
        let acct_dir = kingi_dir.join("accounts").join(name);
        fs::create_dir_all(&acct_dir).unwrap();
        fs::write(acct_dir.join("config.toml"), toml).unwrap();
        acct_dir
    }

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
    fn account_load_signature_prefers_account_file() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        let acct_dir = write_account(&kingi_dir, "work", VALID_ACCOUNT_TOML);
        fs::write(kingi_dir.join("signature"), "global sig").unwrap();
        fs::write(acct_dir.join("signature"), "work sig").unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(
                config.accounts[0].load_signature().as_deref(),
                Some("work sig")
            );
        });
    }

    #[test]
    fn config_load_success() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        write_account(&kingi_dir, "personal", VALID_ACCOUNT_TOML);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(config.accounts.len(), 1);
            let acct = &config.accounts[0];
            assert_eq!(acct.name, "personal");
            assert_eq!(acct.mailboxes[0].label, "Inbox");
            assert_eq!(acct.mailboxes[0].path, "/home/user/mail/inbox");
            assert_eq!(acct.smtp.host, "smtp.example.com");
            assert_eq!(acct.smtp.port, 587);
            assert_eq!(acct.smtp.username, "user@example.com");
            assert_eq!(acct.smtp.password, "secret");
            assert!(acct.smtp.name.is_none());
            assert!(config.sync.is_none());
            // Drafts and Trash are always appended when missing
            assert!(acct.mailboxes.iter().any(|mb| mb.is_drafts()));
            assert!(acct.mailboxes.iter().any(|mb| mb.is_trash()));
        });
    }

    #[test]
    fn config_load_sorts_accounts_alphabetically() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        write_account(&kingi_dir, "work", VALID_ACCOUNT_TOML);
        write_account(&kingi_dir, "personal", VALID_ACCOUNT_TOML);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let names: Vec<_> = config.accounts.iter().map(|a| a.name.as_str()).collect();
            assert_eq!(names, vec!["personal", "work"]);
        });
    }

    #[test]
    fn config_load_with_sync() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        write_account(&kingi_dir, "default", VALID_ACCOUNT_TOML);
        let toml = "[sync]\ncommand = \"mbsync -a\"\ninterval = 300\ntimeout = 45\n";
        fs::write(kingi_dir.join("config.toml"), toml).unwrap();
        with_xdg(&dir, || {
            let sync = Config::load().unwrap().sync.unwrap();
            assert_eq!(sync.command, "mbsync -a");
            assert_eq!(sync.interval, 300);
            assert_eq!(sync.timeout, Some(45));
            assert_eq!(
                sync.timeout_duration(),
                std::time::Duration::from_secs(45)
            );
        });
    }

    #[test]
    fn config_sync_timeout_defaults_to_60() {
        let sync = Sync {
            command: "mbsync -a".to_string(),
            interval: 300,
            timeout: None,
        };
        assert_eq!(
            sync.timeout_duration(),
            std::time::Duration::from_secs(60)
        );
    }

    #[test]
    fn config_load_multiple_mailboxes() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
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
        write_account(&kingi_dir, "default", toml);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let acct = &config.accounts[0];
            assert_eq!(acct.mailboxes[0].label, "Inbox");
            assert_eq!(acct.mailboxes[1].label, "Sent");
            assert_eq!(acct.mailboxes[1].path, "/mail/sent");
            assert!(acct.mailboxes.iter().any(|mb| mb.is_drafts()));
            assert!(acct.mailboxes.iter().any(|mb| mb.is_trash()));
        });
    }

    #[test]
    fn config_load_smtp_optional_name() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
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
        write_account(&kingi_dir, "default", toml);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert_eq!(config.accounts[0].smtp.name, Some("Alice".to_string()));
        });
    }

    #[test]
    fn config_load_missing_accounts_returns_error() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        with_xdg(&dir, || {
            let err = Config::load().unwrap_err();
            assert!(err.to_string().contains("cannot read accounts directory"));
        });
    }

    // ── Mailbox::is_drafts ────────────────────────────────────────────────────

    fn mb(label: &str) -> Mailbox {
        Mailbox {
            label: label.to_string(),
            path: String::new(),
            markers: false,
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
    fn ensure_defaults_adds_drafts_and_trash_in_account_dir() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        let acct_dir = write_account(&kingi_dir, "work", VALID_ACCOUNT_TOML);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let acct = &config.accounts[0];
            let drafts = acct.mailboxes.iter().find(|mb| mb.is_drafts()).unwrap();
            let trash = acct.mailboxes.iter().find(|mb| mb.is_trash()).unwrap();
            assert_eq!(drafts.path, acct_dir.join("drafts").to_string_lossy());
            assert_eq!(trash.path, acct_dir.join("trash").to_string_lossy());
        });
    }

    #[test]
    fn ensure_defaults_does_not_duplicate_configured_drafts() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        let toml = format!(
            "{VALID_ACCOUNT_TOML}\n[[mailbox]]\nlabel = \"Drafts\"\npath = \"/mail/drafts\"\n"
        );
        write_account(&kingi_dir, "work", &toml);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let acct = &config.accounts[0];
            let count = acct.mailboxes.iter().filter(|mb| mb.is_drafts()).count();
            assert_eq!(count, 1);
            let drafts = acct.mailboxes.iter().find(|mb| mb.is_drafts()).unwrap();
            assert_eq!(drafts.path, "/mail/drafts");
        });
    }

    #[test]
    fn ensure_defaults_does_not_duplicate_configured_trash() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        let toml =
            format!("{VALID_ACCOUNT_TOML}\n[[mailbox]]\nlabel = \"Trash\"\npath = \"/mail/trash\"\n");
        write_account(&kingi_dir, "work", &toml);
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            let acct = &config.accounts[0];
            let count = acct.mailboxes.iter().filter(|mb| mb.is_trash()).count();
            assert_eq!(count, 1);
            let trash = acct.mailboxes.iter().find(|mb| mb.is_trash()).unwrap();
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

    #[test]
    fn config_load_markers_and_status() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        let acct_toml = r#"
[[mailbox]]
label = "LKML"
path = "/mail/lkml"
markers = true

[smtp]
host = "smtp.example.com"
port = 587
username = "user"
password = "pass"
"#;
        write_account(&kingi_dir, "kernel", acct_toml);
        let cfg_toml = "[status]\nmerged_markers = [\"applied\", \"merged\"]\n";
        fs::write(kingi_dir.join("config.toml"), cfg_toml).unwrap();
        with_xdg(&dir, || {
            let config = Config::load().unwrap();
            assert!(config.accounts[0].mailboxes[0].markers);
            assert_eq!(config.merged_markers(), vec!["applied", "merged"]);
        });
    }

    // ── load_reply_template ──────────────────────────────────────────────────

    #[test]
    fn load_reply_template_returns_none_when_missing() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        with_xdg(&dir, || {
            assert!(load_reply_template(1).is_none());
        });
    }

    #[test]
    fn load_reply_template_returns_content() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("reply-1"), "Thanks for the patch!").unwrap();
        with_xdg(&dir, || {
            assert_eq!(
                load_reply_template(1),
                Some("Thanks for the patch!".to_string())
            );
        });
    }

    #[test]
    fn load_reply_template_loads_correct_digit() {
        let _lock = ENV_LOCK.lock().unwrap();
        let dir = temp_dir();
        let kingi_dir = dir.join("kingi");
        fs::create_dir_all(&kingi_dir).unwrap();
        fs::write(kingi_dir.join("reply-3"), "Template three").unwrap();
        fs::write(kingi_dir.join("reply-0"), "Template zero").unwrap();
        with_xdg(&dir, || {
            assert_eq!(load_reply_template(3), Some("Template three".to_string()));
            assert_eq!(load_reply_template(0), Some("Template zero".to_string()));
            assert!(load_reply_template(5).is_none());
        });
    }
}
