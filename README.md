<div align="center">

# 🩷 MindGate

**Make distractions harder than doing the work.**

*A modern, open-source Linux focus tool — inspired by Cold Turkey.*

⚠️ **work in progress — not usable yet.** This is a dev build, not a release.
See [Status](#-status) for what's actually done.

✦ ✦ ✦

</div>

---

## ✧ What is MindGate?

MindGate is a Linux-native focus tool that helps you block the things you've decided
distract you — and makes going back on that decision *effortful* enough that you
usually don't.

**Right now it's a work in progress**, not a working product. There's no release,
no installer that points at anything real, and no Lock Mode enforcement yet — see
[Status](#-status) for the honest breakdown. Everything below the testing section
is for people building/testing the daemon and CLI from source, not for daily use.

It is **not** a parental-control tool, surveillance software, or productivity tracker.
It doesn't monitor you, log your browsing history, or judge how you spend your time.
It simply puts real friction between you and the distractions you chose to block.

MindGate is **open source**, currently in early development. There are no accounts,
no cloud sync, no telemetry — your rules live only on your machine.

> It's not trying to force productivity. It's designed to help you keep the promises
> you make to yourself, before distraction gets a vote.

---

## 🤍 How it works

MindGate blocks distractions in two layers, because a firewall and a browser see
very different things:

| You block | Enforced by | Why |
|---|---|---|
| A whole website (`reddit.com`) | **Daemon** — nftables + local DNS | DNS and the firewall see the domain regardless of TLS |
| A subreddit (`reddit.com/r/x`) | **Browser Extension** | The path is inside the encrypted request — invisible to the network layer |
| A keyword in a URL | **Browser Extension** | Same reason — only the browser sees it before encryption |

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
              │           Browser Extension    (Argon2)
      Local DNS Resolver
```

`mindgated` is the only privileged, long-running process. The CLI never touches
nftables, DNS, or the password store directly — every change flows through the
socket, into the daemon, and out through the engine.

---

## ✦ Principles

- **User first** — you decide what to block. Nothing is blocked by default.
- **Friction over convenience** — once committed, a rule should take real effort to undo.
- **Commit before you need discipline** — configure your rules first, lock them, then face temptation.
- **Defense in depth** — browser filtering, DNS enforcement, and network-level blocking, layered.
- **Privacy by design** — no accounts, no cloud, no telemetry, no history collection.
- **Open source** — trust through transparency, not promises.
- **Linux native** — built on systemd, Unix sockets, nftables, and Native Messaging.

MindGate never claims to be unbypassable. Friction is the product, not a guarantee.

---

## ◆ Dev Testing Guide

**Terminal 1 — the daemon** *(start once, leave running)*

```bash
cd ~/Desktop/MindGate
source mindgate-env.sh
sudo -E target/debug/mindgated
```

- `sudo` is required — nftables changes need root.
- `-E` is required — without it, sudo wipes the env vars you just exported, and
  you're back to the default-paths problem.

**Terminal 2 — CLI commands** *(separate window/tab)*

```bash
cd ~/Desktop/MindGate
source mindgate-env.sh
```

Then any of:

```bash
sudo -E target/debug/mindgate add example.com          # block a website (network-wide)
sudo -E target/debug/mindgate remove example.com       # unblock it
sudo -E target/debug/mindgate add-keyword pizza        # block a keyword (browser layer)
sudo -E target/debug/mindgate remove-keyword pizza
sudo -E target/debug/mindgate add-subreddit gonewild   # block a subreddit (browser layer)
sudo -E target/debug/mindgate remove-subreddit gonewild
sudo -E target/debug/mindgate list                     # show current rules
sudo -E target/debug/mindgate status                   # daemon health + extension connection
```

**Reset DNS** *(if internet stops working)*

```bash
sudo nft flush ruleset && sudo systemctl restart NetworkManager
```

**Reset Settings** 

sudo rm -f "$MINDGATE_CONFIG_DIR/rules.toml" "$MINDGATE_CONFIG_DIR/lock.toml" "$MINDGATE_CONFIG_DIR/password.hash"

---

## ✧ Testing on your own machine

Everything above depends on one file: **`mindgate-env.sh`**. It points the daemon
and CLI at dev-friendly paths so you don't need to write into `/etc` or `/run` as
root just to test locally.

```bash
#!/bin/bash
# Source this in any terminal before running mindgate/mindgated commands:
#   source mindgate-env.sh
export MINDGATE_SOCKET=/tmp/mindgate-dev/mindgate.sock
export MINDGATE_CONFIG_DIR=/tmp/mindgate-dev
export MINDGATE_RUN_DIR=/tmp/mindgate-dev
```

To test on your own PC:

1. **Clone the repo** and build it: `cargo build`
2. **Copy `mindgate-env.sh`** into the project root if it isn't already there.
3. **Change nothing in the file itself** — the paths in `/tmp/mindgate-dev` work
   for any user, on any machine. You only need to edit paths in **your own scripts**
   that reference an absolute install location, e.g. `extension/mindgate.sh` and
   `com.mindgate.protector.json`, which currently point to
   `/home/faisal/Desktop/MindGate/...` — swap that prefix for wherever *you*
   cloned the repo.
4. **`source mindgate-env.sh`** in every terminal before running `mindgate` or
   `mindgated` — this is what keeps both processes talking over the same socket.
5. Follow the Dev Testing Guide above, exactly as written.

If you skip `source mindgate-env.sh`, the daemon falls back to `/run/mindgate` and
`/etc/mindgate`, which need real root and a real install — fine for production,
painful for iterating locally.

---

## 🩷 Status

**Not done. Not released. Don't expect it to work end-to-end.**

What actually exists right now:

- ✅ Daemon (`mindgated`) + CLI (`mindgate`) talking over a Unix socket
- ✅ Add/remove website, keyword, and subreddit rules, persisted to disk
- ✅ Browser extension skeleton syncing rules via native messaging (rough around the edges)

What's explicitly **not** built yet:

- ❌ Lock Mode enforcement — the module is a stub, no timer/password lock actually stops you
- ❌ The extension-down fail-closed fallback (falling back to whole-domain block if the extension disconnects)
- ❌ A real installer — `install.sh` exists but has nothing tagged to install yet
- ❌ Any GUI
- ❌ Testing beyond one dev machine — paths in `mindgate.sh` / the native-messaging host JSON are still hardcoded to one person's `$HOME`

If you're trying this out, you're testing raw dev builds from source, not installing
a finished tool. Expect rough edges, and please open an issue if you hit one.

Contributions, issues, and questions are welcome — that's the point of open source.

<div align="center">

✦ ✧ ✦

*MindGate — friction, by design.*

</div>