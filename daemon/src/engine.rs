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
//!   3. Blocked domains are also resolved to IPs and mirrored into an
//!      nftables set that drops matching traffic outright — this is
//!      what survives CDN IP rotation and catches anything that still
//!      has a stale/cached resolution.

use anyhow::{Context, Result};
use mindgate_common::{RuleSet, WebsiteRule};
use std::collections::BTreeSet;
use std::net::{IpAddr, ToSocketAddrs};
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
const RESOLVER_PORT: u16 = 5353;

pub struct NftEngine {
    nft_binary: PathBuf,
    dnsmasq_binary: PathBuf,
    /// Handle to our spawned dnsmasq child, if running. Held behind a
    /// mutex because `apply()` needs to kill-and-respawn it on every
    /// rule change, and the server dispatches requests concurrently.
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

    /// Resolve every website domain to its current IP set, so we can
    /// block by IP as well as by name (defense-in-depth against stale
    /// caches, direct-IP access, or a client bypassing our resolver
    /// redirect some other way).
    ///
    /// Known, accepted limitation (documented, not hidden — see
    /// CONTEXT.md §4): this goes through whatever resolver the daemon's
    /// own libc is configured with, which may itself end up being our
    /// redirected loopback resolver once the nft rules are active.
    /// That's fine for MVP1; it just means re-resolution happens
    /// through the same blackhole-aware resolver we configured.
    pub async fn resolve_all(domains: &[WebsiteRule]) -> BTreeSet<IpAddr> {
        let mut ips = BTreeSet::new();
        for rule in domains {
            let host = rule.domain.clone();
            let resolved = tokio::task::spawn_blocking(move || {
                (host.as_str(), 443u16)
                    .to_socket_addrs()
                    .map(|it| it.map(|sa| sa.ip()).collect::<Vec<_>>())
                    .unwrap_or_default()
            })
            .await
            .unwrap_or_default();
            ips.extend(resolved);
        }
        ips
    }

    /// Pure function: given a resolved IP set, render the nft script
    /// for whole-domain IP blocking + the DoT (port 853) drop. Kept
    /// separate from `apply()` so it's unit-testable without a real
    /// `nft` binary.
    pub fn render_filter_script(ips: &BTreeSet<IpAddr>) -> String {
        let v4: Vec<String> =
            ips.iter().filter(|ip| ip.is_ipv4()).map(|ip| ip.to_string()).collect();
        let v6: Vec<String> =
            ips.iter().filter(|ip| ip.is_ipv6()).map(|ip| ip.to_string()).collect();

        format!(
            r#"table inet {table} {{
    set blocked_v4 {{
        type ipv4_addr;
        flags interval;
        elements = {{ {v4_set} }}
    }}

    set blocked_v6 {{
        type ipv6_addr;
        flags interval;
        elements = {{ {v6_set} }}
    }}

    chain output {{
        type filter hook output priority 0; policy accept;
        ip daddr @blocked_v4 counter drop
        ip6 daddr @blocked_v6 counter drop
        tcp dport 853 counter drop
    }}
}}
"#,
            table = FILTER_TABLE,
            v4_set = v4.join(", "),
            v6_set = v6.join(", "),
        )
    }

    /// Pure function: render the nat-table script that redirects all
    /// outbound DNS (port 53, tcp+udp) to our local resolver. This is
    /// the anti-bypass foundation — it applies regardless of what the
    /// app or OS resolver config claims to be using.
    pub fn render_dns_redirect_script() -> String {
        format!(
            r#"table ip {table} {{
    chain output {{
        type nat hook output priority -100; policy accept;
        meta l4proto {{ tcp, udp }} th dport 53 redirect to :{port}
    }}
}}
"#,
            table = NAT_TABLE,
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
    async fn apply_resolver(&self, websites: &[WebsiteRule], config_path: &PathBuf) -> Result<()> {
        if !self.dnsmasq_available().await {
            tracing::warn!(
                "dnsmasq not found — DRY RUN: local resolver not started, \
                 DNS-layer blocking is not being enforced."
            );
            return Ok(());
        }

        let config = Self::render_resolver_config(websites, "1.1.1.1");
        if let Some(parent) = config_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(config_path, &config)
            .await
            .with_context(|| format!("failed to write {}", config_path.display()))?;

        let mut guard = self.resolver_child.lock().await;
        if let Some(mut child) = guard.take() {
            let _ = child.kill().await;
        }

        let child = Command::new(&self.dnsmasq_binary)
            .args(["--keep-in-foreground", "--conf-file"])
            .arg(config_path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to spawn dnsmasq")?;
        *guard = Some(child);

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

    /// Apply the full rule set: local resolver, DNS redirect + DoT
    /// block, and IP-level website blocking. This is the single
    /// entry point `server.rs` calls after every mutation.
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

        let redirect_script = Self::render_dns_redirect_script();
        self.apply_nft_script("ip", NAT_TABLE, &redirect_script).await?;

        let ips = Self::resolve_all(&rules.websites).await;
        let filter_script = Self::render_filter_script(&ips);
        self.apply_nft_script("inet", FILTER_TABLE, &filter_script).await?;

        tracing::info!(
            "applied {} website rule(s), {} IP(s) blocked, DNS redirect + DoT block active",
            rules.websites.len(),
            ips.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_script_includes_both_families_and_dot_block() {
        let mut ips = BTreeSet::new();
        ips.insert("93.184.216.34".parse::<IpAddr>().unwrap());
        ips.insert("2606:2800:220:1:248:1893:25c8:1946".parse::<IpAddr>().unwrap());
        let script = NftEngine::render_filter_script(&ips);
        assert!(script.contains("93.184.216.34"));
        assert!(script.contains("2606:2800:220:1:248:1893:25c8:1946"));
        assert!(script.contains("table inet mindgate"));
        assert!(script.contains("ip daddr @blocked_v4 counter drop"));
        assert!(script.contains("tcp dport 853 counter drop"));
    }

    #[test]
    fn filter_script_handles_empty_set() {
        let script = NftEngine::render_filter_script(&BTreeSet::new());
        assert!(script.contains("elements = {  }") || script.contains("elements = { }"));
    }

    #[test]
    fn dns_redirect_script_targets_resolver_port() {
        let script = NftEngine::render_dns_redirect_script();
        assert!(script.contains("table ip mindgate_dns"));
        assert!(script.contains("th dport 53 redirect to :5353"));
        assert!(script.contains("hook output priority -100"));
    }

    #[test]
    fn resolver_config_blackholes_doh_canary_even_with_no_rules() {
        let config = NftEngine::render_resolver_config(&[], "1.1.1.1");
        assert!(config.contains("server=/use-application-dns.net/"));
        assert!(config.contains("server=1.1.1.1"));
        assert!(config.contains("port=5353"));
    }

    #[test]
    fn resolver_config_blackholes_each_blocked_domain() {
        let websites = vec![
            WebsiteRule { domain: "reddit.com".into() },
            WebsiteRule { domain: "twitter.com".into() },
        ];
        let config = NftEngine::render_resolver_config(&websites, "1.1.1.1");
        assert!(config.contains("server=/reddit.com/"));
        assert!(config.contains("server=/twitter.com/"));
        assert!(config.contains("server=/use-application-dns.net/"));
    }
}