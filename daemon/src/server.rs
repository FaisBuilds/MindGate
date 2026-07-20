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
///
/// FIX: this was 30s, matching what `background.js` originally asked
/// `chrome.alarms` for (`periodInMinutes: 0.5`). But Chrome enforces a
/// hard 1-minute floor on alarm periods for installed extensions —
/// asking for 30s doesn't get you 30s, it silently gets clamped to
/// ~60s (plus scheduling jitter on top of that). So the real heartbeat
/// cadence was always ~60s+, while this only tolerated a 30s gap —
/// meaning `mindgate status` was stale (and thus wrongly "NO") for
/// roughly the back half of every single cycle, sync/extension working
/// correctly the entire time. 150s gives comfortable headroom over the
/// real ~60s cadence plus jitter, without being so loose that a truly
/// dead extension takes minutes to show up as disconnected.
///
/// Also reused by `guardian.rs` as the definition of "extension is
/// missing" for the browser-kill fallback — one definition of "stale"
/// shared by both `mindgate status` and the guardian, so they can
/// never disagree about whether the extension is connected.
pub(crate) const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(150);

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
        // --- The ratchet ---
        //
        // `add` is ALWAYS allowed, locked or not — you can only ever
        // tighten a committed ruleset, never loosen it. Before a lock
        // exists, `add` just stages (writes to rules.toml, nothing
        // enforced — that's what `mindgate lock` activates). Once
        // locked, `add` still writes to rules.toml, but for a website
        // it now ALSO calls `engine.apply()` immediately, so the new
        // block takes effect right away rather than waiting for
        // another `lock` call that (deliberately) can't happen again.
        // Keyword/subreddit adds while locked don't need an explicit
        // apply step — the extension already treats "locked" as "sync
        // and enforce," so a newly staged keyword just gets picked up
        // on its next periodic sync.
        //
        // `remove` is the actual commitment mechanism: rejected
        // outright whenever `lock::effective_locked` is true. There's
        // deliberately no `unlock` — a lock clears itself only once
        // its timer naturally expires, or never, for `lock forever`.
        Request::AddWebsite { domain } => {
            let mut rules = state.rules.lock().await;
            if !rules.websites.iter().any(|w| w.domain == domain) {
                rules.websites.push(mindgate_common::WebsiteRule { domain });
            }
            add_and_maybe_apply(state, &rules).await
        }
        Request::RemoveWebsite { domain } => {
            if let Some(resp) = reject_if_locked(state).await {
                return resp;
            }
            let mut rules = state.rules.lock().await;
            rules.websites.retain(|w| w.domain != domain);
            persist_only(&rules).await
        }
        Request::AddKeyword { value } => {
            let mut rules = state.rules.lock().await;
            if !rules.keywords.iter().any(|k| k.value == value) {
                rules.keywords.push(mindgate_common::KeywordRule { value });
            }
            persist_only(&rules).await
        }
        Request::RemoveKeyword { value } => {
            if let Some(resp) = reject_if_locked(state).await {
                return resp;
            }
            let mut rules = state.rules.lock().await;
            rules.keywords.retain(|k| k.value != value);
            persist_only(&rules).await
        }
        // The one and only path-level "add a rule under a domain"
        // primitive — no Reddit-specific command exists or is planned.
        // `mindgate add path reddit.com/r/gaming` and `mindgate add
        // path youtube.com/shorts` both land here identically.
        Request::AddPath { domain, path } => {
            let mut rules = state.rules.lock().await;
            if !rules.paths.iter().any(|p| p.domain == domain && p.path == path) {
                rules.paths.push(mindgate_common::PathRule { domain, path });
            }
            persist_only(&rules).await
        }
        Request::RemovePath { domain, path } => {
            if let Some(resp) = reject_if_locked(state).await {
                return resp;
            }
            let mut rules = state.rules.lock().await;
            rules.paths.retain(|p| !(p.domain == domain && p.path == path));
            persist_only(&rules).await
        }
        Request::List => {
            let rules = state.rules.lock().await;
            Response::Rules(rules.clone())
        }
        Request::Status => build_status(state).await,

        // --- The one and only activation path ---
        Request::Lock { duration_secs } => lock_ruleset(state, duration_secs).await,

        Request::ExtensionHeartbeat => {
            *state.last_heartbeat.lock().await = Some(Instant::now());
            Response::Ok
        }
    }
}

/// The ratchet's "add" half. Always persists the (already-mutated)
/// ruleset to disk — staging is never blocked by lock state. If the
/// ruleset is *currently* locked, this ALSO calls `engine.apply()`
/// immediately, so a website added mid-lock takes effect right away
/// instead of silently waiting for a `lock` call that can't happen
/// again (there's no re-lock while already locked — see
/// `lock_ruleset`'s own guard).
///
/// Only called from `Request::AddWebsite`. Keyword/subreddit adds
/// don't need this — they go through `persist_only` directly, because
/// the extension itself re-syncs and re-enforces on every periodic
/// poll while locked (see `background.js`'s `syncRules`), so a newly
/// staged keyword gets picked up on its own without an explicit apply
/// step here.
///
/// Deliberately fail-visible, matching `lock_ruleset`'s own posture:
/// if `engine.apply()` fails while locked, we report that error rather
/// than silently pretending the add succeeded — the caller needs to
/// know the new site is staged but NOT yet actually blocked.
async fn add_and_maybe_apply(state: &AppState, rules: &mindgate_common::RuleSet) -> Response {
    if let Err(e) = crate::store::save(rules).await {
        return Response::Error { message: format!("failed to save rules: {e:#}") };
    }

    let locked = {
        let lock_state = state.lock.lock().await;
        lock::effective_locked(&lock_state)
    };

    if locked {
        if let Err(e) = state.engine.apply(rules, &state.resolver_config_path).await {
            return Response::Error {
                message: format!(
                    "site staged and saved, but failed to apply enforcement while \
                     locked: {e:#} — it will not be blocked until this is resolved"
                ),
            };
        }
    }

    Response::Ok
}

/// Returns `Some(Response::Error{..})` if the ruleset is currently
/// locked (using `lock::effective_locked`, which correctly treats an
/// expired timed lock as unlocked even if the on-disk flag hasn't
/// been proactively cleared), or `None` if the caller should proceed.
async fn reject_if_locked(state: &AppState) -> Option<Response> {
    let lock_state = state.lock.lock().await;
    if lock::effective_locked(&lock_state) {
        let detail = match lock_state.unlock_at {
            Some(unlock_at) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("locked for {} more second(s)", unlock_at.saturating_sub(now))
            }
            None => "locked forever".to_string(),
        };
        Some(Response::Error {
            message: format!("ruleset is {detail} — cannot modify while locked"),
        })
    } else {
        None
    }
}

/// The only place `engine.apply()` is ever called after startup.
/// Deliberately fail-closed: enforcement is attempted FIRST, and the
/// lock is only recorded (in memory + on disk) if it actually
/// succeeds. This guarantees "the ruleset reports locked" always
/// means "enforcement is actually active" — never a state where the
/// CLI claims success but nothing is really blocked.
async fn lock_ruleset(state: &AppState, duration_secs: Option<u64>) -> Response {
    {
        let lock_state = state.lock.lock().await;
        if lock::effective_locked(&lock_state) {
            return Response::Error {
                message: "ruleset is already locked — wait for it to expire".into(),
            };
        }
    }

    let rules = state.rules.lock().await;
    if let Err(e) = state.engine.apply(&rules, &state.resolver_config_path).await {
        return Response::Error { message: format!("failed to apply rules: {e:#}") };
    }
    drop(rules);

    let mut lock_state = state.lock.lock().await;
    lock::lock(&mut lock_state, duration_secs);

    if let Err(e) = crate::store::save_lock(&lock_state).await {
        // Enforcement is already active at this point (engine.apply
        // succeeded above) but we couldn't persist the lock record —
        // surface this loudly rather than silently: a restart right
        // now would come back up unlocked despite nft/dnsmasq still
        // reflecting a locked ruleset until the next apply.
        return Response::Error {
            message: format!(
                "enforcement is active but failed to persist lock state: {e:#} — \
                 a daemon restart before this is fixed will NOT preserve the lock"
            ),
        };
    }

    Response::Ok
}

/// Persist the ruleset to disk. This is now the ONLY thing `add`/
/// `remove` requests do — no `engine.apply()` call, ever. See the
/// module-level split described in `dispatch`'s doc comment above.
async fn persist_only(rules: &mindgate_common::RuleSet) -> Response {
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

    // Report EFFECTIVE lock status, not the raw on-disk flag — a timed
    // lock whose timer has already passed should read as unlocked here
    // even if nothing has proactively cleared `.locked` yet. This is
    // also what the extension relies on (via a Status check) to decide
    // whether to actually enforce its synced keyword/subreddit rules.
    let mut reported_lock = lock_state.clone();
    reported_lock.locked = lock::effective_locked(&lock_state);

    Response::Status(StatusInfo {
        daemon_running: true,
        nft_table_active: state.engine.nft_available().await,
        rule_count: rules.total_rules(),
        website_count: rules.websites.len(),
        keyword_count: rules.keywords.len(),
        subreddit_count: rules.subreddits.len(),
        path_count: rules.paths.len(),
        extension_connected,
        lock: reported_lock,
    })
}