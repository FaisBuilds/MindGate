# MindGate MVP1 — Architecture

## Vision

MindGate is a **stubborn browser protector**.

The browser extension blocks distractions.

The daemon protects the extension.

The goal is not system-wide filtering.
The goal is making browser-based distractions difficult to bypass.

---

# Core Architecture

```text
Browser Extension
        │
 Native Messaging
        │
 MindGate Daemon ── adapters/ (optional, narrow-interface additions)
        │
    Watchdog
```

Per `ADAPTER.md`: the core (`daemon/`, `extension/`) is treated as frozen by
default. New capabilities — a new browser, a new platform behavior, a bug
fix that needs new information the core doesn't have — are built as
self-contained crates or modules under `adapters/`, wired into core only
through a narrow, explicit interface, and only with deliberate approval.
This document describes core. See `ADAPTER.md` for how the adapter layer
around it works, and `adapters/<name>/` for what currently exists there.

### Extension

Responsible for:

* Website blocking
* Keyword blocking
* Path blocking
* Subreddit blocking
* Premium block page
* Motivational quotes
* Settings UI
* Rule management
* Heartbeat (includes the current lock state — see "Rule Storage" below)

---

### Daemon

Responsible for:

* Verifying extension heartbeat
* Detecting extension removal
* Detecting extension crashes
* Closing supported browsers when protection disappears
* Running as a systemd service
* Native Messaging bridge
* CLI
* Install / Uninstall
* Managing protection state
* Consulting the `system-lock-resume` adapter before treating a stale
  heartbeat as "the extension is gone" — see "Adapters" below

The daemon **never decides what to block.**

It only protects the blocker.

---

### Watchdog

Responsible for:

* Monitoring daemon
* Restarting daemon if it crashes
* Starting on boot
* Ensuring protection survives reboots

---

# Adapters

New capabilities that need information the core doesn't natively have are
built as isolated crates under `adapters/`, not folded into core. Full
rules for this live in `ADAPTER.md`; this section just names what
currently exists and why.

### `system-lock-resume`

Answers one question for the daemon: **is the user's Linux session
currently locked, or is the system currently suspended?**

This exists because a locked or suspended session can legitimately stop
the extension's heartbeat from arriving for longer than the daemon's
timeout — not because the extension crashed or was disabled, but because
the browser process itself gets frozen by the OS while locked. Without
this adapter, the daemon would treat that as "the extension is dead" and
close the browser mid-lock, which is exactly the failure MindGate exists
to prevent.

It works by watching `systemd-logind` over D-Bus (`org.freedesktop.login1`)
— not a desktop-environment-specific screensaver API — so it behaves the
same way on GNOME, KDE, and XFCE. It combines fast-path signals
(`PrepareForSleep`, `Lock`/`Unlock`) with a periodic direct property
reconciliation, since a single dropped signal (a confirmed, filed systemd
bug) or an unreliable screen-locker (confirmed in testing: XFCE's
`light-locker` doesn't always keep its `LockedHint` property in sync, even
though it fires the `Lock`/`Unlock` signals correctly) can otherwise leave
the reported state wrong. It fails safe to "unlocked" on any error, so a
bug in this adapter can only ever degrade the daemon back to its
pre-adapter behavior, never introduce a new failure mode.

`guardian.rs` (core) consults this adapter's `is_locked()` through a
narrow interface — a single boolean — before deciding whether a stale
heartbeat means "close the browser." The adapter has no other access to
core state.

---

# Blocking

The extension supports:

### Websites

```
youtube.com
reddit.com
```

---

### Keywords

```
porn
nsfw
gambling
```

Blocks URLs and searches containing configured keywords.

---

### Paths

```
/shorts
/reels
/explore
```

Allows blocking addictive sections without blocking the whole website.

---

### Subreddits

```
r/gonewild
r/nsfw
```

Subreddit-specific blocking.

---

# Browser Support

Support every Chromium browser:

* Google Chrome
* Chromium
* Brave
* Microsoft Edge
* Vivaldi
* Opera

Firefox comes later.

---

# Stubbornness ⭐

MindGate is difficult to bypass.

### Protection

* Daemon starts on boot.
* Watchdog starts on boot.
* Both monitor each other.
* Extension continuously sends heartbeat.
* Missing heartbeat triggers browser shutdown — *unless* the
  `system-lock-resume` adapter confirms the session is genuinely locked
  or suspended, in which case the daemon waits instead.
* Removing or disabling the extension closes supported browsers.
* Daemon automatically recovers from crashes.

Protection should continue after reboot without user interaction.

---

# Installation

One command.

```bash
curl -fsSL https://raw.githubusercontent.com/FrenzyDev-git/MindGate/main/installer/Bootstrap.sh | bash
```

Installer automatically:

### Environment

* Verifies a systemd-based environment
* Installs a Rust toolchain (via rustup) and a C linker if missing
* Checks `systemd-logind` reachability, so the `system-lock-resume`
  adapter is verified working before the daemon ever starts
* Builds MindGate from source
* Registers Native Messaging
* Registers systemd services (daemon + watchdog)

---

### Browser Detection

Writes Native Messaging manifests to every supported Chromium browser's
global native-messaging-hosts directory, so any of them work without a
separate per-browser install step.

---

### User Setup

Displays the one remaining manual step — Chrome doesn't allow an
installer to do this part:

* Load unpacked extension
* Enable Developer Mode
* Enable "Allow in Incognito"
* Install extension into every browser profile

The installer best-effort opens `chrome://extensions` directly so the
user lands on the right page without hunting for the URL themselves.

---

# Uninstall

One command.

```bash
sudo ./installer/uninstall.sh
```

Completely removes:

* Daemon
* Watchdog
* systemd services and unit files
* Native Messaging manifests (from the same global directories
  `install.sh` wrote them to)
* Binaries and helper scripts
* Configuration directory (`/etc/mindgate`)

Leaves browsers untouched. Extension removal from `chrome://extensions`
remains a manual step, by design — MindGate doesn't reach into your
browser profile to do that for you.

---

# CLI

Simple.

```bash
mindgate install
mindgate uninstall

mindgate start
mindgate stop
mindgate restart
mindgate status

mindgate doctor
mindgate logs
```

`stop`, `restart`, `uninstall`, and `install` all refuse to proceed while
a genuine lock is active — enforced daemon-side, not just in the CLI, so
it can't be bypassed by skipping the CLI entirely.

---

### Doctor

Checks:

```
✓ Daemon running
✓ Watchdog running
✓ Native Messaging installed
✓ Extension connected
✓ Heartbeat healthy
✓ Browser detected

⚠ Incognito disabled
⚠ Extension missing in Profile 2
```

---

# Rule Storage

Block rules (websites, keywords, paths, subreddits) are owned entirely by
the extension and never sent to the daemon — the daemon has no way to
know what's blocked, by design, per "The daemon never decides what to
block."

One related but distinct thing *is* shared: the extension's current
**lock state** (locked or not, and until when) rides along on every
heartbeat. This is what lets the daemon refuse `stop`/shutdown while
genuinely locked, and what the `system-lock-resume` adapter's `is_locked`
check gets weighed against when a heartbeat goes stale. This is narrower
than full rule sync — the daemon still has zero knowledge of *what's*
blocked, only *whether* a lock is currently active.

---

# Block Page

Premium experience.

Contains:

* Quote
* Reason for block
* Calm visual design
* MindGate branding

No guilt.

No aggression.

---

# Not Included (MVP2+)

* DNS blocking
* nftables
* iptables
* dnsmasq
* System-wide blocking
* Firefox
* Automatic profile installation
* Automatic Guest Mode enforcement
* Browser Store publishing
* Password protection
* Windows
* macOS
* Cloud sync
* User accounts
* Analytics
* AI features

---

# Definition of Done

A brand-new Linux user can:

1. Install MindGate using one command.
2. Load the extension.
3. Enable Incognito.
4. Install the extension into every browser profile.
5. Open any supported Chromium browser.
6. Websites, keywords, paths and subreddits block correctly.
7. Premium block page appears.
8. Removing or disabling the extension closes the browser automatically.
9. Locking the screen or suspending mid-lock does **not** trigger a false
   shutdown, and protection resumes correctly on unlock/wake.
10. Reboot the PC and remain protected.
11. Run `mindgate doctor` and pass every critical check.
12. Completely uninstall MindGate using one command, with no leftover
    Native Messaging manifests or systemd units.

If all of these work reliably on systemd-based Linux distributions,
**MindGate MVP1 is complete.**