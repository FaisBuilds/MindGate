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
    state_known: AtomicBool,
}

impl SharedState {
    pub(crate) fn is_locked(&self) -> bool {
        self.session_locked.load(ORDERING) || self.system_suspended.load(ORDERING)
    }

    pub(crate) fn is_known(&self) -> bool {
        self.state_known.load(ORDERING)
    }

    pub(crate) fn mark_known(&self) {
        self.state_known.store(true, ORDERING);
    }

    pub(crate) fn mark_unknown(&self) {
        self.state_known.store(false, ORDERING);
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

    /// Clears the state used by callers that need a fail-safe boolean. The
    /// separate visibility bit remains false until a complete reconciliation
    /// succeeds, so callers do not mistake this fallback for confirmation.
    pub(crate) fn reset_to_unlocked(&self, reason: &'static str) {
        self.mark_unknown();
        let was_locked = self.session_locked.swap(false, ORDERING);
        let was_suspended = self.system_suspended.swap(false, ORDERING);
        if was_locked || was_suspended {
            tracing::warn!(
                reason,
                was_locked,
                was_suspended,
                "system-lock-resume: lost D-Bus visibility, fallback state is unlocked"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SharedState;

    #[test]
    fn fallback_is_not_reported_as_confirmed_unlock() {
        let state = SharedState::default();
        assert!(!state.is_locked());
        assert!(!state.is_known());

        state.set_session_locked(true, "test");
        state.mark_known();
        assert!(state.is_locked());
        assert!(state.is_known());

        state.mark_unknown();
        assert!(state.is_locked());
        assert!(!state.is_known());
    }
}
