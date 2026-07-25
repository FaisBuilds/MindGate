# MindGate — MVP1 Scope

This document describes **what MindGate promises to a user**, in plain language.
It intentionally says nothing about file names, code structure, or protocols —
that lives in `CONTEXT.md` and is expected to change often. This file
should rarely need to change. If you're tempted to edit this mid-build for a
technical reason, you're editing the wrong document.

Anything not written here is not part of MVP1.

---

## What MindGate blocks

- **Websites** — by domain (and subdomains of it).
- **Keywords** — matched against the current page URL, and against visible
  page text (DOM content scan) on the initial load and on in-page
  (SPA-style) navigation.
- **Paths** — a specific path prefix under a specific domain.
- **Subreddits** — legacy rule type, matched against Reddit URLs.

All of the above are enforced by the browser extension. The extension is the
only component that knows or decides what's blocked.

## What a "lock" guarantees

- Once locked, the block list cannot be edited or cleared before the lock's
  end time — whether "end time" is a specific duration or "forever" (no
  automatic expiry).
- While locked, the `stop`/shutdown command for the background daemon is
  refused. The daemon will not exit voluntarily while it believes a lock is
  active.
- Closing the browser, or disabling/removing the extension, does not end a
  lock. If the browser or extension goes away while a lock should still be
  active, the daemon closes running browser processes rather than silently
  letting protection lapse.
- When a lock's timer ends, the block clears automatically — no user action
  required, and the previously-blocked tab returns to the page it was
  actually trying to reach.

## Platforms & browsers supported in MVP1

- **OS:** Linux, x86_64. Verified on Linux Mint (Debian-based) + XFCE.
  Expected to work on other systemd-based distributions (including Arch)
  since the daemon and watchdog only depend on `systemctl`, not a specific
  distro — but only Mint has been hands-on tested so far.
- **Browsers:** Google Chrome, verified directly. Brave, Microsoft Edge,
  Vivaldi, and Opera are expected to work since they share the same
  extension APIs and are already listed as processes the daemon watches for
  — but each requires its own native-messaging host registration and its
  own copy of the extension loaded, and neither has been hands-on tested yet.
- **Firefox and other non-Chromium browsers:** not supported in MVP1. See
  "Deferred" below.

## Known limitations (by design, not bugs)

These are not defended against in MVP1. They are conscious scope
boundaries, not gaps to be silently fixed:

- **Incognito windows** are not protected unless the user manually enables
  "Allow in Incognito" for the extension — Chrome does not allow an
  extension to grant itself incognito access.
- **Other browser profiles** are not protected unless the extension is
  separately loaded into each one a user wants covered.
- **A different OS user account**, or booting different media entirely,
  is not defended against. MVP1 protects a browser session set up by the
  user who installed it, against accidental or impulsive bypass — not
  against a determined effort to fully route around the whole OS.
- **Uninstalling and reinstalling everything** (daemon + extension) while
  unlocked is always possible, by design — you can't lock yourself out
  permanently by accident, only lock yourself into a specific session.

## Deferred to post-MVP1 (explicitly not now)

- A native Firefox build (the extension's WebExtension APIs and MV3 support
  differ enough from Chromium that this is separate work, not a port).
- Distro-specific install scripts beyond Debian/Mint (Arch, Fedora, etc.).
- Chrome Web Store distribution (MVP1 ships as a manually-loaded unpacked
  extension; Web Store submission runs in parallel, not a launch blocker).
- Any defense against incognito, multi-profile, or multi-user bypass beyond
  clearly documenting the limitation to the user.

---

*Last written: reflects the state of the project as of this conversation.
When scope actually changes (a feature is added or a platform promise is
made), edit this file deliberately as its own change — don't let it drift
via unrelated architecture work.*