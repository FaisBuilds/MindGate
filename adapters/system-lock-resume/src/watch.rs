//! The background task: connect to logind, subscribe to the fast-path
//! signals, and periodically reconcile against directly-queried properties
//! so a dropped signal can never permanently strand the reported state.

use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::StreamExt;
use zbus::Connection;

use crate::login1::{ManagerProxy, SessionProxy};
use crate::state::SharedState;

/// How often the fallback reconciliation directly queries current state via
/// D-Bus `Get`, instead of relying on signals alone.
const RECONCILE_INTERVAL: Duration = Duration::from_secs(12);

/// How long to wait before retrying after the D-Bus connection is lost or
/// never established (e.g. no logind on this platform).
const RECONNECT_BACKOFF: Duration = Duration::from_secs(15);

/// Runs forever: connect, watch, and on any failure, retry after a backoff.
/// 
/// CRITICAL FIX for XFCE light-locker freezing bug:
/// Only fail-safe to "unlocked" on FIRST connection failure (before we've ever
/// successfully connected). On reconnections after we've already been watching,
/// we keep the last-known state and try to reverify it. This prevents the race:
/// - Session is locked, browser frozen
/// - D-Bus connection drops (logind restart, etc)
/// - reset_to_unlocked() is called (too aggressive!)
/// - Daemon thinks session is unlocked and kills frozen browser
/// - User returns to find browser killed
/// 
/// By holding state on reconnect, we avoid false "unlocked" reports while
/// the session is genuinely locked. The reconciliation pass will reverify
/// the current state once reconnected.
pub(crate) async fn run(state: Arc<SharedState>) {
    let mut first_connection = true;

    loop {
        if let Err(error) = watch_once(&state).await {
            tracing::warn!(
                error = %error,
                "system-lock-resume: D-Bus watch loop ended, will retry"
            );
        }

        // Only fail-safe to unlocked on FIRST startup (before we've ever
        // successfully connected). On reconnects after we've already been
        // watching, keep the last-known state and try to reverify it.
        if first_connection {
            state.reset_to_unlocked("first connection attempt failed");
            first_connection = false;
        } else {
            tracing::info!(
                "system-lock-resume: D-Bus connection lost but keeping previous state \
                 until we can reverify it (prevented aggressive fail-safe during lock)"
            );
        }

        tokio::time::sleep(RECONNECT_BACKOFF).await;
    }
}

/// One connection lifetime: connect, resolve the session, subscribe, and
/// loop until something goes wrong. Returns `Err` on any failure that
/// should trigger a reconnect (connection drop, a signal stream ending, a
/// D-Bus call failing) — it never returns `Ok`.
async fn watch_once(state: &Arc<SharedState>) -> zbus::Result<()> {
    let connection = Connection::system().await.map_err(|error| {
        tracing::warn!(error = %error, "system-lock-resume: failed to connect to the system bus");
        error
    })?;

    let manager = ManagerProxy::new(&connection).await?;
    let session = resolve_session(&manager, &connection).await?;

    tracing::info!(
        session = %session.inner().path(),
        "system-lock-resume: connected to logind, watching session"
    );

    let mut sleep_signals = manager.receive_prepare_for_sleep().await?;
    let mut lock_signals = session.receive_lock().await?;
    let mut unlock_signals = session.receive_unlock().await?;

    let mut reconcile_ticker = tokio::time::interval(RECONCILE_INTERVAL);
    // The first tick fires immediately; skip it here because we do an
    // explicit reconciliation pass right below, before entering the loop,
    // so the reported state is accurate from the first instant rather than
    // waiting on either a signal or the first periodic tick.
    reconcile_ticker.reset_after(RECONCILE_INTERVAL);
    reconcile_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    reconcile(&manager, &session, state).await;

    loop {
        tokio::select! {
            signal = sleep_signals.next() => {
                let signal = signal.ok_or_else(|| stream_closed("PrepareForSleep"))?;
                let args = signal.args()?;
                state.set_system_suspended(args.start, "PrepareForSleep signal");
            }
            signal = lock_signals.next() => {
                signal.ok_or_else(|| stream_closed("Lock"))?;
                state.set_session_locked(true, "Lock signal");
            }
            signal = unlock_signals.next() => {
                signal.ok_or_else(|| stream_closed("Unlock"))?;
                state.set_session_locked(false, "Unlock signal");
            }
            _ = reconcile_ticker.tick() => {
                reconcile(&manager, &session, state).await;
            }
        }
    }
}

/// Directly queries current state via D-Bus `Get` and corrects `state` if
/// it disagrees with what a signal last reported.
///
/// The two halves of this are treated very differently, based on real
/// evidence from testing on XFCE + light-locker:
///
/// * `PreparingForSleep` (suspend) is reconciled bidirectionally — it can
///   both set and clear `system_suspended`. This is what makes a dropped
///   `PrepareForSleep(false)` (the confirmed systemd/systemd#30666 bug)
///   self-healing within one tick.
///
/// * `LockedHint` (session lock) is reconciled **one-directionally only**:
///   it may set `session_locked` from `false` to `true` (catching a missed
///   `Lock` signal), but it is never allowed to clear `session_locked` from
///   `true` to `false`. Only a genuinely received `Unlock` signal clears
///   it. This asymmetry exists because `Lock`/`Unlock` signals were
///   confirmed reliable in testing, while `LockedHint` was not: a real log
///   showed `Lock` firing correctly, followed seven seconds later by a
///   reconciliation pass reading `LockedHint == false` while the session
///   was still genuinely locked — light-locker (XFCE's screen locker) is
///   known to be inconsistent about keeping this property in sync, even
///   though it fires the signals correctly. Trusting `LockedHint` to
///   *clear* the lock state caused a real false-unlock, which in turn
///   caused an incorrect browser kill once the heartbeat went stale.
///
/// Each property is still queried independently: if one call fails (e.g. a
/// transient D-Bus timeout) the other still gets a chance to correct its
/// half of the state, and the failure is logged rather than aborting the
/// whole reconciliation pass. A hard connection failure will still surface
/// on the next signal-stream poll or method call in the main select loop.
async fn reconcile(manager: &ManagerProxy<'_>, session: &SessionProxy<'_>, state: &SharedState) {
    match manager.preparing_for_sleep().await {
        Ok(suspended) => state.set_system_suspended(suspended, "reconciliation"),
        Err(error) => tracing::warn!(
            error = %error,
            "system-lock-resume: reconciliation failed to read PreparingForSleep"
        ),
    }

    match session.locked_hint().await {
        Ok(true) => {
            // Upgrading false -> true is safe and desirable: it catches a
            // Lock signal we may have missed entirely.
            state.set_session_locked(true, "reconciliation");
        }
        Ok(false) => {
            // Deliberately NOT calling set_session_locked(false, ...) here.
            // See the doc comment above: LockedHint has been observed to
            // report false while the session is still genuinely locked
            // (light-locker doesn't reliably maintain this property).
            // Unlocking only ever happens via a real Unlock signal.
            // Still worth knowing about if it happens while we believe
            // we're locked, hence the warning rather than silence.
            if state.is_locked() {
                tracing::warn!(
                    "system-lock-resume: LockedHint reports false while session_locked \
                     is true — ignoring (known unreliable on this locker), waiting for \
                     an actual Unlock signal instead"
                );
            }
        }
        Err(error) => tracing::warn!(
            error = %error,
            "system-lock-resume: reconciliation failed to read LockedHint"
        ),
    }
}

/// Resolves the logind session object path for this process, preferring
/// `GetSessionByPID` (works when the daemon runs inside the user's login
/// session, e.g. as a systemd `--user` unit) and falling back to scanning
/// `ListSessions` for a seated session if that fails (e.g. the daemon runs
/// outside any session's cgroup). Builds and returns the `SessionProxy` for
/// the resolved path, logging its ID/seat/type along the way so the choice
/// can be sanity-checked by hand — see the `tracing::info!` at the bottom.
async fn resolve_session<'c>(
    manager: &ManagerProxy<'_>,
    connection: &'c Connection,
) -> zbus::Result<SessionProxy<'c>> {
    let pid = std::process::id();

    let path = match manager.get_session_by_pid(pid).await {
        Ok(path) => {
            tracing::debug!(
                pid,
                session = %path,
                "system-lock-resume: resolved session via GetSessionByPID"
            );
            path
        }
        Err(error) => {
            tracing::warn!(
                error = %error,
                pid,
                "system-lock-resume: GetSessionByPID failed, falling back to ListSessions"
            );

            let sessions = manager.list_sessions().await?;
            sessions
                .into_iter()
                .find(|(_, _, _, seat_id, _)| !seat_id.is_empty())
                .map(|(session_id, _, _, seat_id, path)| {
                    tracing::debug!(
                        session_id,
                        seat_id,
                        session = %path,
                        "system-lock-resume: resolved session via ListSessions fallback"
                    );
                    path
                })
                .ok_or_else(|| {
                    zbus::Error::Failure(
                        "system-lock-resume: no seated logind session found".to_string(),
                    )
                })?
        }
    };

    let session = SessionProxy::new(connection, path).await?;

    // Diagnostic only: read ID/seat/type once at startup so the resolved
    // session can be confirmed against `loginctl session-status <id>` run
    // by hand in another terminal. Each read is independent and a failure
    // here is logged but never fatal — losing this diagnostic shouldn't
    // block the watcher from starting.
    let id = match session.id().await {
        Ok(id) => id,
        Err(error) => {
            tracing::warn!(error = %error, "system-lock-resume: failed to read session Id");
            "<unreadable>".to_string()
        }
    };
    let seat = match session.seat().await {
        Ok((seat_id, _)) if !seat_id.is_empty() => seat_id,
        Ok(_) => "<unseated>".to_string(),
        Err(error) => {
            tracing::warn!(error = %error, "system-lock-resume: failed to read session Seat");
            "<unreadable>".to_string()
        }
    };
    let session_type = match session.session_type().await {
        Ok(t) => t,
        Err(error) => {
            tracing::warn!(error = %error, "system-lock-resume: failed to read session Type");
            "<unreadable>".to_string()
        }
    };

    tracing::info!(
        session_id = %id,
        seat = %seat,
        session_type = %session_type,
        session_path = %session.inner().path(),
        "system-lock-resume: resolved session at startup — cross-check with \
         `loginctl session-status <session_id>` in another terminal to confirm \
         this is your real graphical session, not a stale or wrong one"
    );

    Ok(session)
}

/// Builds the error used when a signal stream unexpectedly ends (e.g. the
/// bus connection was dropped). Treated the same as any other D-Bus
/// failure: it bubbles up to `run`'s reconnect loop.
fn stream_closed(signal_name: &str) -> zbus::Error {
    zbus::Error::Failure(format!(
        "system-lock-resume: {signal_name} signal stream ended unexpectedly"
    ))
}