//! Guardian: the daemon's core "protect the extension" loop.
//!
//! Every `CHECK_INTERVAL`, this checks how long it's been since the
//! extension last sent an `ExtensionHeartbeat` (see `server.rs`). If
//! that gap exceeds `HEARTBEAT_TIMEOUT` — meaning the extension was
//! disabled, removed, or its background worker crashed — every
//! supported Chromium-based browser process is closed. Per the MVP1
//! doc: "The daemon never decides what to block. It only protects the
//! blocker." This is that protection.
//!
//! NOTE ON REPO STATE: this file previously contained the contents of
//! `installer/install.sh` at this path — `daemon/src/guardian.rs` was
//! bash, not Rust, which meant `mod guardian;` in `main.rs` could not
//! have compiled. That content has been moved back to
//! `installer/install.sh` (see that file). This is the real,
//! from-scratch `guardian.rs`.

use crate::server::HEARTBEAT_TIMEOUT;
use crate::AppState;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;

const CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Grace period after daemon startup before "no heartbeat yet" is
/// treated as "the extension is gone" rather than "hasn't had time to
/// connect yet." Comfortably longer than the extension's own sync
/// cadence (`background.js`'s `chrome.alarms` period, ~60s, plus
/// scheduling jitter) — a bare `mindgate restart` or a reboot must not
/// immediately nuke every open browser before the extension's
/// background worker has even woken up.
const STARTUP_GRACE: Duration = Duration::from_secs(90);

/// Every supported Chromium-based browser's process name, per the
/// MVP1 doc's Browser Support list. Matched by process name via
/// `pkill -x`, not by window title or PID tracking — the simplest
/// thing that reliably closes every window and every profile of a
/// given browser at once, which is the point: leaving one window open
/// while three others close defeats the purpose.
const SUPPORTED_BROWSER_PROCESSES: &[&str] = &[
    "chrome",
    "chromium",
    "brave",
    "brave-browser",
    "msedge",
    "vivaldi-bin",
    "opera",
];

async fn close_supported_browsers() {
    for process in SUPPORTED_BROWSER_PROCESSES {
        match Command::new("pkill").args(["-x", process]).status().await {
            Ok(status) if status.success() => {
                tracing::warn!("guardian: closed running instances of {process}");
            }
            // pkill exits 1 when no matching process is found — not
            // an error, just "that browser wasn't running."
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("guardian: failed to run pkill for {process}: {e}");
            }
        }
    }
}

/// Spawns the background task. Call once from `main.rs`, alongside
/// `self_watch::spawn()`.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CHECK_INTERVAL);
        loop {
            interval.tick().await;

            if state.started_at.elapsed() < STARTUP_GRACE {
                continue;
            }

            let last_heartbeat = *state.last_heartbeat.lock().await;
            let stale = match last_heartbeat {
                Some(t) => t.elapsed() >= HEARTBEAT_TIMEOUT,
                // No heartbeat ever received, and we're past the
                // startup grace period — treat the same as stale.
                None => true,
            };

            if stale {
                tracing::warn!(
                    "guardian: no heartbeat from extension in over {:?} — extension is \
                     missing, disabled, or crashed. Closing supported browsers.",
                    HEARTBEAT_TIMEOUT
                );
                close_supported_browsers().await;
            }
        }
    });
}