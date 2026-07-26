# MindGate MVP1

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
 MindGate Daemon
        │
    Watchdog
```

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
* Heartbeat

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
* Missing heartbeat triggers browser shutdown.
* Removing or disabling the extension closes supported browsers.
* Daemon automatically recovers from crashes.

Protection should continue after reboot without user interaction.

---

# Installation

One command.

```bash
curl ... | bash
```

Installer should automatically:

### Environment

* Detect Debian-based distro
* Verify supported environment
* Install missing dependencies
* Build/install MindGate
* Register Native Messaging
* Register systemd services

---

### Browser Detection

Detect installed Chromium browsers.

Generate Native Messaging manifests automatically.

---

### User Setup

Display remaining manual steps:

* Load unpacked extension
* Enable Developer Mode
* Enable "Allow in Incognito"
* Install extension into every browser profile

Installer verifies completion before finishing.

---

# Uninstall

One command.

```bash
mindgate uninstall
```

Should completely remove:

* Daemon
* Watchdog
* systemd services
* Native Messaging manifests
* CLI
* Configuration
* Logs
* MindGate directory

Leave browsers untouched.

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

---

### Doctor

Checks:

```
✓ Daemon running

✓ Watchdog running

✓ Native Messaging installed

✓ Extension connected

✓ Browser detected

✓ Heartbeat healthy

⚠ Incognito disabled

⚠ Extension missing in Profile 2
```

---

# Rule Storage

Rules are managed by the extension.

Categories:

* Websites
* Keywords
* Paths
* Subreddits

Future versions may synchronize them with the daemon.

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
9. Reboot the PC and remain protected.
10. Run `mindgate doctor` and pass every critical check.
11. Completely uninstall MindGate using one command.

If all of these work reliably on Debian-based distributions, **MindGate MVP1 is complete.**

read this and wait for prompt