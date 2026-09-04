# Changelog

## [0.1.0] - 2026-09-04

### MindGate MVP1

First public MVP release of MindGate, a stubborn browser protector for Linux.

### Added

- Chromium extension blocking for websites, keywords, paths, and subreddits.
- Timed and forever focus locks.
- Automatic rule expiry and restoration of the original blocked URL.
- Root-owned Rust daemon that protects the extension through heartbeats.
- Browser shutdown when the extension disappears during an active protection window.
- Shutdown refusal while a lock is active.
- Watchdog service that restarts the daemon after a crash.
- Native Messaging integration between the extension and daemon.
- `mindgate status`, `mindgate doctor`, `mindgate logs`, and service controls.
- Linux session lock and suspend detection through systemd-logind over D-Bus.
- Reconciliation and reconnect handling for dropped D-Bus signals and connections.
- Startup ordering that waits for D-Bus and systemd-logind.

### Reliability fixes

- Prevented false browser kills when XFCE/light-locker freezes Chromium during a session lock.
- Prevented a reboot-time D-Bus race from treating unknown lock state as confirmed unlocked.
- Added a resume grace period so Chromium can thaw and reconnect its extension heartbeat.
- Fixed heartbeat timeout drift between the daemon and CLI status reporting.
- Improved session resolution when the root daemon is outside the graphical session.

### Supported and verified

- Linux x86_64 with systemd.
- Linux Mint with XFCE.
- Google Chrome.
- Rust workspace tests pass with `cargo test --workspace`.

Brave, Edge, Vivaldi, and Opera are expected to work but are not hands-on verified
in this release. Firefox, incognito protection without manual permission,
additional browser profiles, and multi-user protection remain outside MVP1 scope.

### Upgrade note

After installing or upgrading, confirm the services with:

```bash
mindgate doctor
systemctl is-active mindgated mindgate-watchdog
```

A real lock, unlock, suspend/resume, and reboot cycle should be tested on the
target machine before treating the release as production-grade.
