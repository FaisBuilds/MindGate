//! Loads and saves the rule set to disk. This is the only module that
//! reads/writes rules.toml — the server (`server.rs`) holds the
//! in-memory copy and calls here to persist it after every mutation.
//! Mirrors the split in `engine.rs`: that module is the only one
//! allowed to touch nftables/DNS, this one is the only one allowed to
//! touch `rules.toml`. Neither reaches into the other's territory.

use anyhow::{Context, Result};
use mindgate_common::{lock_state_path, rules_path, LockState, RuleSet};
use std::process::Stdio;
use tokio::fs;
use tokio::process::Command;

/// Best-effort `chattr +i`/`-i` on the lock file. This is friction,
/// not a security boundary — root already has to run this daemon to
/// touch nft/dnsmasq at all, and root can always `chattr -i` it back
/// off. What it buys is that "delete lock.toml and restart the
/// daemon" stops being a single careless command: you have to
/// deliberately clear the immutable bit first, which is exactly the
/// kind of extra, undeniable step Lock Mode is supposed to impose on
/// yourself (see lock.rs's module doc — "friction, not a sandbox").
///
/// Deliberately non-fatal on failure: some filesystems (tmpfs,
/// certain overlay/container setups — relevant for local dev, which
/// often points `MINDGATE_CONFIG_DIR` at `/tmp`) don't support the
/// ext2/3/4 immutable attribute at all. `chattr` failing there just
/// means this one layer of friction isn't available; it must never
/// block persisting the lock state itself, which is the part that
/// actually matters.
async fn set_lock_file_immutable(path: &std::path::Path, immutable: bool) {
    let flag = if immutable { "+i" } else { "-i" };
    let result = Command::new("chattr")
        .arg(flag)
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;

    match result {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(
            "chattr {flag} {} exited with {status} — filesystem likely doesn't support \
             the immutable attribute; continuing without this extra layer of friction",
            path.display()
        ),
        Err(e) => tracing::warn!(
            "failed to run chattr {flag} {}: {e} — continuing without immutable lock \
             file protection",
            path.display()
        ),
    }
}

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

/// Load the persisted lock state from disk. A missing file means the
/// ruleset has never been locked — returns `LockState::default()`
/// (unlocked), same "missing is not an error" reasoning as `load()`.
///
/// Before this existed, the daemon always started with
/// `LockState::default()` regardless of what was on disk, which meant
/// `mindgate lock forever` silently un-locked itself on the next
/// crash or restart. That's the bug this function (and `save_lock`)
/// closes.
pub async fn load_lock() -> Result<LockState> {
    let path = lock_state_path();
    match fs::read_to_string(&path).await {
        Ok(contents) => toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LockState::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Persist the current lock state to disk. Called once, at the moment
/// `mindgate lock` succeeds — see `server.rs`'s `Request::Lock`
/// handler. Deliberately NOT called on every mutation the way
/// `save()` (rules) is, since lock state only ever changes at that
/// one moment (there's no unlock path to save from the other
/// direction).
pub async fn save_lock(state: &LockState) -> Result<()> {
    let path = lock_state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    // MUST run before the write below: a previous call may have left
    // this file `chattr +i`'d (see the end of this function), and an
    // immutable file can't be overwritten — even by root — until the
    // attribute is cleared first. Idempotent/harmless if the file
    // wasn't immutable (fresh install) or doesn't exist yet.
    set_lock_file_immutable(&path, false).await;

    let toml_str = toml::to_string_pretty(state).context("failed to serialize lock state")?;
    fs::write(&path, toml_str)
        .await
        .with_context(|| format!("failed to write {}", path.display()))?;

    // Re-lock the file down immediately after writing IF the new
    // state is actually locked. Left mutable when `state.locked` is
    // false (i.e. this call is persisting a clear/expiry) — an
    // unlocked state has no reason to resist being overwritten by the
    // next `mindgate lock`.
    if state.locked {
        set_lock_file_immutable(&path, true).await;
    }

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

    #[tokio::test]
    async fn lock_state_round_trips_through_disk() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("lock-roundtrip");
        std::env::set_var("MINDGATE_CONFIG_DIR", dir.path());

        let mut lock = LockState::default();
        lock.locked = true;
        lock.unlock_at = Some(9_999_999_999);
        save_lock(&lock).await.unwrap();

        let loaded = load_lock().await.unwrap();
        assert!(loaded.locked);
        assert_eq!(loaded.unlock_at, Some(9_999_999_999));

        std::env::remove_var("MINDGATE_CONFIG_DIR");
    }

    #[tokio::test]
    async fn missing_lock_file_returns_unlocked_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new("lock-missing");
        std::env::set_var("MINDGATE_CONFIG_DIR", dir.path());

        let loaded = load_lock().await.unwrap();
        assert!(!loaded.locked);

        std::env::remove_var("MINDGATE_CONFIG_DIR");
    }
}