mod engine;
mod lock;
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
}

/// Waits for either SIGINT (Ctrl+C, or `kill -INT`) or SIGTERM (what
/// `systemctl stop` and a plain `kill <pid>` send). Returns once
/// either fires, so the caller can run cleanup and exit.
///
/// SIGINT alone (via `tokio::signal::ctrl_c()`) isn't enough on its
/// own: that only catches Ctrl+C in a terminal. `systemctl stop` and a
/// bare `kill` send SIGTERM, which Rust does NOT handle gracefully by
/// default — an un-caught SIGTERM kills the process immediately,
/// skipping any cleanup code entirely, which is exactly how we ended
/// up with orphaned dnsmasq children and stale nft tables in the
/// first place. Both signals need an explicit handler.
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
                // Fall back to never resolving on this branch — ctrl_c
                // above still works, we just lose SIGTERM handling.
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
    // Initialize tracing (ensure tracing-subscriber is in daemon/Cargo.toml dependencies)
    tracing_subscriber::fmt::init();

    let rules = store::load().await?;
    tracing::info!(
        "loaded {} rule(s) from {}",
        rules.total_rules(),
        rules_path().display()
    );

    let engine = NftEngine::default();
    let resolver_config_path = config_dir().join("resolver.conf");

    // Restore state after reboot/restart (CONTEXT.md §6): re-apply
    // whatever was persisted, before we start accepting connections. A
    // failure here is logged, not fatal — the daemon should still come
    // up and serve the socket even if nft/dnsmasq are unavailable
    // (dry-run mode) or a rule fails to apply.
    if let Err(e) = engine.apply(&rules, &resolver_config_path).await {
        tracing::error!("failed to apply rules on startup: {e:#}");
    }

    let state = Arc::new(AppState {
        rules: Mutex::new(rules),
        engine,
        lock: Mutex::new(LockState::default()),
        last_heartbeat: Mutex::new(None),
        resolver_config_path,
    });

    // Race the socket server against the shutdown signal. On a
    // graceful stop (Ctrl+C, `systemctl stop`, plain `kill`), we tear
    // down the resolver child and flush both nft tables BEFORE
    // exiting — this is what an ungraceful death (SIGKILL, panic,
    // OOM-kill) can never give us, so it's not redundant with the
    // orphan self-healing already in `engine.rs`; it's the other half
    // of the fix. That half (`ensure_port_clear` + the post-spawn
    // liveness check) still stands as the safety net for the cases
    // this handler can't catch.
    //
    // `server::run` is expected to run until the process is killed —
    // if it returns on its own (e.g. socket bind failure), that's
    // still surfaced as an error from `main`, same as before.
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