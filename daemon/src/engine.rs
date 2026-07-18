//! The Block Engine.
//!
//! This is the ONLY module that touches the system: it shells out to
//! `nft` and manages a local DNS resolver (`dnsmasq`) as a child
//! process. Nothing else in the daemon writes firewall rules or DNS
//! config directly — everything goes through `NftEngine::apply()`.
//!
//! Scope, per CONTEXT.md §4/§5: this engine enforces WHOLE-DOMAIN
//! blocks (websites) at the network layer. Keyword/subreddit
//! (path-level) rules are enforced by the browser extension — this
//! module doesn't know or care about them; `server.rs` only ever calls
//! `apply()` with the full `RuleSet` and this module reads only the
//! `websites` field out of it.
//!
//! Three things happen on every `apply()`:
//!   1. A local `dnsmasq` instance is (re)started, bound to loopback,
//!      configured to blackhole blocked domains AND the Firefox DoH
//!      canary domain (`use-application-dns.net`) — see the module docs
//!      on `render_resolver_config` for why that one domain matters.
//!   2. `nft` redirects all outbound port 53 traffic to that local
//!      resolver, and drops outbound port 853 (DNS-over-TLS), so
//!      nothing can route around step 1.
//!   3. REMOVED (was IP-level blocking): blocked domains used to also
//!      be resolved and mirrored into an nftables set that dropped
//!      matching traffic by IP, as defense-in-depth. Cut because many
//!      domains share IPs with unrelated sites behind the same CDN
//!      (Cloudflare, Fastly, GitHub Pages, etc.), so blocking "the IP
//!      behind reddit.com" could take down completely unrelated sites
//!      sharing that address. DNS-layer blackholing (step 1) is now
//!      the only enforcement mechanism for whole-domain blocks.
//!
//! ## Orphaned-resolver self-healing (added after a real outage)
//!
//! `resolver_child` is only tracked in this struct's memory, inside a
//! single running `mindgated` process. If the daemon dies any way
//! other than a clean, cooperative shutdown — crash, `kill -9`,
//! `Ctrl+C` without a signal handler upstream in `main.rs`, `cargo run`
//! being interrupted mid-test — the spawned `dnsmasq` child does NOT
//! die with it. It gets reparented to init and keeps running,
//! forever, still bound to `127.0.0.1:5353`, still serving whatever
//! rule set was on disk at the moment of death.
//!
//! The next time the daemon starts, it has a brand-new, empty
//! `resolver_child: None` — zero awareness that anything is already
//! squatting on the port. `apply_resolver` used to just try to spawn a
//! fresh dnsmasq on top of that, which either fails to bind and dies
//! near-instantly (silently — `.spawn()` succeeding only proves a
//! process was forked, not that it bound anything), or races the old
//! one in a way that leaves a window where NOTHING is listening on
//! 5353. Either way, `nft` gets told "redirect all DNS here"
//! regardless, and for that window every DNS query on the machine —
//! not just blocked domains — dies. That's a full internet outage
//! that looks intermittent and unrelated to the blocklist, because
//! it's purely a timing/orphan bug, not a logic bug in the rules
//! themselves.
//!
//! Fix has two parts, both below: (1) `ensure_port_clear` runs at the
//! top of every `apply()` and kills any dnsmasq we ourselves left
//! running from a previous crashed session, identified by matching its
//! `--conf-file` argument against our own config path — NOT by killing
//! whatever happens to be on the port. An earlier version of this fix
//! did exactly that (`fuser -k <port>`), and it once killed
//! `avahi-daemon` — a legitimate, unrelated system service sharing the
//! port at the time — which systemd immediately respawned, re-winning
//! the bind race against our own dnsmasq. Matching on the config path
//! we control can never collide with an unrelated service, regardless
//! of what else happens to be running. (2) `apply_resolver` verifies
//! the newly spawned dnsmasq is still alive a moment after spawning,
//! and refuses to let `apply()` proceed to committing nft rules if it
//! isn't, instead of silently logging success. (2) is a safety net for
//! the cases (1) doesn't cover (bad conf syntax, permissions, a port
//! genuinely still in use for some other reason) — it should rarely
//! fire once (1) is in place.
//!
//! Separately: `RESOLVER_PORT` itself was moved off `5353`, which is
//! the IANA-reserved mDNS port that `avahi-daemon` binds by default on
//! most desktop Linux installs — see the constant's own doc comment.
//!
//! The other half of this fix — making `mindgated` itself clean up on
//! a *graceful* stop (SIGINT/SIGTERM: kill the child, flush both nft
//! tables) — belongs in `main.rs`, not here, since it needs a signal
//! handler at the top level. Not yet implemented; `ensure_port_clear`
//! here means an ungraceful death is no longer catastrophic, but a
//! graceful-shutdown handler is still the right long-term fix so
//! stopping `mindgated` doesn't leave firewall rules active with no
//! daemon behind them.

use anyhow::{Context, Result};
use mindgate_common::{RuleSet, WebsiteRule};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const FILTER_TABLE: &str = "mindgate";
const NAT_TABLE: &str = "mindgate_dns";

/// The canary domain Firefox queries on startup and on every network
/// change. If it resolves to NXDOMAIN/SERVFAIL, Firefox silently
/// disables DoH for the session — this is a documented Mozilla
/// mechanism for exactly this use case (parental controls / content
/// filtering on a network), not a hack we're relying on staying
/// undocumented. See CONTEXT.md §4 point 4.
const FIREFOX_DOH_CANARY: &str = "use-application-dns.net";

/// Port our own dnsmasq instance listens on, loopback-only. Deliberately
/// not 53 itself — nftables redirects traffic *to* this port, dnsmasq
/// doesn't need to (and as a non-root-friendly default, shouldn't try
/// to) bind the privileged port directly in dev mode.
///
/// Deliberately NOT 5353: that's the IANA-reserved mDNS port (RFC
/// 6762), and `avahi-daemon` — which ships enabled by default on
/// Ubuntu desktop and most other desktop Linux — permanently binds
/// `0.0.0.0:5353`. This isn't a one-time orphan to clean up; it's a
/// structural conflict with a legitimate, unrelated system service
/// that will never go away. Found this the hard way: killing whatever
/// held 5353 to "fix" it just killed avahi-daemon, which systemd
/// immediately respawned, re-winning the bind race against our own
/// dnsmasq every time. Picking a port outside any standard reserved
/// range avoids the whole class of problem instead of fighting it.
const RESOLVER_PORT: u16 = 55353;

/// The real DNS server our local resolver forwards to when a domain
/// isn't blocked. Defined once here rather than as a separate literal
/// in both `render_resolver_config` and `render_dns_redirect_script` —
/// those two MUST agree, since the redirect rule's job is specifically
/// to let dnsmasq reach *this exact* address without looping the
/// request back to itself (see `render_dns_redirect_script` docs).
const UPSTREAM_RESOLVER: &str = "1.1.1.1";

/// How long we give a freshly-spawned dnsmasq to either bind
/// successfully or die trying, before we trust it enough to commit
/// nft rules against it. This is not "how long dnsmasq takes to
/// start" (that's near-instant) — it's just enough time for a bind
/// failure (port already held by a stale orphan) to surface as a
/// process exit, which `try_wait()` can then observe.
const RESOLVER_LIVENESS_CHECK_DELAY_MS: u64 = 150;

/// Loopback address blocked domains resolve to, instead of NXDOMAIN.
/// Plain HTTP requests to a blocked domain land here and get served
/// REVERTED: this file previously redirected blocked domains to a
/// local loopback HTTP server on port 80 (`address=/domain/127.0.0.1`
/// + `spawn_block_page_server`) to show a themed block page instead of
/// the browser's bare error. That change is what caused
/// DNS_PROBE_FINISHED_NO_INTERNET / "all subdomains blocked": binding
/// port 80 competes with anything else on the box already listening
/// there, and — separately from that — `address=/domain/IP` matches
/// every subdomain of a blocked domain exactly the same way
/// `server=/domain/` does, so the whole subtree got silently routed
/// into a fragile extra process instead of a plain NXDOMAIN. Back to
/// the blackhole-only approach below; no block-page server, no port
/// 80, no loopback redirect. A themed block page is a real feature to
/// revisit later, but not by making DNS resolution depend on a second
/// service binding a privileged port.
pub struct NftEngine {
    nft_binary: PathBuf,
    dnsmasq_binary: PathBuf,
    /// Handle to our spawned dnsmasq child, if running. Held behind a
    /// mutex because `apply()` needs to kill-and-respawn it on every
    /// rule change, and the server dispatches requests concurrently.
    ///
    /// NOTE: this is in-memory only and does NOT survive a daemon
    /// restart. It is not a reliable source of truth for "is a
    /// resolver already running on RESOLVER_PORT" — see
    /// `ensure_port_clear` and the module-level doc comment above for
    /// why we no longer rely on it for that question.
    resolver_child: Mutex<Option<Child>>,
}

impl Default for NftEngine {
    fn default() -> Self {
        Self {
            nft_binary: PathBuf::from("nft"),
            dnsmasq_binary: PathBuf::from("dnsmasq"),
            resolver_child: Mutex::new(None),
        }
    }
}

impl NftEngine {
    /// Whether `nft` is available on this system. When it's not (dev
    /// container, no root, non-Linux), the daemon runs in DRY RUN mode:
    /// rules are stored and served over the socket, but nothing is
    /// enforced. This lets the CLI/daemon/extension plumbing be
    /// developed and tested without root.
    pub async fn nft_available(&self) -> bool {
        Command::new(&self.nft_binary)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub async fn dnsmasq_available(&self) -> bool {
        Command::new(&self.dnsmasq_binary)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Clear an orphaned dnsmasq left behind by a previous, ungracefully-
    /// killed `mindgated` process — and ONLY that. Does not touch
    /// anything else that might be on `RESOLVER_PORT`.
    ///
    /// This used to shell out to `fuser -k <port>/udp`, which kills
    /// whatever process holds the port, no matter what it is. In
    /// practice that meant it once killed `avahi-daemon` (a legitimate,
    /// unrelated system service that also happened to be sharing the
    /// port at the time), which systemd immediately respawned, re-
    /// winning the bind race against our own dnsmasq. A port-based kill
    /// can never distinguish "our own leaked child" from "some other
    /// service that happens to be there" — it has no way to know whose
    /// process it's about to kill. Matching on our own `--conf-file`
    /// argument does: only a dnsmasq WE spawned is invoked with this
    /// exact config path, so this can't collide with anything else on
    /// the system regardless of what port anyone else is using.
    ///
    /// Best-effort and intentionally quiet on failure: if `pgrep`/`kill`
    /// aren't available, or there's nothing to clean up, that's fine —
    /// this is a defensive clear, not a required step for correctness
    /// on a normal clean start.
    async fn ensure_port_clear(&self, config_path: &PathBuf) {
        let pattern = format!("dnsmasq.*--conf-file[= ]{}", config_path.display());

        let output = Command::new("pgrep")
            .args(["-f", &pattern])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await;

        let Ok(output) = output else {
            tracing::warn!(
                "could not run `pgrep` to check for a leaked dnsmasq from a previous \
                 run (is `procps` installed?). Continuing anyway — the post-spawn \
                 liveness check will catch a real conflict."
            );
            return;
        };

        let pids: Vec<&str> = std::str::from_utf8(&output.stdout)
            .unwrap_or("")
            .lines()
            .filter(|l| !l.is_empty())
            .collect();

        if pids.is_empty() {
            return;
        }

        tracing::warn!(
            "found {} leaked dnsmasq process(es) from a previous run (config: {}), \
             killing before starting a fresh one: {:?}",
            pids.len(),
            config_path.display(),
            pids
        );

        // SIGKILL, not SIGTERM: a leaked process may be in a stopped
        // (SIGTSTP / job-control "Stopped") state, e.g. from a `Ctrl+Z`
        // during manual testing rather than a true orphan. SIGTERM
        // sent to a stopped process is queued by the kernel and only
        // delivered once the process is resumed — it does NOT act on
        // it immediately, so a "successful" kill can silently do
        // nothing while the process keeps holding the port. SIGKILL
        // is delivered immediately regardless of stop state.
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-KILL", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }

        // Poll for actual exit instead of a fixed sleep-and-hope: keep
        // checking (via `kill -0`, which sends no signal and just
        // tests whether the PID still exists) until every PID is
        // confirmed gone, or we give up after a short timeout and let
        // the post-spawn liveness check catch it as a last resort.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1000);
        loop {
            let mut any_alive = false;
            for pid in &pids {
                let alive = Command::new("kill")
                    .args(["-0", pid])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .map(|s| s.success())
                    .unwrap_or(false);
                if alive {
                    any_alive = true;
                }
            }
            if !any_alive {
                break;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!(
                    "one or more leaked dnsmasq processes did not exit within 1s \
                     of SIGKILL — proceeding anyway, the post-spawn liveness \
                     check will catch it if the port is still held"
                );
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Pure function: render the nft script for the DoT (port 853)
    /// drop. Kept separate from `apply()` so it's unit-testable
    /// without a real `nft` binary.
    ///
    /// REMOVED: this used to also take a resolved `BTreeSet<IpAddr>`
    /// and firewall-drop each one (`blocked_v4`/`blocked_v6` sets),
    /// as defense-in-depth against stale DNS caches / direct-IP
    /// access. Cut because it was the actual cause of unrelated sites
    /// breaking: many blocked domains resolve to shared CDN IPs
    /// (Cloudflare, Fastly, GitHub Pages, etc.), so dropping "the IP
    /// behind reddit.com" could also drop every other site sitting on
    /// that same shared address. The DNS-layer NXDOMAIN blackhole in
    /// `render_resolver_config` below is the enforcement mechanism now
    /// — no per-IP blocking, no collateral damage to unrelated
    /// domains. `resolve_all` is gone along with it since nothing else
    /// used the resolved IP set.
    pub fn render_filter_script() -> String {
        format!(
            r#"table inet {table} {{
    chain output {{
        type filter hook output priority 0; policy accept;
        tcp dport 853 counter drop
    }}
}}
"#,
            table = FILTER_TABLE,
        )
    }

    /// Pure function: render the nat-table script that redirects all
    /// outbound DNS (port 53, tcp+udp) to our local resolver. This is
    /// the anti-bypass foundation — it applies regardless of what the
    /// app or OS resolver config claims to be using.
    ///
    /// Critical exception, found the hard way: this hook fires on
    /// *all* outbound port-53 traffic system-wide, with no way to tell
    /// "some random app's query" apart from "our own local resolver
    /// asking the real upstream for an answer" — dnsmasq's own outbound
    /// query to `upstream` is itself outbound traffic to port 53, so
    /// without this exception it gets redirected right back to itself,
    /// forming a loop that never reaches the real internet. That
    /// doesn't just break blocked domains — it breaks DNS resolution
    /// entirely, system-wide, since dnsmasq can never get a real answer
    /// from anywhere. `upstream` must be excluded from the redirect so
    /// the local resolver can actually do its job.
    pub fn render_dns_redirect_script(upstream: &str) -> String {
        format!(
            r#"table ip {table} {{
    chain output {{
        type nat hook output priority -100; policy accept;
        ip daddr {upstream} accept
        meta l4proto {{ tcp, udp }} th dport 53 redirect to :{port}
    }}
}}
"#,
            table = NAT_TABLE,
            upstream = upstream,
            port = RESOLVER_PORT,
        )
    }

    /// Pure function: render the dnsmasq config for our local resolver.
    ///
    /// Two kinds of entries, both using the "empty server=" blackhole
    /// pattern (a documented dnsmasq technique: `server=/domain/` with
    /// no target tells dnsmasq not to forward queries for that domain
    /// anywhere, so it answers NXDOMAIN instead of leaking the query
    /// upstream or returning a real address):
    ///   - every blocked website domain
    ///   - the Firefox DoH canary domain, unconditionally, even if the
    ///     user hasn't blocked anything yet — killing Firefox's DoH
    ///     auto-upgrade is a baseline protection, not a per-rule one.
    ///
    /// Chrome needs no equivalent entry here: it only upgrades to DoH
    /// if the *current* resolver is on its supported provider list, and
    /// once nftables redirects everything to us, it isn't.
    pub fn render_resolver_config(websites: &[WebsiteRule], upstream: &str) -> String {
        let mut out = String::new();
        out.push_str("# Generated by mindgated — do not edit by hand.\n");
        out.push_str("no-resolv\n");
        out.push_str("no-hosts\n");
        out.push_str("bind-interfaces\n");
        out.push_str("listen-address=127.0.0.1\n");
        out.push_str(&format!("port={}\n", RESOLVER_PORT));
        out.push_str(&format!("server={}\n", upstream));
        out.push_str(&format!("server=/{}/\n", FIREFOX_DOH_CANARY));
        for rule in websites {
            out.push_str(&format!("server=/{}/\n", rule.domain));
        }
        out
    }

    /// Write the resolver config and (re)spawn dnsmasq bound to it.
    /// Idempotent: any previously-running instance is killed first, so
    /// repeated calls converge on the current rule set rather than
    /// leaving stale processes behind.
    ///
    /// Ordering, changed from the original version: we now spawn the
    /// NEW child and confirm it's alive BEFORE killing the old one we
    /// have a handle to. Killing old-then-spawning-new left a window
    /// where, if the new spawn failed, we'd have zero resolvers
    /// running at all. Spawning-then-verifying-then-killing-old means
    /// there's always at least one resolver up during the transition.
    async fn apply_resolver(&self, websites: &[WebsiteRule], config_path: &PathBuf) -> Result<()> {
        if !self.dnsmasq_available().await {
            tracing::warn!(
                "dnsmasq not found — DRY RUN: local resolver not started, \
                 DNS-layer blocking is not being enforced."
            );
            return Ok(());
        }

        // Clear any orphan from a previous ungraceful shutdown before
        // we do anything else. See module-level doc comment and
        // `ensure_port_clear` doc comment for why this can't be
        // handled by killing `self.resolver_child` alone, and why it
        // now matches on our own config path rather than "whatever is
        // on the port."
        self.ensure_port_clear(config_path).await;

        let config = Self::render_resolver_config(websites, UPSTREAM_RESOLVER);
        if let Some(parent) = config_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(config_path, &config)
            .await
            .with_context(|| format!("failed to write {}", config_path.display()))?;

        let mut new_child = Command::new(&self.dnsmasq_binary)
            .arg("--keep-in-foreground")
            // MUST be one combined `--conf-file=path` token, not two
            // separate argv entries (`--conf-file`, then the path).
            // This dnsmasq build's own argument parser rejects the
            // split form with "junk found in command line" — found by
            // running the exact same invocation by hand. This was
            // very likely broken from day one; it went unnoticed
            // because a manually-started dnsmasq (using the correct
            // `=` form) happened to already be sitting on the port
            // during earlier testing, silently covering for every
            // failed spawn attempt the daemon itself made.
            .arg(format!("--conf-file={}", config_path.display()))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn dnsmasq")?;

        // Give it a moment to either bind or die. `.spawn()` succeeding
        // only proves a process was forked, not that it bound the
        // port — a stale orphan already holding RESOLVER_PORT (or a
        // bad conf, or a permissions issue) will make dnsmasq exit
        // almost immediately. Without this check we'd log success
        // and let `apply()` go on to commit nft rules that redirect
        // all system DNS into a process that's already dead.
        tokio::time::sleep(std::time::Duration::from_millis(
            RESOLVER_LIVENESS_CHECK_DELAY_MS,
        ))
        .await;

        match new_child.try_wait() {
            Ok(Some(status)) => {
                use tokio::io::AsyncReadExt;
                let mut stderr_output = String::new();
                if let Some(mut stderr) = new_child.stderr.take() {
                    let _ = stderr.read_to_string(&mut stderr_output).await;
                }
                anyhow::bail!(
                    "dnsmasq exited immediately after spawn (status: {status}). \
                     dnsmasq stderr: {}",
                    if stderr_output.trim().is_empty() {
                        "<empty — dnsmasq printed nothing to stderr>".to_string()
                    } else {
                        stderr_output.trim().to_string()
                    }
                );
            }
            Ok(None) => {
                // Still running — good, trust it.
            }
            Err(e) => {
                tracing::warn!(
                    "could not confirm dnsmasq liveness ({e}) — proceeding, but \
                     this check existing to catch exactly the failure mode it \
                     just failed to check for."
                );
            }
        }

        // Only now, with the new instance confirmed alive, replace and
        // kill whatever we previously held a handle to.
        let mut guard = self.resolver_child.lock().await;
        if let Some(mut old) = guard.take() {
            let _ = old.kill().await;
        }
        *guard = Some(new_child);

        tracing::info!("local resolver (dnsmasq) running on 127.0.0.1:{}", RESOLVER_PORT);
        Ok(())
    }

    /// Apply an nft script by flushing the named table first (ignoring
    /// failure — it may not exist yet on first run) then loading the
    /// new script via `nft -f -`. This is what makes repeated `apply()`
    /// calls idempotent instead of additive.
    async fn apply_nft_script(&self, table_family: &str, table_name: &str, script: &str) -> Result<()> {
        let _ = Command::new(&self.nft_binary)
            .args(["delete", "table", table_family, table_name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        let mut child = Command::new(&self.nft_binary)
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn nft")?;

        child
            .stdin
            .take()
            .context("no stdin on nft child")?
            .write_all(script.as_bytes())
            .await
            .context("failed to write ruleset to nft stdin")?;

        let output = child.wait_with_output().await.context("nft did not exit cleanly")?;
        if !output.status.success() {
            anyhow::bail!(
                "nft rejected the {} ruleset: {}",
                table_name,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    /// Apply the full rule set: local resolver (NXDOMAIN blackhole for
    /// blocked domains + the Firefox DoH canary) and the DNS redirect +
    /// DoT block. This is the single entry point `server.rs` calls
    /// after every mutation.
    ///
    /// No IP-level blocking and no block-page server here anymore —
    /// see the doc comments on `render_filter_script` and the
    /// module-level note near `NftEngine`'s definition for why both
    /// were cut. DNS blackhole is the enforcement mechanism; nothing
    /// else touches individual IPs, so there's no shared-CDN-IP
    /// collateral damage to unrelated sites.
    ///
    /// If `apply_resolver` bails (e.g. the liveness check failed),
    /// `apply()` returns that error immediately and does NOT go on to
    /// apply the nft DNS redirect — see the `?` below. That way, even
    /// if the resolver is unhealthy, we never commit the redirect
    /// against it (which would otherwise take down all DNS, not just
    /// blocked domains).
    pub async fn apply(&self, rules: &RuleSet, resolver_config_path: &PathBuf) -> Result<()> {
        self.apply_resolver(&rules.websites, resolver_config_path).await?;

        if !self.nft_available().await {
            tracing::warn!(
                "nft binary not found or not usable — DRY RUN: no firewall rules are \
                 actually being enforced. Rules are still tracked and served to the \
                 CLI/extension."
            );
            return Ok(());
        }

        let redirect_script = Self::render_dns_redirect_script(UPSTREAM_RESOLVER);
        self.apply_nft_script("ip", NAT_TABLE, &redirect_script).await?;

        let filter_script = Self::render_filter_script();
        self.apply_nft_script("inet", FILTER_TABLE, &filter_script).await?;

        tracing::info!(
            "applied {} website rule(s) via DNS blackhole, DNS redirect + DoT block active",
            rules.websites.len(),
        );
        Ok(())
    }

    /// Tear down everything this engine may have set up: kill the
    /// resolver child if we're holding a handle to it, and flush both
    /// nft tables. Intended to be called from a graceful-shutdown
    /// signal handler in `main.rs` (SIGINT/SIGTERM) so stopping
    /// `mindgated` cleanly doesn't leave firewall rules active with no
    /// daemon behind them.
    ///
    /// This does NOT solve the ungraceful-death case (crash, SIGKILL)
    /// — nothing running in-process can, by definition. That case is
    /// covered by `ensure_port_clear` (for the resolver) and should
    /// additionally be covered by `KillMode=control-group` in the
    /// systemd unit (for both the resolver and, longer-term, for
    /// leftover nft state — though nft rules are kernel-resident and
    /// systemd killing the daemon won't remove them; a graceful stop
    /// via this method is the only thing that removes them, which is
    /// exactly why the signal handler in `main.rs` matters).
    pub async fn teardown(&self) {
        let mut guard = self.resolver_child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
        }
        drop(guard);

        for (family, table) in [("ip", NAT_TABLE), ("inet", FILTER_TABLE)] {
            let _ = Command::new(&self.nft_binary)
                .args(["delete", "table", family, table])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_script_drops_dot_and_has_no_ip_sets() {
        // Regression test for the CDN-collision bug: this script must
        // NOT contain any per-IP blocking anymore. Enforcement is
        // DNS-layer only (see render_resolver_config); this table's
        // only remaining job is blocking DNS-over-TLS so nothing can
        // route around our DNS redirect via an encrypted resolver port.
        let script = NftEngine::render_filter_script();
        assert!(script.contains("table inet mindgate"));
        assert!(script.contains("tcp dport 853 counter drop"));
        assert!(!script.contains("blocked_v4"));
        assert!(!script.contains("blocked_v6"));
        assert!(!script.contains("ip daddr"));
    }

    #[test]
    fn dns_redirect_script_targets_resolver_port() {
        let script = NftEngine::render_dns_redirect_script(UPSTREAM_RESOLVER);
        assert!(script.contains("table ip mindgate_dns"));
        assert!(script.contains("th dport 53 redirect to :5353"));
        assert!(script.contains("hook output priority -100"));
    }

    #[test]
    fn dns_redirect_script_exempts_upstream_to_avoid_self_redirect_loop() {
        // Regression test for a real bug found by hand: without this
        // exception, dnsmasq's own outbound query to the upstream
        // resolver gets caught by the same rule and redirected back to
        // itself, breaking ALL DNS resolution system-wide (not just
        // blocked domains) rather than just enforcing blocks.
        let script = NftEngine::render_dns_redirect_script(UPSTREAM_RESOLVER);
        assert!(script.contains(&format!("ip daddr {} accept", UPSTREAM_RESOLVER)));
        // The accept exception must come before the redirect rule —
        // nft evaluates rules in order within a chain.
        let accept_pos = script.find("accept").unwrap();
        let redirect_pos = script.find("redirect to").unwrap();
        assert!(accept_pos < redirect_pos);
    }

    #[test]
    fn resolver_config_blackholes_doh_canary_even_with_no_rules() {
        // The DoH canary is still a blackhole (NXDOMAIN) — there's no
        // reason to serve it a page, it's not something the user
        // would ever open in a browser tab themselves.
        let config = NftEngine::render_resolver_config(&[], "1.1.1.1");
        assert!(config.contains("server=/use-application-dns.net/"));
        assert!(config.contains("server=1.1.1.1"));
        assert!(config.contains("port=5353"));
    }

    #[test]
    fn resolver_config_blackholes_each_blocked_domain() {
        // Regression test for the loopback-redirect regression: blocked
        // website domains must be a plain `server=/domain/` blackhole
        // (NXDOMAIN), not an `address=/domain/IP` redirect to a local
        // block-page server. The redirect approach is what broke DNS
        // system-wide and took whole subdomain trees down with it.
        let websites = vec![
            WebsiteRule { domain: "reddit.com".into() },
            WebsiteRule { domain: "twitter.com".into() },
        ];
        let config = NftEngine::render_resolver_config(&websites, "1.1.1.1");
        assert!(config.contains("server=/reddit.com/"));
        assert!(config.contains("server=/twitter.com/"));
        assert!(!config.contains("address=/"));
        assert!(config.contains("server=/use-application-dns.net/"));
    }
}