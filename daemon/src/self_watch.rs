//! Keeps the companion `mindgate-watchdog.service` systemd unit alive
//! from the daemon's own side — the mirror image of what
//! `mindgate-watchdog.sh` does for `mindgated.service` (see
//! `installer/mindgate-watchdog.sh`).
//!
//! Neither `Restart=always` nor a watching companion can restart
//! itself if IT is the one that gets stopped — a deliberate
//! `systemctl stop` is honored by systemd, not treated as a crash to
//! recover from — so each of the two units watches the OTHER instead.
//! Killing or masking both at the same time still works; this is
//! friction, not a sandbox (see `lock.rs`'s module doc for the same
//! philosophy applied to Lock Mode itself). What it buys is that
//! stopping MindGate stops being a single command — you have to
//! deliberately take down both independent units, not just one.
//!
//! Only runs if `systemctl` is actually available. Silently a no-op
//! otherwise (e.g. a non-systemd dev environment, or `cargo run`
//! outside the installed service) — matching `engine.rs`'s own
//! "if the tool isn't there, log and continue" posture rather than
//! treating this as a hard startup dependency.

use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

const SELF_WATCH_INTERVAL: Duration = Duration::from_secs(15);
const WATCHDOG_UNIT: &str = "mindgate-watchdog.service";

async fn watchdog_unit_active() -> bool {
    Command::new("systemctl")
        .args(["is-active", "--quiet", WATCHDOG_UNIT])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Spawns the background task. Call once from `main.rs`, alongside
/// `spawn_lock_watcher` and `guardian::spawn`.
pub fn spawn() {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SELF_WATCH_INTERVAL);
        loop {
            interval.tick().await;
            if !watchdog_unit_active().await {
                tracing::warn!("{WATCHDOG_UNIT} is not active — attempting to restart it");
                let _ = Command::new("systemctl")
                    .args(["start", WATCHDOG_UNIT])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
            }
        }
    });
}
