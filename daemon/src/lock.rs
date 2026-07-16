//! Lock Mode primitives.
//!
//! Per CONTEXT.md §7, MVP1 does NOT include enforcement (the daemon
//! refusing rule mutations/uninstall while locked). What's implemented
//! here are the real primitives that enforcement will sit on top of
//! later: computing whether a timed lock has actually expired, and
//! verifying a password against the Argon2 hash on disk. This module
//! never claims a lock is unbypassable — see CONTEXT.md §6: Lock Mode
//! is friction, not a sandbox.

use anyhow::{Context, Result};
use argon2::password_hash::{PasswordHash, PasswordVerifier};
use argon2::Argon2;
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
        // No timer set — password-gated lock with no expiry, or a
        // freeform commitment. Stays locked until explicitly unlocked.
        None => true,
    }
}

/// Transition into a locked state. `duration_secs` of `None` means no
/// timer (only an explicit unlock, subject to `password_required`,
/// clears it).
pub fn lock(state: &mut LockState, duration_secs: Option<u64>, password_required: bool) {
    state.locked = true;
    state.unlock_at = duration_secs.map(|d| now_unix() + d);
    state.password_required = password_required;
}

/// Clear a lock unconditionally. Callers must have already verified
/// the password via `verify_password` if `state.password_required` was
/// set — this function itself does no verification, so it's not the
/// thing that should be exposed directly to untrusted input.
pub fn clear(state: &mut LockState) {
    state.locked = false;
    state.unlock_at = None;
    state.password_required = false;
}

/// Verify a candidate password against the Argon2 hash stored on disk.
/// If no hash file exists yet (no password ever set), any unlock
/// attempt succeeds — there's nothing to check against, and refusing
/// unlock in that case would strand a user who never opted into
/// password protection in the first place.
pub async fn verify_password(hash_path: &std::path::Path, candidate: Option<&str>) -> Result<bool> {
    let hash_str = match tokio::fs::read_to_string(hash_path).await {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e).context("failed to read password hash"),
    };

    let candidate = match candidate {
        Some(c) => c,
        None => return Ok(false), // hash exists, no password given — reject
    };

    let parsed_hash = PasswordHash::new(hash_str.trim())
        .map_err(|e| anyhow::anyhow!("stored password hash is malformed: {e}"))?;
    Ok(Argon2::default().verify_password(candidate.as_bytes(), &parsed_hash).is_ok())
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
        lock(&mut state, Some(0), false); // expires immediately
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(!effective_locked(&state));
    }

    #[test]
    fn timed_lock_still_effective_before_expiry() {
        let mut state = LockState::default();
        lock(&mut state, Some(3600), false);
        assert!(effective_locked(&state));
    }

    #[test]
    fn untimed_lock_stays_locked_until_cleared() {
        let mut state = LockState::default();
        lock(&mut state, None, true);
        assert!(effective_locked(&state));
        clear(&mut state);
        assert!(!effective_locked(&state));
    }

    #[tokio::test]
    async fn verify_password_allows_unlock_when_no_hash_set() {
        let path = std::env::temp_dir().join(format!("mindgate-nohash-{}", std::process::id()));
        let _ = tokio::fs::remove_file(&path).await;
        assert!(verify_password(&path, None).await.unwrap());
    }
}