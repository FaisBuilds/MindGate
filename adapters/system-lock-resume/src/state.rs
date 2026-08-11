//! The state shared between the background watch task and the public,
//! synchronous [`crate::LockWatcher::is_locked`] reader.
//!
//! Two independent flags are tracked, not one:
//!
//! * `session_locked` — the logind session's own lock state (screensaver /
//!   manual lock).
//! * `system_suspended` — whether the system is currently asleep, per
//!   `PrepareForSleep`.
//!
//! They're combined with OR rather than folded into a single flag because
//! they change independently and a resume can race an unlock: if the
//! machine wakes from suspend, `system_suspended` clears, but the session
//! may still be genuinely locked and `session_locked` must keep reporting
//! that. Keeping the two flags separate means each one is only ever
//! written by the specific signal/property that owns it.

use std::sync::atomic::{AtomicBool, Ordering};

/// Ordering rationale: this is a single-writer-loop, many-reader flag pair,
/// not a synchronization primitive protecting other memory — so `Relaxed`
/// is sufficient. Every write already happens strictly after the async
/// D-Bus call that observed it, and every read only ever needs the latest
/// value, not a happens-before relationship with other state.
const ORDERING: Ordering = Ordering::Relaxed;

#[derive(Debug, Default)]
pub(crate) struct SharedState {
    session_locked: AtomicBool,
    system_suspended: AtomicBool,
}

impl SharedState {
    pub(crate) fn is_locked(&self) -> bool {
        self.session_locked.load(ORDERING) || self.system_suspended.load(ORDERING)
    }

    /// Sets the session-lock flag, logging only on an actual transition so
    /// reconciliation ticks that confirm the existing state don't spam
    /// `journalctl`.
    pub(crate) fn set_session_locked(&self, locked: bool, source: &'static str) {
        let previous = self.session_locked.swap(locked, ORDERING);
        if previous != locked {
            tracing::info!(
                locked,
                source,
                "system-lock-resume: session lock state changed"
            );
        }
    }

    /// Sets the system-suspended flag, logging only on an actual transition.
    pub(crate) fn set_system_suspended(&self, suspended: bool, source: &'static str) {
        let previous = self.system_suspended.swap(suspended, ORDERING);
        if previous != suspended {
            tracing::info!(
                suspended,
                source,
                "system-lock-resume: system suspend state changed"
            );
        }
    }

    /// Called when the D-Bus connection is lost or never established.
    /// Per the fail-safe requirement, an adapter that can't observe the
    /// real state reports "unlocked" rather than holding onto stale data.
    pub(crate) fn reset_to_unlocked(&self, reason: &'static str) {
        let was_locked = self.session_locked.swap(false, ORDERING);
        let was_suspended = self.system_suspended.swap(false, ORDERING);
        if was_locked || was_suspended {
            tracing::warn!(
                reason,
                was_locked,
                was_suspended,
                "system-lock-resume: lost D-Bus visibility, failing safe to unlocked"
            );
        }
    }
}