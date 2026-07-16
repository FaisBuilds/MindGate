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

    server::run(state).await
}