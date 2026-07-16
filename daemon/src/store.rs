//! Loads and saves the rule set to disk. This is the only module that
//! reads/writes rules.toml — the server (`server.rs`) holds the
//! in-memory copy and calls here to persist it after every mutation.
//! Mirrors the split in `engine.rs`: that module is the only one
//! allowed to touch nftables/DNS, this one is the only one allowed to
//! touch `rules.toml`. Neither reaches into the other's territory.

use anyhow::{Context, Result};
use mindgate_common::{rules_path, RuleSet};
use tokio::fs;

/// Load the rule set from disk. A missing file is not an error — it
/// just means no rules have been added yet (e.g. first run after
/// install), so this returns `RuleSet::default()` rather than bailing.
pub async fn load() -> Result<RuleSet> {
    let path = rules_path();
    match fs::read_to_string(&path).await {
        Ok(contents) => toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RuleSet::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Persist the rule set to disk, creating the config directory if it
/// doesn't exist yet. Overwrites the whole file — rules.toml is small
/// enough (hundreds of entries at most) that there's no reason for
/// incremental writes, and a full rewrite is easier to reason about
/// than a partial-update format.
pub async fn save(rules: &RuleSet) -> Result<()> {
    let path = rules_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let toml_str = toml::to_string_pretty(rules).context("failed to serialize rules")?;
    fs::write(&path, toml_str)
        .await
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mindgate_common::WebsiteRule;
    use std::sync::Mutex;

    // Tests mutate the process-wide MINDGATE_CONFIG_DIR env var, so they
    // must not run concurrently with each other (cargo runs tests in
    // threads within one process by default). This lock serializes them.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct TempDir(std::path::PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("mindgate-test-{}-{}", tag, std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn round_trips_through_disk() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("roundtrip");
        std::env::set_var("MINDGATE_CONFIG_DIR", dir.path());

        let mut rules = RuleSet::default();
        rules.websites.push(WebsiteRule { domain: "reddit.com".into() });
        save(&rules).await.unwrap();

        let loaded = load().await.unwrap();
        assert_eq!(loaded.websites.len(), 1);
        assert_eq!(loaded.websites[0].domain, "reddit.com");

        std::env::remove_var("MINDGATE_CONFIG_DIR");
    }

    #[tokio::test]
    async fn missing_file_returns_default_not_error() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("missing");
        std::env::set_var("MINDGATE_CONFIG_DIR", dir.path());

        let loaded = load().await.unwrap();
        assert_eq!(loaded.total_rules(), 0);

        std::env::remove_var("MINDGATE_CONFIG_DIR");
    }
}


