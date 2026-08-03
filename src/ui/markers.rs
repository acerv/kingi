// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
use crate::core::config::config_dir;
use crate::core::thread::Email;
use regex::Regex;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchStatus {
    Normal,
    Reviewed,
    Merged,
}

pub struct MarkersCache {
    path: PathBuf,
    cache: HashMap<String, PatchStatus>,
    dirty: bool,
    merged_regexes: Vec<Regex>,
    hash: String,
}

impl MarkersCache {
    pub fn new(custom_markers: Vec<String>) -> Self {
        let path = config_dir().join("markers_cache");
        let mut cache = HashMap::new();

        let mut hasher = DefaultHasher::new();
        CACHE_VERSION.hash(&mut hasher);
        custom_markers.hash(&mut hasher);
        let current_hash = hasher.finish().to_string();
        let mut cache_valid = false;

        if let Ok(content) = fs::read_to_string(&path) {
            let mut lines = content.lines();
            if let Some(first_line) = lines.next() {
                if first_line == current_hash {
                    cache_valid = true;
                    for line in lines {
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        if parts.len() == 2 {
                            let status = match parts[1] {
                                "M" => PatchStatus::Merged,
                                "R" => PatchStatus::Reviewed,
                                _ => PatchStatus::Normal,
                            };
                            cache.insert(parts[0].to_string(), status);
                        }
                    }
                }
            }
        }

        let mut merged_regexes = Vec::new();
        if custom_markers.is_empty() {
            merged_regexes.push(
                Regex::new(r"(?i)\b(applied|merged|pushed)(,?\s+thanks|\s+to\s+\S+|[.!]|\s*$)")
                    .unwrap(),
            );
            merged_regexes
                .push(Regex::new(r"(?i)\bthanks,?\s+(applied|merged|pushed)\b").unwrap());
            merged_regexes.push(Regex::new(r"(?i)\b(patch(set|es)?|series)\s+(applied|merged)(,?\s+thanks|\s+to\s+\S+|[.!]?\s*$)").unwrap());
            merged_regexes
                .push(Regex::new(r"(?i)\b(T|t)hanks.*(merged|applied|pushed)").unwrap());
        } else {
            for marker in custom_markers {
                if let Ok(re) = regex::RegexBuilder::new(&marker).case_insensitive(true).build() {
                    merged_regexes.push(re);
                } else if let Ok(re) = regex::RegexBuilder::new(&regex::escape(&marker)).case_insensitive(true).build() {
                    merged_regexes.push(re);
                }
            }
        }

        Self {
            path,
            cache,
            dirty: !cache_valid,
            merged_regexes,
            hash: current_hash,
        }
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        let mut content = String::new();
        content.push_str(&format!("{}\n", self.hash));
        for (id, status) in &self.cache {
            let s = match status {
                PatchStatus::Merged => "M",
                PatchStatus::Reviewed => "R",
                PatchStatus::Normal => "N",
            };
            content.push_str(&format!("{id} {s}\n"));
        }
        let _ = fs::write(&self.path, content);
        self.dirty = false;
    }

    pub fn get_status(&mut self, email: &Email) -> PatchStatus {
        if let Some(status) = self.cache.get(&email.message_id) {
            return *status;
        }

        let status = self.compute_email_status(email);
        self.cache.insert(email.message_id.clone(), status);
        self.dirty = true;
        status
    }

    pub fn set_status(&mut self, email: &Email, status: PatchStatus) {
        self.cache.insert(email.message_id.clone(), status);
        self.dirty = true;
    }

    fn compute_email_status(&self, email: &Email) -> PatchStatus {
        let msg = match email.to_message() {
            Ok(m) => m,
            Err(_) => return PatchStatus::Normal,
        };
        let body = msg.body_text(0).map(|t| t.into_owned()).unwrap_or_default();

        let mut reviewed = false;
        for line in body.lines() {
            if line.starts_with('>') {
                continue;
            }
            for re in &self.merged_regexes {
                if re.is_match(line) {
                    return PatchStatus::Merged;
                }
            }
            if line
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("reviewed-by:")
            {
                reviewed = true;
            }
        }

        if reviewed {
            PatchStatus::Reviewed
        } else {
            PatchStatus::Normal
        }
    }
}

impl Drop for MarkersCache {
    fn drop(&mut self) {
        self.save();
    }
}
