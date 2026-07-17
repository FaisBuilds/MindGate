//! Unix domain socket server. This is the only way in: the CLI, and
//! later the extension's native-messaging bridge mode, talk to
//! `mindgated` exclusively through here. Every mutation this module
//! accepts is persisted via `store::save` and pushed to the system via
//! `engine::NftEngine::apply` — nothing else in the daemon writes rules
//! or touches nftables.
//!
//! Authorization: every accepted connection's peer UID is checked via
//! `SO_PEERCRED` before any request on it is processed. Only the socket
//! owner (the daemon's own UID) or root may issue commands — this is
//! what stops an arbitrary local user from unblocking someone else's
//! session over the same socket.

use crate::lock;
use crate::AppState;
use anyhow::{Context, Result};
use mindgate_common::{socket_path, wire, Request, Response, StatusInfo};
use std::os::unix::io::AsRawFd;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// How stale a heartbeat can be before we consider the extension
/// disconnected for `Status` purposes. Not yet wired to the fail-closed
/// fallback described in CONTEXT.md §5 (that's post-MVP) — for now this
/// only affects what `mindgate status` reports.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(30);

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

    // If `mindgated` itself was launched via `sudo` (needed for the
    // nft/dnsmasq calls in engine.rs, which require root), `my_uid`
    // here is 0 — root. That collapses the check below to "peer must
    // be root," which permanently locks out every unprivileged
    // process the same human runs under their own account: the CLI,
    // and critically the browser extension's native-messaging bridge,
    // which Chrome always spawns as the logged-in desktop user, never
    // as root. `sudo` sets SUDO_UID to the original invoking user's
    // UID precisely so a root-elevated process can still recognize
    // "this is the same human, just unprivileged" — we use it here so
    // that user's own CLI/extension processes remain authorized even
    // though the daemon had to become root to do its job.
    let sudo_uid: Option<u32> = std::env::var("SUDO_UID").ok().and_then(|s| s.parse().ok());

    match peer_uid(stream) {
        Some(uid) => uid == my_uid || uid == 0 || Some(uid) == sudo_uid,
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
/// Removes a stale socket file left over from a previous run (e.g. an
/// unclean shutdown) before binding — otherwise the bind fails with
/// "address in use" even though nothing is actually listening.
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

    // The bind() above creates the socket file owned by whatever UID
    // mindgated itself is running as. When that's root (`sudo -E
    // target/debug/mindgated`, needed for the nft/dnsmasq calls in
    // engine.rs), the resulting socket defaults to owner-only write
    // access — which blocks every unprivileged connecting process,
    // including the browser extension's native-messaging bridge that
    // Chrome always spawns as the logged-in desktop user, never as
    // root. Real access control happens via SO_PEERCRED in
    // `authorized()` below, which checks the actual connecting
    // process's credentials — the file permissions here are
    // deliberately permissive so that check is what decides who's
    // allowed in, not the mode bits on the socket path.
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
                write_response(&mut stream, &Response::Error { message: format!("bad request: {e}") })
                    .await?;
                continue;
            }
        };
        let resp = dispatch(&state, req).await;
        write_response(&mut stream, &resp).await?;
    }
    Ok(())
}

async fn dispatch(state: &AppState, req: Request) -> Response {
    match req {
        Request::AddWebsite { domain } => {
            let mut rules = state.rules.lock().await;
            if !rules.websites.iter().any(|w| w.domain == domain) {
                rules.websites.push(mindgate_common::WebsiteRule { domain });
            }
            persist_and_apply(state, &rules).await
        }
        Request::RemoveWebsite { domain } => {
            let mut rules = state.rules.lock().await;
            rules.websites.retain(|w| w.domain != domain);
            persist_and_apply(state, &rules).await
        }
        Request::AddKeyword { value } => {
            let mut rules = state.rules.lock().await;
            if !rules.keywords.iter().any(|k| k.value == value) {
                rules.keywords.push(mindgate_common::KeywordRule { value });
            }
            persist_only(state, &rules).await
        }
        Request::RemoveKeyword { value } => {
            let mut rules = state.rules.lock().await;
            rules.keywords.retain(|k| k.value != value);
            persist_only(state, &rules).await
        }
        Request::AddSubreddit { subreddit } => {
            let mut rules = state.rules.lock().await;
            if !rules.subreddits.iter().any(|s| s.subreddit == subreddit) {
                rules.subreddits.push(mindgate_common::SubredditRule { subreddit });
            }
            persist_only(state, &rules).await
        }
        Request::RemoveSubreddit { subreddit } => {
            let mut rules = state.rules.lock().await;
            rules.subreddits.retain(|s| s.subreddit != subreddit);
            persist_only(state, &rules).await
        }
        Request::List => {
            let rules = state.rules.lock().await;
            Response::Rules(rules.clone())
        }
        Request::Status => build_status(state).await,
        Request::Lock { duration_secs, password } => {
            // Setting a lock never requires a password (you're choosing to
            // restrict your future self); it's *unlocking* that checks one.
            let _ = password;
            let mut lock_state = state.lock.lock().await;
            lock::lock(&mut lock_state, duration_secs, duration_secs.is_none());
            Response::Ok
        }
        Request::Unlock { password } => {
            let hash_path = mindgate_common::password_hash_path();
            match lock::verify_password(&hash_path, password.as_deref()).await {
                Ok(true) => {
                    let mut lock_state = state.lock.lock().await;
                    lock::clear(&mut lock_state);
                    Response::Ok
                }
                Ok(false) => Response::Error { message: "incorrect password".into() },
                Err(e) => Response::Error { message: format!("unlock failed: {e:#}") },
            }
        }
        Request::ExtensionHeartbeat => {
            *state.last_heartbeat.lock().await = Some(Instant::now());
            Response::Ok
        }
    }
}

async fn persist_and_apply(state: &AppState, rules: &mindgate_common::RuleSet) -> Response {
    if let Err(e) = crate::store::save(rules).await {
        return Response::Error { message: format!("failed to save rules: {e:#}") };
    }
    if let Err(e) = state.engine.apply(rules, &state.resolver_config_path).await {
        return Response::Error { message: format!("failed to apply rules: {e:#}") };
    }
    Response::Ok
}

/// Same as `persist_and_apply` but skips re-running the network engine
/// — used for keyword/subreddit changes, which the extension enforces,
/// not nftables. Still persists to disk so a daemon restart doesn't
/// lose them.
async fn persist_only(_state: &AppState, rules: &mindgate_common::RuleSet) -> Response {
    match crate::store::save(rules).await {
        Ok(()) => Response::Ok,
        Err(e) => Response::Error { message: format!("failed to save rules: {e:#}") },
    }
}

async fn build_status(state: &AppState) -> Response {
    let rules = state.rules.lock().await;
    let lock_state = state.lock.lock().await;
    let last_heartbeat = *state.last_heartbeat.lock().await;
    let extension_connected =
        last_heartbeat.map(|t| t.elapsed() < HEARTBEAT_TIMEOUT).unwrap_or(false);

    Response::Status(StatusInfo {
        daemon_running: true,
        nft_table_active: state.engine.nft_available().await,
        rule_count: rules.total_rules(),
        website_count: rules.websites.len(),
        keyword_count: rules.keywords.len(),
        subreddit_count: rules.subreddits.len(),
        extension_connected,
        lock: lock_state.clone(),
    })
}