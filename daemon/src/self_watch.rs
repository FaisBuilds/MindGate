//! Keeps the companion `mindgate-watchdog.service` systemd unit alive
//! from the daemon's own side — the mirror image of what the watchdog
//! script does for `mindgated.service`.
//!
//! Per MVP1 Stubbornness: "Daemon and watchdog monitor each other."
//! Neither `Restart=always` nor a watching companion can restart itself 
//! if IT is the one that gets stopped — a deliberate `systemctl stop` 
//! is honored by systemd, not treated as a crash to recover from. 
//! Therefore, each of the two units watches the OTHER instead. 
//! 
//! This ensures that stopping MindGate requires deliberately taking down 
//! both independent units, adding friction to bypass attempts.
//!
//! Only runs if `systemctl` is actually available. Silently a no-op 
//! otherwise (e.g., a non-systemd dev environment, or `cargo run` 
//! outside the installed service), matching the daemon's overall 
//! "fail gracefully, not catastrophically" posture.

use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::warn;

/// How often to check the watchdog's status.
const SELF_WATCH_INTERVAL: Duration = Duration::from_secs(15);

/// The name of the companion systemd unit.
const WATCHDOG_UNIT: &str = "mindgate-watchdog.service";

/// Checks if the watchdog systemd unit is currently active.
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
/// `guardian::spawn`.
pub fn spawn() {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SELF_WATCH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            
            if !watchdog_unit_active().await {
                warn!("{WATCHDOG_UNIT} is not active — attempting to restart it");
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