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



## UI Scope



### Identity
Linux tool, terminal/hacker aesthetic. Dark, amber-red — evokes an old
CRT terminal, not a lifestyle app or generic SaaS.

### Fonts
- Headings/terminal/accents: IBM Plex Mono — technical, slightly vintage,
  matches an amber-CRT feel better than a clean corporate mono would
- Body: IBM Plex Sans — same family as the mono for cohesion, reads well
  at paragraph length

### Colors (oklch)
- Background: oklch(0.13 0.01 30)
- Foreground: oklch(0.96 0.01 60)
- Primary/brand: oklch(0.68 0.19 40) — amber-red
- Accent (hover/danger): oklch(0.55 0.22 25) — deeper red
- Muted: oklch(0.22 0.02 30)
- Muted foreground: oklch(0.65 0.02 40)

### Hero
- Headline centers the lock guarantee, not generic "block distracting
  sites" copy
- Terminal component shows the REAL bootstrap command, typed out:

      $ curl -fsSL https://raw.githubusercontent.com/FrenzyDev-git/MindGate/main/installer/Bootstrap.sh | bash
      -> git not found, installing...
      -> Downloading MindGate...
      -> Running installer (you may be asked for your password)...
      ✔ MindGate installed

### Sections (in order)
1. **Hero** — lock-guarantee headline, real curl command in terminal
   (signature section, hand-tune this one)
2. **Why Mindgate** — founder pain story + the 3-way gap (outdated OSS /
   paid / maintained-but-bypassable) + "daemon survives you trying to
   get out of it" as the technical differentiator + active-development
   commitment
3. **How it works** — define block list → set lock duration → daemon
   enforces → auto-clears on timer end
4. **Install steps** — curl command, enable developer mode, load
   unpacked extension. Framed honestly: v1, open source, anyone
   (including the maker) can inspect and improve it — not hidden as a
   weakness
5. **Features** — domain blocking, keyword+DOM scanning, path blocking,
   subreddit blocking
6. **Technical details** — architecture split (extension = sensor,
   daemon = enforcer), platform specifics (verified vs. expected vs. not
   supported), the shutdown-resistance mechanism stated precisely
7. **Known limitations box** — incognito not protected unless enabled,
   other browser profiles need separate loads, different OS
   user/boot-media not defended against, reinstall-while-unlocked always
   possible by design. Framed as "protects against accidental/impulsive
   bypass, not a determined effort to route around the whole OS"
8. **Newsletter signup**
9. **FAQ**
10. **Footer** — real GitHub link, Linux Mint/XFCE tested badge

### What NOT to include
No pricing. No testimonials. No logo cloud. No auth/login/signup —
Mindgate has no accounts.

### Launch copy (X, not the landing page — kept here for reference)
Main post: video attached directly, no link in body. First reply: link.
Second reply (optional, strong per own notes): the light-locker
LockedHint D-Bus bug story as a build-in-public credibility post.