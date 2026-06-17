# 🧠 MindGate — Permanent Website Blocker for Linux

MindGate is a **ruthless, permanent, password-protected website blocker** for Linux. It blocks domains, keywords, and subreddits at the **network level** (iptables) and **DNS level** (systemd-resolved), making it nearly impossible to bypass. Like Cold Turkey, but for Linux.

Once installed, all management is locked behind a password. **No UI tricks. No timer workarounds. Just pure blocking power.**

---

## 🔥 Why MindGate is Powerful

| Feature | Why It Matters |
|---------|----------------|
| **Network-Level Blocking** | Uses `iptables` to drop packets—works even if you change DNS |
| **DNS-Level Blocking** | Blocks at `systemd-resolved` layer—can't bypass with VPN easily |
| **Immutable Config** | Files locked with `chattr +i`—can't edit even as sudo |
| **Auto-Recovery** | systemd service auto-respawns if killed |
| **Password Protected** | All critical actions require password |
| **System-Wide** | Works across ALL browsers and applications |
| **No Exceptions** | Cold Turkey-level stubbornness—blocks until you decide otherwise |

---

## 📦 Installation

### Clone the repository

```bash
git clone <repository-url>
cd MindGate
```

### Run the installer

```bash
sudo chmod +x install.sh
sudo ./install.sh
```

During installation, you'll set an **administrator password**. This password is required for all sensitive operations.

After installation:
- MindGate starts automatically
- Launches at system boot
- Blocks are active immediately

---

# ⚡ Commands

All commands start with `mindgate-` and require administrator password for sensitive operations.

## Check Status

```bash
mindgate-status
```

Shows whether MindGate is running and displays active rules.

**Output:**
```
🧠 MindGate is ACTIVE
   Blocked domains: 5
   Blocked keywords: 4
   Service: running
   iptables rules: active
```

---

## Add a Single Block Rule

```bash
mindgate-add
```

Prompts you to enter a domain, keyword, or subreddit.

**Example:**
```text
Enter domain, keyword, or subreddit to block:
reddit.com

✅ Domain 'reddit.com' added to blocklist.
```

**Supported Formats:**
```
reddit.com          # Domain
porn                # Keyword (blocks any URL containing 'porn')
r/gonewild          # Subreddit (blocks reddit.com/r/gonewild)
```

---

## Import a Blocklist File

```bash
mindgate-import
```

Opens a file picker to import a plain-text blocklist. One entry per line.

**Example blocklist.txt:**
```
# Domains
reddit.com
facebook.com
twitter.com

# Keywords
porn
nsfw
gambling

# Subreddits
r/gonewild
r/nsfw
```

**Result:**
```
✅ Import complete!
   Domains added: 3
   Keywords added: 3
   Subreddits added: 2
```

---

## View Current Blocklist

```bash
mindgate-list
```

Displays all active blocking rules.

**Output:**
```
🧠 MindGate Blocklist:

Domains (5):
  - reddit.com
  - facebook.com
  - youtube.com
  - twitter.com
  - instagram.com

Keywords (4):
  - porn
  - nsfw
  - xxx
  - 18+

Subreddits (3):
  - r/gonewild
  - r/nsfw
  - r/porn
```

---

## Edit the Blocklist

```bash
mindgate-edit
```

Opens the blocklist in your text editor (nano by default). Requires password.

**Note:** The config file is locked after editing.

---

## Stop MindGate (Temporarily)

```bash
mindgate-stop
```

**⚠️ Requires password.** Temporarily disables blocking. Useful for debugging or maintenance.

Blocking resumes when you run:
```bash
mindgate-start
```

---

## Start MindGate

```bash
mindgate-start
```

Starts the blocking service. No password needed.

---

## Restart MindGate

```bash
mindgate-restart
```

Reloads all rules and restarts the service. Requires password.

---

## Change Administrator Password

```bash
mindgate-password
```

Changes your administrator password. Requires the current password.

---

## Uninstall MindGate

```bash
mindgate-uninstall
```

**⚠️ Requires password.** Completely removes MindGate and all its configurations.

**What gets removed:**
- All blocking rules
- Config files and password database
- systemd service
- iptables and DNS rules
- All MindGate executables
- Bash aliases from ~/.bashrc

---

# 📁 Block Types

## Domain Blocking

Blocks entire websites by blocking traffic to their IP addresses.

```
reddit.com
facebook.com
twitter.com
```

**How it works:**
1. Domain is resolved to IP
2. iptables rule drops all packets to that IP
3. Cannot be bypassed with DNS changes

---

## Keyword Blocking

Blocks URLs containing specific keywords.

```
porn
nsfw
gambling
```

**Examples blocked:**
```
https://example.com/porn
https://site.com/nsfw-content
https://gambling-site.com/
```

---

## Subreddit Blocking

Blocks specific subreddits while keeping Reddit accessible (optional).

```
r/gonewild
r/nsfw
r/porn
```

**Allowed:**
```
reddit.com/r/linux
reddit.com/r/python
```

**Blocked:**
```
reddit.com/r/gonewild
reddit.com/r/nsfw
```

---

# 🔒 Security Model

MindGate is designed to be **impossible to bypass** without the password:

- ✅ **Network-level blocking** — Can't bypass with DNS or proxy
- ✅ **Immutable files** — Config locked with `chattr +i` (kernel-level)
- ✅ **Password protection** — Required for all management
- ✅ **Auto-recovery** — Service automatically respawns if killed
- ✅ **Sudo restrictions** — sudoers rules prevent direct file modification
- ✅ **Persistent** — Survives reboots

---

# 📄 License

MIT License

---

**Made with obsession for focus.**
