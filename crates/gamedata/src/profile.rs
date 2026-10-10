//! Warcraft III profile files (`UnitFunc.txt` and similar): INI-like text with
//! `[section]` headers, `key=value` lines and `//` comments.
//!
//! Values are kept as raw text (trimmed, quotes included). Use
//! [`Profile::get_list`] to split comma-separated lists, which respects
//! quoted items that contain commas.

use std::collections::BTreeMap;

use crate::{decode_text, Error};

/// A parsed profile file. Keys before any section header belong to section `""`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Profile {
    /// Parses profile text. A later duplicate key in the same section replaces
    /// the earlier value.
    pub fn parse(src: &str) -> Result<Profile, Error> {
        let src = src.strip_prefix('\u{feff}').unwrap_or(src);
        let mut profile = Profile::default();
        let mut section = String::new();
        for (i, raw) in src.lines().enumerate() {
            let line_no = i + 1;
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                let Some(name) = rest.strip_suffix(']') else {
                    return Err(Error::Profile {
                        line: line_no,
                        msg: "unterminated section header".to_owned(),
                    });
                };
                section = name.trim().to_owned();
                profile.sections.entry(section.clone()).or_default();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(Error::Profile {
                    line: line_no,
                    msg: format!("expected key=value, found {line:?}"),
                });
            };
            let key = key.trim();
            if key.is_empty() {
                return Err(Error::Profile {
                    line: line_no,
                    msg: "empty key".to_owned(),
                });
            }
            profile
                .sections
                .entry(section.clone())
                .or_default()
                .insert(key.to_owned(), value.trim().to_owned());
        }
        Ok(profile)
    }

    /// Parses profile bytes (UTF-8 or Windows-1252).
    pub fn parse_bytes(bytes: &[u8]) -> Result<Profile, Error> {
        Profile::parse(&decode_text(bytes))
    }

    /// The raw trimmed value of `key` in `section`, quotes included.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections.get(section)?.get(key).map(String::as_str)
    }

    /// The value of `key` in `section` split on commas outside quotes.
    /// Items are trimmed and a surrounding pair of quotes is removed.
    /// An empty value yields an empty list; empty items are kept.
    pub fn get_list(&self, section: &str, key: &str) -> Vec<String> {
        match self.get(section, key) {
            Some(raw) => split_list(raw),
            None => Vec::new(),
        }
    }

    /// Section names in sorted order.
    pub fn sections(&self) -> impl Iterator<Item = &str> + '_ {
        self.sections.keys().map(String::as_str)
    }

    /// Merges `other` on top of `self`: every key in `other` replaces the same
    /// key in `self`, and new sections and keys are added.
    pub fn merge(&mut self, other: &Profile) {
        for (section, entries) in &other.sections {
            let dst = self.sections.entry(section.clone()).or_default();
            for (key, value) in entries {
                dst.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Removes a `//` comment that is not inside double quotes.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_quote = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b'/' if !in_quote && bytes.get(i + 1) == Some(&b'/') => return &line[..i],
            _ => {}
        }
    }
    line
}

fn split_list(raw: &str) -> Vec<String> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    for ch in raw.chars() {
        match ch {
            '"' => {
                in_quote = !in_quote;
                current.push(ch);
            }
            ',' if !in_quote => {
                items.push(unquote(&current));
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    items.push(unquote(&current));
    items
}

fn unquote(item: &str) -> String {
    let t = item.trim();
    match t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.to_owned(),
        None => t.to_owned(),
    }
}
