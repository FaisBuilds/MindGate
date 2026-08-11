mod guardian;
mod self_watch;
mod server;

use anyhow::Result;
use mindgate_common::LockState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Shared daemon state.
///
/// Per MVP1: The daemon deliberately knows almost nothing.
/// It only protects the browser extension.
pub struct AppState {
    /// Updated whenever the extension sends a heartbeat.
    pub last_heartbeat: Mutex<Option<Instant>>,

    /// Used by guardian.rs to avoid immediately closing browsers while
    /// the extension is still starting after boot.
    pub started_at: Instant,

    /// Prevents the guardian from killing browsers while the daemon is
    /// intentionally shutting down.
    pub shutting_down: AtomicBool,

    /// Tracks the current lock state reported by the extension.
    pub lock_state: Mutex<Option<LockState>>,
}

/// Waits for a termination signal (SIGINT or SIGTERM).
async fn wait_for_shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
        info!("received SIGINT");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                info!("received SIGTERM");
            }
            Err(e) => {
                warn!("failed to install SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing with RUST_LOG support (e.g., RUST_LOG=debug mindgated)
    // Defaults to 'info' level if not specified, ideal for systemd journal.
    tracing_subscriber::fmt::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    info!("MindGate daemon starting...");

    // Initialize shared state
    let state = Arc::new(AppState {
        last_heartbeat: Mutex::new(None),
        started_at: Instant::now(),
        shutting_down: AtomicBool::new(false),
        lock_state: Mutex::new(None),
    });

    // system-lock-resume adapter. Fails safe to "unlocked" on its own
    // (see adapters/system-lock-resume) — a problem in this adapter
    // degrades guardian back to its pre-adapter behavior, never to a
    // new failure mode.
    let lock_watcher = system_lock_resume::spawn().await;

    // Spawn background protection and health tasks
    guardian::spawn(state.clone(), lock_watcher);
    self_watch::spawn();

    // Run the Unix Domain Socket server and wait for shutdown
    let server_future = server::run(state.clone());
    let shutdown_future = wait_for_shutdown_signal();

    tokio::select! {
        result = server_future => {
            if let Err(e) = result {
                warn!("server exited with error: {e}");
            }
        }
        _ = shutdown_future => {
            info!("MindGate daemon shutting down...");
            state.shutting_down.store(true, Ordering::Release);

            // Give guardian a brief moment to observe the shutting_down flag
            // before the process actually exits, preventing accidental browser kills.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            info!("Shutdown complete.");
        }
    }

    Ok(())
}