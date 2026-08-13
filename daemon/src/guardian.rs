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
//! `STARTUP_GRACE` is measured from when the *daemon* starts (at boot,
//! via systemd), not from when the user actually opens a browser. If
//! more than `STARTUP_GRACE` passes after boot before Chrome is first
//! opened — completely normal; login and getting to a browser takes
//! time — `last_heartbeat` is still `None` the moment grace ends, and
//! the very next `CHECK_INTERVAL` tick would otherwise treat that as
//! "the extension is gone" and kill Chrome within seconds of it being
//! opened, before the extension's first heartbeat (sent ~1s after its
//! service worker wakes) has a real chance to land. Confirmed via a
//! real log: daemon started at 14:38:00, first kill fired at 14:39:29
//! (89s later, right at the grace boundary), with `last_heartbeat`
//! never having been set at all in that session.
//!
//! The fix below only affects the "never connected at all" (`None`)
//! case: it gets one extra `CHECK_INTERVAL` tick before a kill, purely
//! to cover the startup race. A heartbeat that was previously received
//! and then genuinely goes stale (`Some(t)` case) is untouched and
//! still triggers a kill on the very next check, exactly as before —
//! real protection against a disabled/crashed extension is not
//! weakened by this change.

use crate::server::HEARTBEAT_TIMEOUT;
use crate::AppState;
use system_lock_resume::LockWatcher;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;

const CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Grace period after daemon startup before "no heartbeat yet" is
/// treated as "the extension is gone" rather than "hasn't had time to
/// connect yet."
const STARTUP_GRACE: Duration = Duration::from_secs(90);

/// How many consecutive "never received a heartbeat at all" checks to
/// tolerate after STARTUP_GRACE ends before actually killing. 2 means:
/// the first post-grace tick that still sees `None` just warns and
/// waits one more CHECK_INTERVAL (10s) instead of killing immediately —
/// enough to cover the boot-race window above without meaningfully
/// weakening protection against a genuinely-never-installed extension,
/// which would keep missing every subsequent tick anyway.
const NEVER_CONNECTED_TOLERANCE: u32 = 2;

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
pub fn spawn(state: Arc<AppState>, lock_watcher: LockWatcher) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CHECK_INTERVAL);

        // Local to this task, not shared/core state: counts consecutive
        // post-grace checks that still see last_heartbeat == None. Reset
        // to 0 the moment a real heartbeat ever arrives (last_heartbeat
        // becomes Some), so this only ever matters during the initial
        // boot-race window, never again afterward in the same run.
        let mut never_connected_misses: u32 = 0;

        loop {
            interval.tick().await;

            if state.started_at.elapsed() < STARTUP_GRACE {
                continue;
            }

            let last_heartbeat = *state.last_heartbeat.lock().await;

            match last_heartbeat {
                Some(t) => {
                    // A heartbeat was received at some point — reset the
                    // never-connected counter (irrelevant now) and fall
                    // back to the original, unmodified strict check:
                    // stale means kill, on this very tick, no extra
                    // tolerance. This path's behavior is identical to
                    // before this change.
                    never_connected_misses = 0;

                    if t.elapsed() >= HEARTBEAT_TIMEOUT {
                        if lock_watcher.is_locked() {
                            tracing::info!(
                                "guardian: heartbeat stale but session is locked — not closing browsers"
                            );
                            continue;
                        }

                        tracing::warn!(
                            "guardian: no heartbeat from extension in over {:?} — extension is \
                             missing, disabled, or crashed. Closing supported browsers.",
                            HEARTBEAT_TIMEOUT
                        );
                        close_supported_browsers(&state).await;
                    }
                }
                None => {
                    // Never received a single heartbeat yet. Could be a
                    // genuinely dead/never-loaded extension, or just the
                    // boot-race window described above. Give it
                    // NEVER_CONNECTED_TOLERANCE consecutive ticks before
                    // treating it as the former.
                    if lock_watcher.is_locked() {
                        tracing::info!(
                            "guardian: no heartbeat yet but session is locked — not closing browsers"
                        );
                        continue;
                    }

                    never_connected_misses += 1;

                    if never_connected_misses < NEVER_CONNECTED_TOLERANCE {
                        tracing::warn!(
                            "guardian: no heartbeat received yet ({}/{}) — waiting one more \
                             check before assuming the extension is missing, in case this is \
                             the startup race rather than a dead extension",
                            never_connected_misses,
                            NEVER_CONNECTED_TOLERANCE
                        );
                        continue;
                    }

                    tracing::warn!(
                        "guardian: still no heartbeat after {} consecutive checks post-grace — \
                         extension is missing, disabled, or crashed. Closing supported browsers.",
                        never_connected_misses
                    );
                    close_supported_browsers(&state).await;
                    never_connected_misses = 0;
                }
            }
        }
    });
}