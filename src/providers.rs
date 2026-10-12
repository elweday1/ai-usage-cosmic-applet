// SPDX-License-Identifier: GPL-3.0-only

//! Usage providers. Every provider is a JSONL history file in the common
//! usage format (see `usage.rs`). Providers are auto-detected: a known tool's
//! file must exist, and any `*.jsonl` file dropped into the drop-in directory
//! becomes a provider named after its file stem.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// Stable identifier (file stem for drop-in providers).
    pub id: String,
    /// Tab label.
    pub name: String,
    pub path: PathBuf,
}

/// Known tools and where they keep their usage history.
const KNOWN: &[(&str, &str, &str)] = &[
    ("claude", "Claude", ".claude/usage-history.jsonl"),
    ("codex", "Codex", ".codex/usage-history.jsonl"),
    ("opencode", "OpenCode", ".local/share/opencode/usage-history.jsonl"),
    ("antigravity", "Antigravity", ".antigravity/usage-history.jsonl"),
];

/// Directory scanned for extra providers: `<dir>/<id>.jsonl`.
fn drop_in_dir(home: &Path) -> PathBuf {
    home.join(".local/share/ai-usage")
}

/// Detect all providers that currently have a history file. `claude_override`
/// replaces the default Claude path (the settings' history path).
pub fn detect(home: &Path, claude_override: Option<&Path>) -> Vec<Provider> {
    let mut out: Vec<Provider> = Vec::new();

    for (id, name, rel) in KNOWN {
        let path = match (*id, claude_override) {
            ("claude", Some(p)) => p.to_path_buf(),
            _ => home.join(rel),
        };
        if path.is_file() {
            out.push(Provider { id: id.to_string(), name: name.to_string(), path });
        }
    }

    if let Ok(entries) = fs::read_dir(drop_in_dir(home)) {
        let mut extra: Vec<Provider> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && p.is_file())
            .filter_map(|p| {
                let id = p.file_stem()?.to_str()?.to_string();
                let name = display_name(&id);
                Some(Provider { id, name, path: p })
            })
            .filter(|p| !out.iter().any(|k| k.id == p.id))
            .collect();
        extra.sort_by(|a, b| a.id.cmp(&b.id));
        out.extend(extra);
    }

    out
}

/// "my-tool" → "My-tool"; known ids keep their curated names via `KNOWN`.
fn display_name(id: &str) -> String {
    let mut c = id.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => id.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ai-usage-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detects_only_existing_known_providers() {
        let home = tmp("known");
        assert!(detect(&home, None).is_empty());
        fs::create_dir_all(home.join(".codex")).unwrap();
        fs::write(home.join(".codex/usage-history.jsonl"), "").unwrap();
        let p = detect(&home, None);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].id, "codex");
        assert_eq!(p[0].name, "Codex");
    }

    #[test]
    fn drop_in_files_become_providers_and_known_ids_are_not_duplicated() {
        let home = tmp("dropin");
        fs::create_dir_all(home.join(".local/share/ai-usage")).unwrap();
        fs::write(home.join(".local/share/ai-usage/gemini.jsonl"), "").unwrap();
        fs::write(home.join(".local/share/ai-usage/claude.jsonl"), "").unwrap();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(home.join(".claude/usage-history.jsonl"), "").unwrap();
        let p = detect(&home, None);
        let ids: Vec<&str> = p.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["claude", "gemini"]);
        assert_eq!(p[1].name, "Gemini");
    }

    #[test]
    fn claude_override_replaces_default_path() {
        let home = tmp("override");
        let custom = home.join("custom.jsonl");
        fs::write(&custom, "").unwrap();
        let p = detect(&home, Some(&custom));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].path, custom);
    }
}
