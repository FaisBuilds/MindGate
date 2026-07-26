<div align="center">

<img src="extension/icons/icon128.png" alt="MindGate" width="96" />

# MindGate

**A stubborn browser protector for Linux.**
Blocks distractions in your browser, guarded by a local daemon that doesn't take "stop" for an answer.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Linux](https://img.shields.io/badge/platform-Linux-3776AB)](#supported-platforms)
[![Made with Rust](https://img.shields.io/badge/daemon-Rust-orange)](#architecture-in-one-paragraph)
[![Chrome Extension](https://img.shields.io/badge/browser-Chromium-4285F4)](#supported-platforms)

*Protect your future self.*

</div>

---

## Why MindGate exists

Browser extensions that block distracting sites are easy to disable the moment willpower runs out — which is exactly the moment you need them most. MindGate splits itself into two pieces on purpose:

- A **browser extension** that owns all blocking decisions — websites, keywords, paths, subreddits.
- A **local daemon** (`mindgated`) whose only job is protecting that extension — it notices if the extension goes quiet and closes your browsers, and it refuses to shut itself down while a focus lock is active.

Neither piece can fully undo a lock on its own. That's the whole point.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/FrenzyDev-git/MindGate/main/installer/Bootstrap.sh | bash
```

This one command clones the repo, builds the daemon from source, and wires up systemd + native messaging. First run takes a few minutes (compiling Rust dependencies) — that's expected, not a hang.

Prefer to read the script before running it? That's encouraged, not just tolerated:

```bash
git clone https://github.com/FrenzyDev-git/MindGate.git
cd MindGate
sudo ./installer/install.sh
```

MindGate is fully open source specifically so you don't have to take a root-owned daemon on faith.

After installing, `installer/install.sh` prints the last manual step — loading the extension in `chrome://extensions` — since no installer can do that part for you.

## Features

| | |
|---|---|
| 🌐 **Website blocking** | Block entire domains, subdomains included |
| 🔑 **Keyword blocking** | Matches URLs *and* live page content |
| 📁 **Path blocking** | Block a specific path under a specific domain (e.g. `reddit.com/r/gaming`) |
| 🔒 **Focus Lock** | Timed or "forever" — the block list can't be edited or cleared early |
| 🛡️ **Daemon-guarded** | If the extension goes dark mid-lock, the daemon closes your browsers rather than letting protection lapse |
| 🚫 **Stop-resistant** | The daemon refuses to shut down while a lock is active |

## How a Focus Lock actually holds

1. You lock in the extension popup — a duration, or forever.
2. The block list is frozen. No edits, no early unlock.
3. The daemon is told immediately, not on some slow timer — it now refuses `stop` too.
4. Close the browser, disable the extension, whatever — the daemon notices and closes browser processes rather than quietly letting the block lapse.
5. When the timer runs out, everything reopens on its own. No fiddling, no new tab required.

## Supported platforms

- **OS:** Linux (systemd-based). Verified on Linux Mint + XFCE; expected to work on Arch and other systemd distros, since the daemon only depends on `systemctl`.
- **Browsers:** Google Chrome, verified directly. Brave, Edge, Vivaldi, and Opera are expected to work (same extension APIs, already watched for by the daemon) but not yet hands-on tested.
- **Not yet supported:** Firefox, non-systemd distros. See [`SCOPE.md`](SCOPE.md) for the full, honest list of what's in and out of scope right now.

## Architecture, in one paragraph

The extension (Manifest V3, `declarativeNetRequest` + `webNavigation`) owns every blocking decision and never asks the daemon for permission to block something. The daemon (`mindgated`, Rust) knows almost nothing about *what's* blocked — it only tracks whether the extension is alive (via a heartbeat over native messaging) and whether a lock is currently active, so it can refuse to shut down and can close browsers if the extension disappears. A companion watchdog service and the daemon watch each other, so stopping MindGate means deliberately stopping two independent systemd units, not just one. Full detail lives in `ARCHITECTURE.md`.

## Contributing

Issues and PRs welcome. Before proposing a new feature, skim [`SCOPE.md`](SCOPE.md) — it's the single source of truth for what MindGate currently promises, so we don't relitigate scope in every conversation.

## License

MIT — see [`LICENSE`](LICENSE). Built to be trusted, not just used.

---

<div align="center">
<sub>Not affiliated with Google, Chrome, or any browser vendor. MindGate protects you from yourself — not from a determined attacker with full access to your machine. See <a href="SCOPE.md">SCOPE.md</a> for what's explicitly out of scope.</sub>
</div>