# MindGate — Project Context

**Status: LOCKED.** This document is the single source of truth for architecture, file
structure, and MVP scope. If any AI assistant (ChatGPT, Gemini, or otherwise) suggests
something that conflicts with this document, this document wins unless the user
explicitly approves the change in writing. Do not redesign the architecture, rename
modules, or add crates without approval.

---

## 1. What MindGate is

A modern, open-source Linux focus tool inspired by Cold Turkey. It helps users
intentionally block distractions and makes bypassing those blocks *effortful* enough
that they stay focused.

It is **not** an antivirus, enterprise security suite, parental-control product, or
firewall manager. It never claims to be unbypassable — friction is the product, not
a guarantee.

Priorities, in order: reliability, simplicity, clean architecture, modern Linux
practices, good UX. No Python, no Electron, no GUI, no bash beyond the installer.

---

## 2. Architecture

```
                         mindgate (CLI)
                               │
                       Unix Domain Socket
                               │
                        mindgated (daemon)
                               │
              ┌────────────────┼────────────────┐
              │                │                │
         Rule Engine    Native Messaging   Lock Manager
              │            Bridge Mode          │
         nftables              │             Password
              │           Browser Extension   (Argon2)
      Local DNS Resolver
      (dnsmasq/unbound,
       forced via nft)
```

Notes on this diagram, since it's easy to misread:

- **Rule Engine**, **Lock Manager**, and the **native-messaging bridge** are all
  modules *inside* `mindgated` — not separate processes. `mindgated` is the only
  long-running privileged process.
- There is **no separate bridge binary**. Browsers spawn native-messaging hosts
  themselves per their own protocol, so the thing they spawn is the existing
  `mindgate` CLI binary run in a hidden mode (e.g. `mindgate __native-bridge`),
  registered as the `path` in `extension/native-messaging-host.json`. That process
  is short-lived: it relays stdio (browser) ↔ Unix socket (daemon) and exits when the
  browser closes the port. This keeps the workspace at 3 crates instead of 4.
- The CLI never touches nftables, DNS, or the password file directly. Every mutation
  goes: `mindgate` → socket → `mindgated` → engine/store/lock. This is the single most
  important invariant in the whole project.

---

## 3. Why the two enforcement layers exist (do not collapse this)

MindGate's three MVP1 features split into two genuinely different engineering
problems, and no amount of clever engineering merges them into one:

| Feature | Where it's enforceable | Why |
|---|---|---|
| Block whole website (`reddit.com`) | Network layer (Rust/nftables) | DNS + firewall see the domain regardless of TLS |
| Block a subreddit (`reddit.com/r/x`) | Browser layer (extension) | The path is inside the encrypted payload — invisible to network tools |
| Block a keyword in a URL | Browser layer (extension) | Same reason |

A firewall cannot see `/r/gonewild` in an HTTPS request. Only the browser sees it,
before encryption. This is why the browser extension is a **core, non-optional**
component, not a nice-to-have.

---

## 4. Layer 1 — Network-level website blocking (`daemon/src/engine.rs`)

1. **nftables redirects all outbound port 53** to a local resolver (dnsmasq/unbound)
   regardless of what the app or OS thinks it's using. This is the anti-bypass
   foundation.
2. **Block DNS-over-TLS (port 853)** outbound, so nothing can sidestep the redirect
   with a standard encrypted resolver port.
3. **Chrome/Chromium DoH handling:** Chrome only upgrades to DoH if the *current*
   resolver is on its supported list (Cloudflare, Google, etc.). Once step 1 forces
   everything through our own local resolver, Chrome has nothing to upgrade to — no
   DoH IP blocklist is required for this browser family.
4. **Firefox DoH handling:** Firefox tests a canary domain, `use-application-dns.net`,
   through the OS resolver on startup and on every network change. If that lookup
   returns NXDOMAIN/SERVFAIL, Firefox silently disables DoH for the session — this is
   a documented, intentional Mozilla mechanism for exactly this use case. Our local
   resolver returns NXDOMAIN for this one domain. No IP blocklist needed here either.
5. **Blocked domains:** resolved by our own resolver, mirrored into nftables sets
   (`blocked_v4`/`blocked_v6`), and **re-resolved periodically** so CDN IP rotation
   doesn't silently defeat the block. This is already correctly shaped in the current
   `engine.rs` — keep it.
6. **Dry-run mode:** if `nft` isn't available (dev container, no root, non-Linux),
   the daemon logs a warning and still tracks/serves rules over the socket without
   enforcing them. This is what lets us develop the CLI/daemon/extension plumbing
   without root.

Known, accepted limitation: domain resolution for building nftables sets goes through
whatever resolver `mindgated`'s own libc is configured with. Documented, not hidden.

---

## 5. Layer 2 — Browser-level path/keyword blocking (`extension/`)

- **Manifest V3**, both Chrome and Firefox. MV3 is mandatory on Chrome; Firefox's MV3
  implementation retains more power but we target the lowest common denominator so
  one codebase works on both.
- Use **`declarativeNetRequest` dynamic rules**, not the deprecated blocking
  `webRequest` API. `urlFilter` matches against the full URL including path and
  query string, so `||reddit.com/r/gonewild` is a valid, supported pattern — this is
  sufficient for subreddit and keyword blocking. No need for per-request JS
  inspection.
- The extension holds no independent state. On connect (and periodically), it syncs
  its dynamic rule set from `mindgated` via native messaging, using the same
  length-prefixed JSON wire format already defined in `common/src/lib.rs` (native
  messaging's own protocol is length-prefixed JSON too, so this reuses, not
  reinvents, the framing).
- **Fail-closed rule:** if the daemon detects the extension has gone silent
  (heartbeat via `Request::ExtensionHeartbeat` stops arriving) during an active
  session, and path-level rules exist (`RuleSet::has_path_level_rules()`), the
  engine should fall back to blocking the *whole domain* at the network layer rather
  than silently losing subreddit/keyword granularity. (Not required for MVP1 — see
  §7 — but the hook point is `has_path_level_rules()`, already written.)
- Block page (`extension/block-page/`) is soft pink themed, consistent with the rest
  of the product's visual identity.

---

## 6. Daemon responsibilities (`daemon/`)

- Own all state: rules, password hash, lock state.
- Authenticate privileged operations (password checks via Argon2 — never
  hand-rolled).
- Apply blocking rules through the engine only.
- Synchronize the browser extension's rule set.
- Manage Lock Mode (timed or password-gated commitment device).
- Restore state after reboot (systemd unit, `RemainAfterExit`/re-apply on start).
- Expose the Unix socket API, with `SO_PEERCRED` checks so arbitrary local users
  can't issue commands as another user.

Lock Mode is honestly scoped: on desktop Linux the user typically has root, and root
can always `systemctl kill`, delete the binary, or boot a live USB. MindGate does not
and will not claim otherwise. Lock Mode raises the cost of quitting; it is not a
sandbox.

---

## 7. MVP1 — exact scope

**MVP1 ships when:**

1. `mindgate add <domain>` blocks that domain network-wide (all browsers, curl,
   any app) via the nftables + local-resolver pipeline in §4.
2. `mindgate add-subreddit <name>` / `mindgate add-keyword <value>` blocks that
   path/keyword in both Chrome and Firefox via the extension in §5, synced from the
   daemon over native messaging.
3. `mindgate list` / `mindgate status` accurately reflect daemon state, including
   whether the extension is currently connected.
4. Everything above survives a reboot (systemd-managed `mindgated`, rules reloaded
   from `/etc/mindgate/rules.toml` on start).

**Explicitly NOT in MVP1** (do not build ahead of this without approval):

- Lock Mode enforcement (module exists as a stub — `daemon/src/lock.rs` — but no
  CLI-exposed timer/password lock logic yet)
- The fail-closed fallback described in §5
- `installer/install.sh` running against a real released binary (script exists and
  is checksum-verified, but there's no tagged release yet to point it at)
- Per-process/eBPF enforcement (see §8 — future roadmap only)
- Any GUI

**Build order to reach MVP1**, in the locked file structure:

1. `common/src/lib.rs` — done
2. `daemon/src/engine.rs` — extend with local-resolver management, port 53/853
   redirect rules, the `use-application-dns.net` NXDOMAIN record, periodic
   re-resolve loop
3. `daemon/src/store.rs` — done
4. `daemon/src/server.rs` — Unix socket listener, `SO_PEERCRED`, dispatches
   `Request` → engine/store
5. `daemon/src/lock.rs` — stub only for MVP1
6. `daemon/src/main.rs` — wiring, `tracing-subscriber` init, systemd-friendly
   startup (reload rules, re-apply engine state)
7. `cli/src/main.rs` — `clap` subcommands (git-style), plus the hidden
   `__native-bridge` mode used by the extension
8. `extension/` — `manifest.json` (MV3), `background.js`
   (`declarativeNetRequest` dynamic rules + native messaging sync),
   `native-messaging-host.json`, pink-themed `block-page/`
9. `installer/install.sh` — already written; wire up to real GitHub release once
   one exists

---

## 8. Post-MVP roadmap (not now, do not build early)

- **eBPF per-process enforcement**, for "block the Reddit app, not just the
  browser" or detecting `curl`-based bypass attempts. `aya-rs` is the intended
  toolkit (pure Rust, no C toolchain dependency) if/when this is built. Requires a
  reasonably modern kernel. Prior art to study: OpenSnitch (interactive outbound
  filtering, nftables integration).
- Real Lock Mode timer/password enforcement.
- Fail-closed extension-down fallback.
- Cross-distro fallback path for systems without `systemd-resolved` (some Arch
  minimal installs, Void, Alpine): the design already avoids hard-depending on
  `systemd-resolved` specifically by owning the local resolver ourselves, so this is
  mostly a testing/packaging task, not an architecture change.

---

## 9. File structure (locked)

```
mindgate/
├── Cargo.toml
├── common/
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
├── daemon/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── engine.rs
│       ├── store.rs
│       ├── lock.rs
│       └── server.rs
├── cli/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── extension/
│   ├── manifest.json
│   ├── background.js
│   ├── native-messaging-host.json
│   ├── block-page/
│   │   ├── block.html
│   │   ├── block.css
│   │   └── block.js
│   └── icons/
├── installer/
│   └── install.sh
└── README.md
```

No new crates, no new top-level directories, without explicit user approval in this
conversation.

---

## 10. Design principles (unchanged)

- One responsibility per module.
- Small, testable modules over large files.
- Understandable months later, by someone who isn't the original author.
- No cleverness for its own sake.
- Feels like a professional Linux system utility — CLI ergonomics similar to Git,
  Cargo, or Docker.
- A dependable blocker with fewer features beats an ambitious one users can't trust.