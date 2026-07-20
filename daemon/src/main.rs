mod engine;
mod guardian;
mod lock;
mod self_watch;
mod server;
mod store;

use anyhow::Result;
// Use crate:: to explicitly direct the compiler to our sibling module
use crate::engine::NftEngine;
use mindgate_common::{config_dir, rules_path, LockState, RuleSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// Everything the socket server needs, shared across connection tasks.
/// `rules` and `lock` are behind their own mutexes rather than one big
/// lock over the whole struct, since `Status` reads both independently
/// of whether a mutation is in flight. `engine` has no external mutex
/// here because its own internal state (the dnsmasq child handle) is
/// already guarded inside `NftEngine` itself.
pub struct AppState {
    pub rules: Mutex<RuleSet>,
    pub engine: NftEngine,
    pub lock: Mutex<LockState>,
    pub last_heartbeat: Mutex<Option<Instant>>,
    pub resolver_config_path: PathBuf,
    /// When this `AppState` was constructed. Read by `guardian.rs` to
    /// grant a startup grace period before treating "no heartbeat has
    /// ever arrived" as "the extension is gone" rather than "the
    /// browser/extension just hasn't had time to connect yet."
    pub started_at: Instant,
}

/// How often the lock-expiry watcher checks whether a timed lock has
/// run out. Doesn't need to be tight — worst case this is how much
/// "grace" you get past your own configured unlock time (e.g. up to
/// ~15s late), which is fine for a focus tool.
const LOCK_WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Real bug this closes: `lock::effective_locked()` correctly treats
/// an expired timed lock as unlocked, and `server.rs` (`build_status`,
/// `reject_if_locked`) uses that everywhere it reports/gates on lock
/// state. But that's a pure computation — nothing actually calls
/// `engine.teardown()` or re-`apply()`s when the timer runs out.
/// `lock_ruleset` in server.rs only ever calls `engine.apply()` once,
/// at the moment `mindgate lock` succeeds. So the nft tables and the
/// dnsmasq blackhole are kernel/process-resident and just keep
/// running forever after the timer passes — `mindgate status` says
/// "unlocked" while the actual block stays live. That's exactly the
/// "lock doesn't get removed after time duration" bug.
///
/// This task polls `state.lock` every `LOCK_WATCH_INTERVAL` and, the
/// moment it finds a lock that is still flagged `.locked` on disk but
/// is no longer `effective_locked` (i.e. it had a timer and that timer
/// has passed), it:
///   1. clears the in-memory + persisted lock state via `lock::clear`
///      + `store::save_lock`, so a restart doesn't come back up
///      claiming to be locked, and
///   2. calls `engine.teardown()` to actually flush the nft tables and
///      kill the dnsmasq child, removing real enforcement.
///
/// Deliberately does NOT touch an untimed lock (`unlock_at: None`) —
/// `effective_locked` never flips that to false on its own, by design
/// (CONTEXT.md: "locked forever" stays locked until explicitly
/// cleared), so this watcher correctly never fires for it.
fn spawn_lock_watcher(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(LOCK_WATCH_INTERVAL);
        loop {
            interval.tick().await;

            let mut lock_state = state.lock.lock().await;
            let expired = lock_state.locked && !crate::lock::effective_locked(&lock_state);
            if !expired {
                continue;
            }

            tracing::info!(
                "timed lock has expired — clearing lock state and tearing down enforcement"
            );
            crate::lock::clear(&mut lock_state);
            if let Err(e) = store::save_lock(&lock_state).await {
                tracing::error!(
                    "failed to persist cleared lock state after expiry: {e:#} — will retry \
                     on the next watch tick"
                );
                // Don't teardown enforcement if we couldn't persist the
                // clear — better to stay blocked and retry the save
                // than to silently unblock while disk still says
                // "locked" (which a restart would then re-enforce
                // anyway, so at worst this is a delay, not a leak).
                continue;
            }
            drop(lock_state);

            state.engine.teardown().await;
            tracing::info!("enforcement torn down after lock expiry");
        }
    });
}

/// Waits for either SIGINT (Ctrl+C, or `kill -INT`) or SIGTERM (what
/// `systemctl stop` and a plain `kill <pid>` send). Returns once
/// either fires, so the caller can run cleanup and exit.
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!("failed to install SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("received SIGINT, shutting down gracefully"),
        _ = terminate => tracing::info!("received SIGTERM, shutting down gracefully"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let rules = store::load().await?;
    tracing::info!(
        "loaded {} rule(s) from {}",
        rules.total_rules(),
        rules_path().display()
    );

    // Load persisted lock state. This did NOT exist before — the lock
    // was previously always reset to LockState::default() on every
    // daemon start, which meant `lock forever` (or any timed lock)
    // silently un-locked itself on the next crash/restart. That
    // defeats the entire point of Lock Mode, so it's now persisted
    // the same way rules are — see store::load_lock/save_lock.
    let lock_state = store::load_lock().await?;
    tracing::info!(
        "loaded lock state from {} — locked: {}",
        mindgate_common::lock_state_path().display(),
        crate::lock::effective_locked(&lock_state)
    );

    let engine = NftEngine::default();
    let resolver_config_path = config_dir().join("resolver.conf");

    // Restore state after reboot/restart (CONTEXT.md §6): re-apply
    // whatever was persisted, before we start accepting connections.
    //
    // Changed: enforcement is now only re-armed on startup if the
    // ruleset is actually locked (`effective_locked`). Staged-but-
    // unlocked rules (added via `mindgate add`, not yet committed via
    // `mindgate lock`) must NOT be enforced — that's the whole point
    // of splitting `add` from `lock`. Applying them unconditionally on
    // every startup, like the old code did, would silently activate
    // rules the user never actually committed to.
    if crate::lock::effective_locked(&lock_state) {
        if let Err(e) = engine.apply(&rules, &resolver_config_path).await {
            tracing::error!("failed to re-apply rules on startup: {e:#}");
        }
    } else {
        tracing::info!("ruleset is not locked — skipping enforcement apply on startup");
    }

    let state = Arc::new(AppState {
        rules: Mutex::new(rules),
        engine,
        lock: Mutex::new(lock_state),
        last_heartbeat: Mutex::new(None),
        resolver_config_path,
        started_at: Instant::now(),
    });

    spawn_lock_watcher(state.clone());
    guardian::spawn(state.clone());
    self_watch::spawn();

    tokio::select! {
        result = server::run(state.clone()) => {
            result
        }
        _ = wait_for_shutdown_signal() => {
            tracing::info!("cleaning up: killing resolver child, flushing nft tables");
            state.engine.teardown().await;
            tracing::info!("shutdown complete");
            Ok(())
        }
    }
}