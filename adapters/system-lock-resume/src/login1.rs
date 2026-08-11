//! D-Bus proxy trait definitions for `org.freedesktop.login1`, kept
//! separate from `watch.rs` so the wire shapes being relied on are
//! auditable on their own, without the connect/reconcile control flow
//! around them.
//!
//! Method, signal, and property names are matched against the real
//! `org.freedesktop.login1` interface as published by systemd — see
//! `man 5 org.freedesktop.login1`. zbus's `#[proxy]` macro converts a
//! `snake_case` Rust method name to `UpperCamelCase` for the D-Bus member
//! name by default; the two places that don't round-trip correctly
//! (`GetSessionByPID`'s non-standard capitalization, and `Type` being a
//! Rust keyword) are called out explicitly below with a `name` override.

use zbus::zvariant::OwnedObjectPath;

/// `org.freedesktop.login1.Manager` — the well-known singleton object at
/// `/org/freedesktop/login1` that session/seat/sleep operations go through.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
pub(crate) trait Manager {
    /// Resolves the logind session object path for a given PID. Naive
    /// `snake_case` -> `UpperCamelCase` conversion would produce the
    /// non-existent `GetSessionByPid`; the real D-Bus method capitalizes
    /// the acronym, hence the explicit override.
    #[zbus(name = "GetSessionByPID")]
    fn get_session_by_pid(&self, pid: u32) -> zbus::Result<OwnedObjectPath>;

    /// Fallback used when `GetSessionByPID` fails (e.g. the daemon isn't
    /// running inside any session's cgroup). Tuple fields, per the
    /// `org.freedesktop.login1.Manager.ListSessions` signature, are
    /// `(session_id, user_id, user_name, seat_id, object_path)`.
    fn list_sessions(
        &self,
    ) -> zbus::Result<Vec<(String, u32, String, String, OwnedObjectPath)>>;

    /// Fired both when a suspend/hibernate is about to happen (`start ==
    /// true`) and again once it completes (`start == false`) — see the
    /// crate-level docs for why `watch.rs` treats the second half of this
    /// pair as unreliable and reconciles against `preparing_for_sleep`
    /// below rather than trusting it alone.
    #[zbus(signal)]
    fn prepare_for_sleep(&self, start: bool) -> zbus::Result<()>;

    /// Directly queryable mirror of the signal above — the reconciliation
    /// pass in `watch.rs` calls this instead of only trusting a delivered
    /// signal, since a dropped `PrepareForSleep(false)` is a confirmed,
    /// filed systemd bug (systemd/systemd#30666).
    #[zbus(property)]
    fn preparing_for_sleep(&self) -> zbus::Result<bool>;
}

/// `org.freedesktop.login1.Session` — per-session object, resolved to a
/// specific object path at runtime by `watch::resolve_session`, so this
/// proxy (unlike `ManagerProxy`) has no `default_path` and is constructed
/// with `SessionProxy::new(connection, path)`.
#[zbus::proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
pub(crate) trait Session {
    /// Fired when the session is locked (manual lock or screensaver
    /// activation). No arguments.
    #[zbus(signal)]
    fn lock(&self) -> zbus::Result<()>;

    /// Fired when the session is unlocked. No arguments.
    #[zbus(signal)]
    fn unlock(&self) -> zbus::Result<()>;

    /// Directly queryable mirror of the Lock/Unlock signals, used the same
    /// way `preparing_for_sleep` is: as the reconciliation pass's source
    /// of truth, not just a cache of the last signal seen.
    #[zbus(property)]
    fn locked_hint(&self) -> zbus::Result<bool>;

    /// The session's own ID (e.g. `"3"`), used only for the diagnostic
    /// startup log in `resolve_session` so the resolved session can be
    /// hand-verified against `loginctl session-status <id>`.
    #[zbus(property)]
    fn id(&self) -> zbus::Result<String>;

    /// `(seat_id, seat_object_path)`. An empty `seat_id` means an
    /// unseated session (e.g. an SSH login) — `resolve_session` treats
    /// that as a signal it may have resolved the wrong session and logs
    /// it as `<unseated>`.
    #[zbus(property)]
    fn seat(&self) -> zbus::Result<(String, OwnedObjectPath)>;

    /// Maps to the D-Bus `Type` property (e.g. `"wayland"`, `"x11"`,
    /// `"tty"`). Named `session_type` here since `type` is a Rust
    /// keyword, hence the explicit override back to the real property
    /// name.
    #[zbus(property, name = "Type")]
    fn session_type(&self) -> zbus::Result<String>;
}