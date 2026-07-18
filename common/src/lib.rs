//! mindgate-common
//!
//! Shared types used by `mindgate` (CLI), `mindgated` (daemon), and the
//! CLI's hidden native-messaging bridge mode. This crate defines the
//! on-disk rule format and the IPC protocol that flows over the Unix
//! domain socket. Nothing in here touches the network, nftables, or the
//! filesystem directly — that's the daemon's job (see `daemon/src/engine.rs`
//! and `daemon/src/store.rs`). Keeping this crate dependency-light means
//! the CLI doesn't have to pull in tokio just to know what a `Request` is.

use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;

/// A single blocked website (whole-domain block). Enforced at the
/// network layer: DNS redirect + nftables + periodically-resolved IP set.
/// See CONTEXT.md §4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebsiteRule {
    pub domain: String,
}

/// A keyword that, if present in a URL's path or query string, causes
/// the request to be blocked. Enforced by the browser extension, because
/// the path/query is only visible before TLS encrypts it. See CONTEXT.md §5.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeywordRule {
    pub value: String,
}

/// LEGACY, read-only in practice. A specific subreddit (e.g.
/// "gonewild" for reddit.com/r/gonewild). Nothing in the CLI writes
/// this type anymore — `PathRule` (`domain: "reddit.com", path:
/// "/r/gonewild"`) is the one, general mechanism for this now. Kept
/// only so a `rules.toml` written before this change still parses and
/// its entries still display/enforce, rather than silently vanishing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubredditRule {
    pub subreddit: String,
}

/// A domain-scoped path prefix (e.g. `domain: "reddit.com"`,
/// `path: "/r/gaming"`, or `domain: "twitter.com"`, `path: "/explore"`).
/// Enforced by the extension, same reasoning as `KeywordRule` — the
/// path is only visible before TLS.
///
/// This is THE path-level primitive — any domain + any path prefix,
/// not a Reddit-specific concept. `mindgate add path reddit.com/r/gaming`
/// and `mindgate add path youtube.com/shorts` are the same command,
/// just a different domain/path. The CLI deliberately does not have,
/// and will not grow, a `add-subreddit`-style command for any specific
/// site — see `SubredditRule`'s doc comment for the type this replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathRule {
    pub domain: String,
    pub path: String,
}

/// The full rule set, as persisted to `/etc/mindgate/rules.toml` (or the
/// dev path — see `config_dir()`) and as synced to the extension over
/// native messaging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuleSet {
    #[serde(default)]
    pub websites: Vec<WebsiteRule>,
    #[serde(default)]
    pub keywords: Vec<KeywordRule>,
    #[serde(default)]
    pub subreddits: Vec<SubredditRule>,
    /// NEW, additive field. `#[serde(default)]` matters here — it's
    /// what makes an existing `rules.toml` written before this field
    /// existed still parse cleanly (missing key -> empty Vec) instead
    /// of failing to load entirely.
    #[serde(default)]
    pub paths: Vec<PathRule>,
}

impl RuleSet {
    /// True if this rule set has anything the extension needs to
    /// enforce (i.e. anything nftables/DNS alone cannot handle). This
    /// is the hook point for the post-MVP fail-closed fallback described
    /// in CONTEXT.md §5: if the extension goes silent while this is
    /// true, the engine should fall back to whole-domain blocking.
    pub fn has_path_level_rules(&self) -> bool {
        !self.keywords.is_empty() || !self.subreddits.is_empty() || !self.paths.is_empty()
    }

    pub fn total_rules(&self) -> usize {
        self.websites.len() + self.keywords.len() + self.subreddits.len() + self.paths.len()
    }
}

/// Current lock-mode state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockState {
    pub locked: bool,
    /// Unix timestamp (seconds) when the lock expires, if time-based.
    pub unlock_at: Option<u64>,
}

/// Requests the CLI (or the extension's native-messaging bridge mode)
/// sends to `mindgated` over the Unix socket. Flat enum so the wire
/// format is a single tagged JSON object per message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args")]
pub enum Request {
    AddWebsite { domain: String },
    RemoveWebsite { domain: String },
    AddKeyword { value: String },
    RemoveKeyword { value: String },
    /// General domain + path-prefix block — e.g. `domain:
    /// "reddit.com", path: "/r/gaming"` or `domain: "twitter.com",
    /// path: "/explore"`. This is the ONLY path-level primitive now —
    /// there is deliberately no Reddit-specific command. Teaching
    /// MindGate about "a domain + a path prefix" once covers Reddit,
    /// YouTube Shorts, Twitter/X, Hacker News, or any future site,
    /// without adding a new command for each one.
    ///
    /// REMOVED: `AddSubreddit`/`RemoveSubreddit`. They were a strictly
    /// less general special case of exactly this (`domain:
    /// "reddit.com", path: "/r/<name>"`) — see `RuleSet.subreddits`'s
    /// doc comment for what happens to any data already stored that
    /// way.
    AddPath { domain: String, path: String },
    RemovePath { domain: String, path: String },
    List,
    Status,
    /// Lock rules for `duration_secs` seconds. `None` means no timer —
    /// stays locked until the daemon itself is torn down/reinstalled.
    ///
    /// REMOVED: `password: Option<String>` field, and the `Unlock`
    /// variant that used to sit next to this. There was no unlock path
    /// in this design to begin with (a lock only ever clears itself
    /// when its own timer expires — see `main.rs`'s lock-expiry
    /// watcher) — `server.rs` accepted a password on `Lock` and
    /// silently ignored it (`let _ = password;`), and `Unlock` always
    /// returned a hardcoded rejection. Dead weight, removed rather than
    /// kept "for wire compatibility" now that nothing depends on it.
    Lock { duration_secs: Option<u64> },
    /// Sent by the CLI's native-messaging bridge mode on connect and
    /// then periodically, so the daemon knows the extension is alive.
    ExtensionHeartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusInfo {
    pub daemon_running: bool,
    pub nft_table_active: bool,
    pub rule_count: usize,
    pub website_count: usize,
    pub keyword_count: usize,
    pub subreddit_count: usize,
    /// NEW, additive.
    pub path_count: usize,
    pub extension_connected: bool,
    pub lock: LockState,
}

/// Responses `mindgated` sends back. `Error` carries a human-readable
/// message rather than a structured error code — the CLI just prints it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", content = "data")]
pub enum Response {
    Ok,
    Error { message: String },
    Rules(RuleSet),
    Status(StatusInfo),
}

/// Where the Unix socket lives. In production this is under `/run`
/// (root-owned, tmpfs, cleared on reboot — appropriate for a socket).
/// Overridable via `MINDGATE_SOCKET` for local dev/testing without root,
/// e.g. `MINDGATE_SOCKET=/tmp/mindgate.sock`.
pub fn socket_path() -> PathBuf {
    if let Ok(p) = env::var("MINDGATE_SOCKET") {
        return PathBuf::from(p);
    }
    PathBuf::from("/run/mindgate/mindgate.sock")
}

/// Where persisted rules live. Overridable via
/// `MINDGATE_CONFIG_DIR` for dev/testing so we're not writing into
/// `/etc` as a non-root user.
pub fn config_dir() -> PathBuf {
    if let Ok(p) = env::var("MINDGATE_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    PathBuf::from("/etc/mindgate")
}

pub fn rules_path() -> PathBuf {
    config_dir().join("rules.toml")
}

/// Where the current `LockState` is persisted. Separate file from
/// `rules.toml` rather than a field on `RuleSet`, since the two have
/// different lifecycles: rules can be freely staged/edited pre-lock,
/// while lock state represents a one-way commitment. Keeping them in
/// separate files makes it possible to reason about (and back up/
/// inspect) each independently.
pub fn lock_state_path() -> PathBuf {
    config_dir().join("lock.toml")
}

/// Length-prefixed JSON framing shared by both ends of the socket:
/// a 4-byte big-endian length, then that many bytes of JSON. This is
/// deliberately the same shape as the browser native-messaging
/// protocol (4-byte length prefix + JSON), so the CLI's bridge mode
/// can sit in the middle without reframing anything — it just
/// forwards length-prefixed bodies between stdio and the socket.
pub mod wire {
    use serde::{de::DeserializeOwned, Serialize};

    pub fn encode<T: Serialize>(msg: &T) -> anyhow::Result<Vec<u8>> {
        let body = serde_json::to_vec(msg)?;
        let len = (body.len() as u32).to_be_bytes();
        let mut out = Vec::with_capacity(4 + body.len());
        out.extend_from_slice(&len);
        out.extend_from_slice(&body);
        Ok(out)
    }

    pub fn decode<T: DeserializeOwned>(body: &[u8]) -> anyhow::Result<T> {
        Ok(serde_json::from_slice(body)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruleset_counts_path_level_rules_correctly() {
        let mut rs = RuleSet::default();
        assert!(!rs.has_path_level_rules());
        rs.websites.push(WebsiteRule { domain: "reddit.com".into() });
        assert!(!rs.has_path_level_rules());
        rs.subreddits.push(SubredditRule { subreddit: "gonewild".into() });
        assert!(rs.has_path_level_rules());
        assert_eq!(rs.total_rules(), 2);
    }

    #[test]
    fn path_rule_counts_independently_alongside_subreddit_rule() {
        // Regression guard: PathRule is a NEW, separate field from
        // SubredditRule — adding one must not disturb counting the
        // other, and both must contribute to has_path_level_rules()/
        // total_rules() at the same time.
        let mut rs = RuleSet::default();
        rs.subreddits.push(SubredditRule { subreddit: "gonewild".into() });
        rs.paths.push(PathRule { domain: "twitter.com".into(), path: "/explore".into() });
        assert!(rs.has_path_level_rules());
        assert_eq!(rs.total_rules(), 2);
        assert_eq!(rs.subreddits.len(), 1);
        assert_eq!(rs.paths.len(), 1);
    }

    #[test]
    fn missing_paths_field_deserializes_as_empty_not_error() {
        // A rules.toml (or any serialized RuleSet) written before
        // `paths` existed must still load cleanly instead of failing
        // to parse. Using JSON here since serde_json is already a
        // confirmed dependency of this crate (see the `wire` module
        // above) — toml is only confirmed used in the daemon crate's
        // store.rs, not necessarily this one.
        let old_style_json = r#"{"websites":[{"domain":"reddit.com"}]}"#;
        let rs: RuleSet = serde_json::from_str(old_style_json).unwrap();
        assert_eq!(rs.paths.len(), 0);
        assert_eq!(rs.websites.len(), 1);
    }

    #[test]
    fn wire_roundtrip() {
        let req = Request::AddWebsite { domain: "reddit.com".into() };
        let bytes = wire::encode(&req).unwrap();
        // strip the 4-byte length prefix before decoding, same as the
        // socket reader on the other end will do
        let decoded: Request = wire::decode(&bytes[4..]).unwrap();
        match decoded {
            Request::AddWebsite { domain } => assert_eq!(domain, "reddit.com"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn default_socket_path_when_no_env_override() {
        std::env::remove_var("MINDGATE_SOCKET");
        assert_eq!(socket_path(), PathBuf::from("/run/mindgate/mindgate.sock"));
    }
}