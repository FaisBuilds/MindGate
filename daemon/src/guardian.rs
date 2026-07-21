//! Guardian: the daemon's core "protect the extension" loop.
//!
//! Every `CHECK_INTERVAL`, this checks how long it's been since the
//! extension last sent an `ExtensionHeartbeat` (see `server.rs`). If
//! that gap exceeds `HEARTBEAT_TIMEOUT` — meaning the extension was
//! disabled, removed, or its background worker crashed — every
//! supported Chromium-based browser process is closed. Per the MVP1
//! doc: "The daemon never decides what to block. It only protects the
//! blocker." This is that protection.

use crate::server::HEARTBEAT_TIMEOUT;
use crate::AppState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;

const CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Grace period after daemon startup before "no heartbeat yet" is
/// treated as "the extension is gone" rather than "hasn't had time to
/// connect yet."
const STARTUP_GRACE: Duration = Duration::from_secs(90);

/// Every supported Chromium-based browser's process name.
const SUPPORTED_BROWSER_PROCESSES: &[&str] = &[
    "chrome",
    "chromium",
    "brave",
    "brave-browser",
    "msedge",
    "vivaldi-bin",
    "opera",
];

/// FIX: Pass state reference so we can reset the heartbeat after killing.
async fn close_supported_browsers(state: &AppState) {
    for process in SUPPORTED_BROWSER_PROCESSES {
        match Command::new("pkill").args(["-x", process]).status().await {
            Ok(status) if status.success() => {
                tracing::warn!("guardian: closed running instances of {process}");
            }
            // pkill exits 1 when no matching process is found — not an error.
            Ok(_) => {}
            Err(e) => {
                tracing::warn!("guardian: failed to run pkill for {process}: {e}");
            }
        }
    }
    
    // CRITICAL FIX: Set heartbeat to CURRENT TIME
    // This gives the user a FULL HEARTBEAT_TIMEOUT (150s) to:
    // 1. Reopen Chrome
    // 2. Re-enable the extension
    // 3. Let the extension send its first heartbeat
    *state.last_heartbeat.lock().await = Some(Instant::now());
    tracing::info!("guardian: reset heartbeat timer - user has 150s to reconnect");
}

/// Spawns the background task. Call once from `main.rs`.
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
                // No heartbeat ever received, and we're past the startup grace period.
                None => true,
            };

            if stale {
                tracing::warn!(
                    "guardian: no heartbeat from extension in over {:?} — extension is \
                     missing, disabled, or crashed. Closing supported browsers.",
                    HEARTBEAT_TIMEOUT
                );
                // FIX: Pass state to the function
                close_supported_browsers(&state).await;
            }
        }
    });
}