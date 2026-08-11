//! `system-lock-resume` — a MindGate adapter, per `ADAPTER.md`.
//!
//! Answers one question: **is the user's Linux session currently locked, or
//! is the system currently suspended?** Nothing else. It does not decide
//! what to block, does not talk to the extension, and does not read or
//! write MindGate's lock state — see `SCOPE.md` for what a MindGate "lock"
//! guarantees; this crate is a signal an eventual core integration could
//! use, not part of the enforcement path itself. That wiring is a separate,
//! explicit step, deliberately not done here (see the crate-level README /
//! the accompanying chat message for why).
//!
//! Goes through `systemd-logind` over D-Bus (`org.freedesktop.login1`)
//! rather than any desktop-environment-specific screensaver API, which is
//! what makes it work the same way on GNOME, KDE, and XFCE.
//!
//! # Reliability design
//!
//! Two things make this hold up over long uptimes, not just at first
//! connection:
//!
//! * **Signals are the fast path, not the only path.** In addition to
//!   listening for `PrepareForSleep` and `Lock`/`Unlock`, a periodic
//!   reconciliation directly queries current state via D-Bus `Get`
//!   (`PreparingForSleep` on the Manager, `LockedHint` on the Session).
//!   This matters concretely: systemd has a confirmed bug
//!   ([systemd/systemd#30666](https://github.com/systemd/systemd/issues/30666))
//!   where `PrepareForSleep(false)` is not always emitted after resuming
//!   from suspend, which would otherwise strand a pure signal-listener
//!   reporting "suspended" forever. The reconciliation pass makes that
//!   self-healing within one tick.
//! * **Every failure mode fails to `false` (unlocked).** A D-Bus
//!   connection error, a timeout, or running on a platform without
//!   `logind` all result in [`LockWatcher::is_locked`] reporting `false`,
//!   never a stale `true`.
//!
//! # Usage
//!
//! ```no_run
//! # async fn example() {
//! let watcher = system_lock_resume::spawn().await;
//! if watcher.is_locked() {
//!     // session is locked, or the system is suspended
//! }
//! # }
//! ```

mod login1;
mod state;
mod watch;


use std::sync::Arc;

use tokio::task::JoinHandle;

use state::SharedState;

/// Handle to the background logind watcher. Cheap to hold onto; the only
/// thing it exposes is [`is_locked`](LockWatcher::is_locked).
///
/// Dropping this handle stops the background task — MindGate's daemon is
/// expected to hold one for its entire lifetime (see `AppState` in
/// `main.rs`, which is exactly the kind of place a future, explicitly
/// approved wiring step would store it), so this only matters for tests
/// and short-lived callers.
pub struct LockWatcher {
    state: Arc<SharedState>,
    task: JoinHandle<()>,
}

impl LockWatcher {
    /// Whether the user's session is currently locked, or the system is
    /// currently suspended. Synchronous and non-blocking: it reads a
    /// shared flag kept up to date by the background task started in
    /// [`spawn`], rather than making a D-Bus call itself.
    ///
    /// Defaults to `false` before the first successful connection, and
    /// fails safe to `false` on any D-Bus error — see the crate-level
    /// docs. Callers that need to distinguish "confirmed unlocked" from
    /// "we don't currently know" are out of scope for this crate; it only
    /// promises the fail-safe boolean.
    pub fn is_locked(&self) -> bool {
        self.state.is_locked()
    }
}

impl Drop for LockWatcher {
    fn drop(&mut self) {
        // Not required for the daemon's normal lifetime (the watcher lives
        // as long as the process), but keeps tests and any short-lived
        // caller from leaking a task that polls D-Bus forever.
        self.task.abort();
    }
}

/// Starts the background logind watcher and returns a handle to it.
///
/// Never fails: if the system bus can't be reached right away (or ever,
/// e.g. on a platform without `logind`), the returned [`LockWatcher`]
/// simply reports `is_locked() == false` and the background task keeps
/// retrying on a backoff. See the crate-level docs for the fail-safe and
/// reconciliation design.
///
/// Must be called from within a Tokio runtime (it uses [`tokio::spawn`]).
pub async fn spawn() -> LockWatcher {
    let state = Arc::new(SharedState::default());
    let task = tokio::spawn(watch::run(state.clone()));

    LockWatcher { state, task }
}