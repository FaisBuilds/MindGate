//! Lock Mode primitives.
//!
//! Computing whether a timed lock has actually expired — the real
//! primitive enforcement (in `main.rs`'s lock-expiry watcher and
//! `server.rs`'s `reject_if_locked`/`lock_ruleset`) sits on top of.
//! This module never claims a lock is unbypassable — see
//! CONTEXT.md §6: Lock Mode is friction, not a sandbox.
//!
//! REMOVED: password-gated unlocking (`verify_password`, the Argon2
//! hash check, `password_required` on `LockState`). There was no
//! actual unlock path in this design to begin with — a lock only ever
//! clears itself once its own timer expires (or never, for `lock
//! forever`) — so a password to gate an unlock that doesn't exist was
//! dead code from day one. `server.rs` already treated it as such
//! (`let _ = password;` on `Lock`, and `Unlock` was a hardcoded
//! rejection). Removed rather than left "for later," matching the
//! current one-way-ratchet design rather than a password-recovery
//! design that was never actually wired up.

use mindgate_common::LockState;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Whether the lock is *actually* in effect right now, accounting for
/// a timed lock that has already expired. `LockState.locked` on disk
/// can be stale (e.g. daemon was down when the timer expired), so
/// callers should use this instead of reading `.locked` directly.
pub fn effective_locked(state: &LockState) -> bool {
    if !state.locked {
        return false;
    }
    match state.unlock_at {
        Some(unlock_at) => now_unix() < unlock_at,
        // No timer set — locked until explicitly cleared (only ever
        // happens via a timer expiring; `lock forever` never clears
        // itself at all — see `clear`'s doc comment).
        None => true,
    }
}

/// Transition into a locked state. `duration_secs` of `None` means no
/// timer — stays locked until the daemon itself is torn down (there is
/// no unlock command).
pub fn lock(state: &mut LockState, duration_secs: Option<u64>) {
    state.locked = true;
    state.unlock_at = duration_secs.map(|d| now_unix() + d);
}

/// Clear a lock. Currently only called from `main.rs`'s lock-expiry
/// watcher, once a timed lock's clock has actually run out — there is
/// still no user-facing "unlock early" path.
pub fn clear(state: &mut LockState) {
    state.locked = false;
    state.unlock_at = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocked_state_is_never_effective_locked() {
        let state = LockState::default();
        assert!(!effective_locked(&state));
    }

    #[test]
    fn timed_lock_expires() {
        let mut state = LockState::default();
        lock(&mut state, Some(0)); // expires immediately
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!effective_locked(&state));
    }

    #[test]
    fn timed_lock_still_effective_before_expiry() {
        let mut state = LockState::default();
        lock(&mut state, Some(3600));
        assert!(effective_locked(&state));
    }

    #[test]
    fn untimed_lock_stays_locked_until_cleared() {
        let mut state = LockState::default();
        lock(&mut state, None);
        assert!(effective_locked(&state));
        clear(&mut state);
        assert!(!effective_locked(&state));
    }
}