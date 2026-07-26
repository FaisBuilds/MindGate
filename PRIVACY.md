# MindGate Privacy Policy

*Last updated: July 26, 2026*

MindGate is built to run entirely on your own machine. This policy explains exactly what data the browser extension and the local daemon touch, and — just as important — what they don't.

## The short version

- MindGate does not have a server. There is nothing to send your data *to*.
- Nothing MindGate reads ever leaves your computer.
- No analytics, no telemetry, no tracking, no accounts.
- Everything described below is also visible directly in the source code, since MindGate is open source: https://github.com/FrenzyDev-git/MindGate

## What the extension accesses, and why

MindGate's Chrome extension requests broad permissions because its entire job is watching what you browse and blocking specific things. Here's what each one is actually used for:

| Permission | What it's for |
|---|---|
| Access to all URLs (`<all_urls>` host permission) | Needed to check the URL of any page you visit against your block list. Without this, MindGate could only protect a handful of sites you'd pre-approved, which defeats the purpose. |
| Reading page content (content script) | Used only to scan visible page text against your configured **keywords**. This runs entirely in your browser — the text is never transmitted anywhere, not even to the local daemon. It's compared against your own keyword list and then discarded. |
| `storage` | Your block list (websites, keywords, paths, subreddits) and lock settings are stored using Chrome's local extension storage, on your device only. |
| `webNavigation` / `declarativeNetRequest` | Used to detect and redirect blocked navigations to MindGate's own block page. |
| `alarms` | Used for MindGate's internal timers — lock expiry and a periodic health-check heartbeat (see below). |
| `nativeMessaging` | Used to talk to the local `mindgated` daemon on your own machine — see next section. No data crosses this connection except a simple "I'm still running" signal and the current lock state. |
| `tabs` / `activeTab` | Used to redirect a blocked tab to MindGate's block page, and to return you to the real page once a lock expires. |

## What the local daemon (`mindgated`) accesses, and why

The daemon runs on your machine and communicates with the extension over a local Unix domain socket — never over the network. It deliberately knows very little:

- It receives a periodic **heartbeat** from the extension (roughly every 30 seconds) confirming the extension is still running, along with the current lock state (locked or not, and until when).
- It does **not** know what websites, keywords, or paths you've blocked. That information lives only in the extension, on your device.
- It uses `SO_PEERCRED` (a Linux kernel feature) to confirm that only your own user account, or root, can send it commands — a different local user on the same machine cannot query or control it.
- It never makes an outbound network connection. There is no server for it to talk to, and no telemetry it reports.

## Data storage and retention

- Your block list and lock state are stored locally via Chrome's extension storage APIs, on your device, for as long as the extension is installed.
- Uninstalling the extension removes this data. Running MindGate's uninstaller removes the daemon, its configuration, and systemd services — extension data in your browser profile is left untouched (removing the extension itself handles that).

## What MindGate does not do

- It does not collect analytics or usage statistics.
- It does not create a user account or ask for one.
- It does not share, sell, or transmit any browsing data to anyone, including us — because there is no "us" on the other end of a connection. There is no backend server in the current version of MindGate.
- It does not use cookies or any web-based tracking of its own.

## If this ever changes

Some planned future features (for example, an optional companion mobile app to remotely manage a lock, or cross-device sync) would require a server component. If and when that happens:

- Those features will be clearly optional and separate from the core extension and daemon described here.
- This policy will be updated *before* any such feature ships, with a clear explanation of exactly what data that specific feature sends and why.
- The core local blocking and daemon functionality described above is not expected to change in this regard — it's a deliberate design choice, not a temporary one.

## Open source

Because MindGate is open source, you don't have to take this document's word for it. Every claim above can be checked directly against the code:

- Extension: `extension/` in the repository
- Daemon: `daemon/` in the repository

## Contact

Questions about this policy or MindGate's data practices: [faisal.eng41@gmail.com](mailto:faisal.eng41@gmail.com)