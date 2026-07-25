//! Unix domain socket server. This is the only way in: the CLI, and
//! the browser extension's native-messaging bridge, talk to
//! `mindgated` exclusively through here.
//!
//! MVP1 scope cut: no rule-mutation requests anymore (previously
//! `AddWebsite` / `AddKeyword` / `AddPath` / `RemovePath` / `List` /
//! `Lock`). Per the MVP1 doc: "The daemon never decides what to
//! block. It only protects the blocker." — so it has nothing to store
//! or serve back about rules. The only two requests left are `Status`
//! (for `mindgate status` / `doctor`) and `ExtensionHeartbeat` (so
//! `guardian.rs` knows the extension is alive).
//!
//! Authorization: every accepted connection's peer UID is checked via
//! `SO_PEERCRED` before any request on it is processed. Only the
//! socket owner (the daemon's own UID) or root may issue commands —
//! this is what stops an arbitrary local user from spoofing a
//! heartbeat or reading status for someone else's session.

use crate::AppState;
use anyhow::{Context, Result};
use mindgate_common::{socket_path, wire, Request, Response, StatusInfo};
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// How stale a heartbeat can be before the extension is considered
/// disconnected — shared by both `mindgate status`'s report AND
/// `guardian.rs`'s browser-close trigger, so the two can never
/// disagree about whether the extension is connected.
///
/// 150s: comfortable headroom over the extension's real ~60s
/// `chrome.alarms` sync cadence (Chrome enforces a 1-minute floor on
/// alarm periods for installed extensions) plus scheduling jitter,
/// without being so loose that a genuinely dead extension takes
/// minutes to show up as gone.
pub(crate) const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(80);

/// Look up the connecting peer's UID via `SO_PEERCRED`. Returns `None`
/// if the platform/socket doesn't support it, which callers treat as
/// "reject" rather than "allow" — fail closed.
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    unsafe {
        let mut cred: libc::ucred = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let ret = libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        );
        if ret == 0 {
            Some(cred.uid)
        } else {
            None
        }
    }
}

fn authorized(stream: &UnixStream) -> bool {
    let my_uid = unsafe { libc::getuid() };

    // If `mindgated` itself was launched via `sudo` (dev workflow),
    // `SUDO_UID` identifies the real human behind the elevation.
    let sudo_uid: Option<u32> = std::env::var("SUDO_UID").ok().and_then(|s| s.parse().ok());

    // Production equivalent: install.sh writes the installing human's
    // UID into an EnvironmentFile the systemd unit loads (see
    // `installer/install.sh` and `mindgated.service`), since systemd
    // starts the daemon directly as root with no sudo layer to read
    // `SUDO_UID` from.
    let owner_uid: Option<u32> =
        std::env::var("MINDGATE_OWNER_UID").ok().and_then(|s| s.parse().ok());

    match peer_uid(stream) {
        Some(uid) => uid == my_uid || uid == 0 || Some(uid) == sudo_uid || Some(uid) == owner_uid,
        None => false,
    }
}

async fn read_frame(stream: &mut UnixStream) -> Result<Option<Vec<u8>>> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e).context("failed to read frame length"),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.context("failed to read frame body")?;
    Ok(Some(body))
}

async fn write_response(stream: &mut UnixStream, resp: &Response) -> Result<()> {
    let bytes = wire::encode(resp)?;
    stream.write_all(&bytes).await.context("failed to write response")?;
    Ok(())
}

/// Bind the socket and serve connections until the process exits.
/// Removes a stale socket file left over from a previous run before
/// binding — otherwise the bind fails with "address in use" even
/// though nothing is actually listening.
pub async fn run(state: Arc<AppState>) -> Result<()> {
    let path = socket_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if path.exists() {
        tokio::fs::remove_file(&path)
            .await
            .with_context(|| format!("failed to remove stale socket {}", path.display()))?;
    }

    let listener = UnixListener::bind(&path)
        .with_context(|| format!("failed to bind {}", path.display()))?;

    // Permissive file mode is deliberate: real access control happens
    // via SO_PEERCRED in `authorized()` above, which checks the
    // actual connecting process's credentials, not the mode bits on
    // the socket path (the daemon runs as root, which would otherwise
    // lock out the unprivileged native-messaging bridge process).
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666))
        .await
        .with_context(|| format!("failed to set permissions on {}", path.display()))?;

    tracing::info!("listening on {}", path.display());

    loop {
        let (stream, _addr) = listener.accept().await.context("accept failed")?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, state).await {
                tracing::warn!("connection error: {e:#}");
            }
        });
    }
}

async fn handle_connection(mut stream: UnixStream, state: Arc<AppState>) -> Result<()> {
    if !authorized(&stream) {
        tracing::warn!("rejected connection: peer UID not authorized");
        let _ = write_response(
            &mut stream,
            &Response::Error { message: "not authorized".into() },
        )
        .await;
        return Ok(());
    }

    while let Some(body) = read_frame(&mut stream).await? {
        let req: Request = match wire::decode(&body) {
            Ok(r) => r,
            Err(e) => {
                let _ = write_response(
                    &mut stream,
                    &Response::Error { message: format!("bad request: {e:#}") },
                )
                .await;
                continue;
            }
        };

        let resp = dispatch(req, &state).await;
        write_response(&mut stream, &resp).await?;
    }

    Ok(())
}

async fn dispatch(req: Request, state: &AppState) -> Response {
    use std::sync::atomic::Ordering;

    match req {
        Request::Status => build_status(state).await,
        
        // UPDATED: Accept and save the lock_state from the extension heartbeat
        Request::ExtensionHeartbeat { lock_state } => {
            *state.last_heartbeat.lock().await = Some(Instant::now());
            if let Some(ls) = lock_state {
                *state.lock_state.lock().await = Some(ls);
            }
            Response::Ok
        }
        
        Request::Ping => Response::Pong,
        Request::Version => Response::Version {
            daemon: env!("CARGO_PKG_VERSION").to_string(),
            protocol: 1,
        },
        
        // UPDATED: Intercept Shutdown and reject if locked
        Request::Shutdown => {
            let current_lock = state.lock_state.lock().await.clone();
            
            if let Some(lock) = current_lock {
                if lock.locked {
                    let now_ms = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64;
                    
                    let is_expired = if let Some(unlock_at) = lock.unlock_at {
                        unlock_at <= now_ms
                    } else {
                        false // "forever" lock never expires
                    };
                    
                    if !is_expired {
                        tracing::warn!("Shutdown rejected: MindGate is currently locked");
                        return Response::Error {
                            message: "MindGate is currently locked. Shutdown is disabled.".into(),
                        };
                    }
                }
            }
            
            tracing::info!("Shutdown requested via CLI");
            state.shutting_down.store(true, Ordering::Release);
            Response::Ok
        }
    }
}

async fn build_status(state: &AppState) -> Response {
    let last_heartbeat = *state.last_heartbeat.lock().await;
    let extension_connected =
        last_heartbeat.map(|t| t.elapsed() < HEARTBEAT_TIMEOUT).unwrap_or(false);
    
    // UPDATED: Fetch and include the current lock state in the status response
    let lock_state = state.lock_state.lock().await.clone();

    Response::Status(StatusInfo {
        daemon_running: true,
        extension_connected,
        lock_state,
    })
}