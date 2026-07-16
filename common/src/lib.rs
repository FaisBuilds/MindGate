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

/// A specific subreddit (e.g. "gonewild" for reddit.com/r/gonewild).
/// Enforced by the extension, same reasoning as `KeywordRule`. Kept as
/// its own type — rather than folded into keywords — because the CLI/UX
/// treats it as a first-class concept and it always expands to a
/// `reddit.com/r/<name>` path match in the extension's dynamic rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubredditRule {
    pub subreddit: String,
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
}

impl RuleSet {
    /// True if this rule set has anything the extension needs to
    /// enforce (i.e. anything nftables/DNS alone cannot handle). This
    /// is the hook point for the post-MVP fail-closed fallback described
    /// in CONTEXT.md §5: if the extension goes silent while this is
    /// true, the engine should fall back to whole-domain blocking.
    pub fn has_path_level_rules(&self) -> bool {
        !self.keywords.is_empty() || !self.subreddits.is_empty()
    }

    pub fn total_rules(&self) -> usize {
        self.websites.len() + self.keywords.len() + self.subreddits.len()
    }
}

/// Current lock-mode state. Stub for MVP1 (see CONTEXT.md §7) — the type
/// exists so `Status` has a stable shape, but the daemon does not yet
/// enforce refusal of mutations while locked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LockState {
    pub locked: bool,
    /// Unix timestamp (seconds) when the lock expires, if time-based.
    pub unlock_at: Option<u64>,
    /// True if unlocking requires the master password rather than
    /// just waiting out the timer.
    pub password_required: bool,
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
    AddSubreddit { subreddit: String },
    RemoveSubreddit { subreddit: String },
    List,
    Status,
    /// Lock rules for `duration_secs` seconds. If `password` is set,
    /// unlocking early requires it instead of a fixed timer. Stub for
    /// MVP1 — accepted by the protocol, not yet enforced by the daemon.
    Lock { duration_secs: Option<u64>, password: Option<String> },
    Unlock { password: Option<String> },
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

/// Where persisted rules and the password hash live. Overridable via
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

pub fn password_hash_path() -> PathBuf {
    config_dir().join("password.hash")
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