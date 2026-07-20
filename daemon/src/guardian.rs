//! Guardian: the browser-kill fallback for when the extension goes
//! dark while locked.
//!
//! Context (see `mindgate-common`'s `RuleSet::has_path_level_rules`
//! doc comment, which flagged exactly this gap as a "post-MVP
//! fail-closed fallback" to build later): keyword/subreddit/path
//! rules are ONLY enforced by the browser extension
//! (content.js/background.js) — engine.rs's nftables/dnsmasq layer
//! has no idea those rules exist, by design (CONTEXT.md §5). If the
//! extension stops reporting in — uninstalled, disabled, the browser
//! profile it's registered in gets swapped for a fresh one, Chrome
//! quietly killed its service worker and it never woke back up, etc.
//! — while the ruleset is locked, path-level enforcement silently
//! stops while everything else (`mindgate status`, the DNS blackhole)
//! keeps looking perfectly healthy. Disabling an extension is four
//! clicks and produces no daemon-side signal at all otherwise.
//!
//! This module is the deliberately blunt answer: while locked, if the
//! daemon hasn't heard an `ExtensionHeartbeat` in longer than
//! `server::HEARTBEAT_TIMEOUT`, kill known browser processes outright,
//! on a fixed interval, for as long as the heartbeat stays stale. It
//! does not try to close "just the blocked tab" — the daemon has no
//! visibility into what any given browser window is showing, and a
//! full restart is also what's actually required for a re-enabled or
//! reinstalled extension to register itself with the daemon again
//! anyway.

use crate::lock;
use crate::AppState;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

/// How often to re-check + re-kill while the extension is missing.
/// Deliberately short — the whole point is that reopening the browser
/// doesn't buy back any real window of unenforced access.
const GUARDIAN_INTERVAL: Duration = Duration::from_secs(15);

/// Grace period after the daemon itself starts, before the guardian
/// will act on "no heartbeat has ever arrived". Without this, a fresh
/// boot into an already-locked state (see `main.rs`'s startup restore)
/// would see `last_heartbeat: None` and start killing the browser
/// before it's even had a chance to launch and the extension had a
/// chance to connect for the first time.
const STARTUP_GRACE: Duration = Duration::from_secs(90);

/// Process names to kill, matched against `comm` (the actual binary
/// name via plain `pkill <name>`) — deliberately NOT `pkill -f`, which
/// matches the full command line and can collide with any unrelated
/// process that merely has e.g. "chrome" somewhere in one of its
/// arguments (a directory name, a URL, whatever). Matching `comm`
/// instead means this can only ever hit an actual browser binary.
///
/// This list is necessarily approximate and distro-dependent — Linux
/// truncates `comm` to 15 characters, and some browsers' real binary
/// name doesn't match their marketing name at all (Vivaldi's is
/// `vivaldi-bin`, not `vivaldi`). Trim or extend to match what's
/// actually installed on this machine; an easy way to check is
/// `ps -eo comm` while the browser is open.
const BROWSER_PROCESS_NAMES: &[&str] = &[
    "firefox",
    "firefox-esr",
    "chrome",
    "chromium",
    "chromium-browse", // truncated `comm` for chromium-browser
    "brave",
    "brave-browser",
    "opera",
    "vivaldi-bin",
    "msedge",
];

async fn kill_browsers() {
    for name in BROWSER_PROCESS_NAMES {
        let status = Command::new("pkill")
            .args(["-9", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
        match status {
            // pkill exits 0 only when it actually matched and killed
            // something. Exit 1 ("no process matched") is the
            // overwhelmingly common case here and isn't an error, so
            // it's deliberately not logged — logging it every 15s for
            // every one of nine names when nothing's even open would
            // be pure noise.
            Ok(status) if status.success() => {
                tracing::warn!("guardian: killed running '{name}' process(es)");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("guardian: failed to run pkill for '{name}': {e}");
            }
        }
    }
}

/// Spawns the background task. Call once from `main.rs`, alongside
/// `spawn_lock_watcher`, after `AppState` is constructed.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(GUARDIAN_INTERVAL);
        loop {
            interval.tick().await;

            let locked = {
                let lock_state = state.lock.lock().await;
                lock::effective_locked(&lock_state)
            };
            if !locked {
                continue;
            }

            // Only bother if the ruleset actually depends on the
            // extension for something. A pure website-level lock is
            // already fully enforced at the DNS layer (engine.rs)
            // regardless of whether the extension is alive, so
            // killing the browser over that would be pure friction
            // with zero protective value — the domain is already
            // NXDOMAIN either way.
            let depends_on_extension = {
                let rules = state.rules.lock().await;
                rules.has_path_level_rules()
            };
            if !depends_on_extension {
                continue;
            }

            let last_heartbeat = *state.last_heartbeat.lock().await;
            let extension_missing = match last_heartbeat {
                Some(t) => t.elapsed() > crate::server::HEARTBEAT_TIMEOUT,
                None => state.started_at.elapsed() > STARTUP_GRACE,
            };

            if extension_missing {
                tracing::warn!(
                    "guardian: locked ruleset has path-level rules but the extension \
                     heartbeat is stale — killing browser processes"
                );
                kill_browsers().await;
            }
        }
    });
}
