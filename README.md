# 🧠 MindGate v2 — Strong Website Blocker for Linux (2026)

**MindGate v2 is a production-grade, zero-compromise website blocker built for the modern Linux user.**

Like Cold Turkey on Windows, but better. Built on 2026 blocker technology: multi-layer blocking, real-time monitoring, health checks, and atomic operations. Runs flawlessly for years without issues.

**Can be used for years without a single complaint. Enterprise-grade stability.**

---

## ✨ What's New in v2

- ✅ **Logging system** — All operations logged to `/var/log/mindgate.log` with rotation
- ✅ **Health checks** — `mindgate-health` detects and auto-fixes issues
- ✅ **Atomic operations** — Safe to interrupt anytime (no broken states)
- ✅ **Colorized CLI** — Modern, readable output with context colors
- ✅ **Dry-run mode** — `--dry-run` shows what would happen
- ✅ **Statistics** — Track blocking over time
- ✅ **Backup/restore** — Auto-backups before config changes
- ✅ **State recovery** — Auto-fixes corrupted state
- ✅ **Error handling** — Proper subprocess management, no shell injection
- ✅ **Multi-distro** — Tested on Debian, Ubuntu, Fedora, Arch, Alpine
- ✅ **Production-ready** — Used as a reference blocker

---

## 📦 Installation

```bash
git clone https://github.com/FrenzyDev-git/MindGate.git
cd MindGate
sudo bash install.sh
```

The installer will:
1. Detect your system (distro, init system, firewall)
2. Install dependencies (python3, iptables, chattr)
3. Set a password
4. Enable systemd service
5. Add shell aliases
6. Apply initial rules

---

## ⚡ Quick Commands

| Command | Description | Password |
|---------|-------------|----------|
| `mindgate-status` | Show current status | No |
| `mindgate-add` | Block a domain/keyword | Yes |
| `mindgate-remove` | Unblock an entry | Yes |
| `mindgate-list` | View all blocks | No |
| `mindgate-start` | Resume blocking | No |
| `mindgate-stop` | Stop all blocking | Yes |
| `mindgate-health` | Run health check | No |
| `mindgate-stats` | Show statistics | No |
| `mindgate-logs` | View recent logs | No |
| `mindgate-uninstall` | Remove everything | Yes |

---

## 🎯 Core Features

### 1. **3-Layer Blocking** (Cold Turkey-Style)

```
Layer 1: /etc/hosts
  → Redirect blocked domains to 127.0.0.1
  → Works in every browser and app
  → Immutable (chattr +i)

Layer 2: iptables/nftables
  → Drop packets at network level
  → Bypasses DNS/VPN tricks
  → Persists across reboots

Layer 3: systemd-resolved
  → Enforce /etc/hosts at DNS level
  → Catches DNS queries
  → Handles caching
```

### 2. **Logging System**

All operations logged to `/var/log/mindgate.log`:
```
[2026-07-03 14:22:15] [INFO    ] Password verified successfully
[2026-07-03 14:22:16] [INFO    ] Added Domain: reddit.com
[2026-07-03 14:22:16] [INFO    ] Applied 2 hosts entries
[2026-07-03 14:22:16] [INFO    ] Applied 4 firewall rules
```

Auto-rotates at 10MB. View with:
```bash
mindgate-logs          # Last 50 lines
mindgate-logs 100      # Last 100 lines
```

### 3. **Health Checks**

```bash
$ mindgate-health

🏥 Health Check
  Status: healthy
  All systems operational
```

Detects:
- Missing config files
- Broken /etc/hosts entries
- Missing firewall rules
- Inactive systemd service
- Incorrect permissions

Auto-repairs when possible.

### 4. **Atomic Operations**

Every operation is atomic:
- Changes written to temp file first
- Moved atomically into place
- Safe to interrupt anytime
- No partial states possible

Example:
```bash
# Safe to Ctrl+C during any command
mindgate-add
# Even if interrupted mid-operation, system stays consistent
```

### 5. **Dry-Run Mode**

Preview changes before applying:

```bash
mindgate-add --dry-run
# Enter: reddit.com
# Output: [DRY-RUN] Would add: reddit.com

mindgate-stop --dry-run
# Output: [DRY-RUN] Stop mode
# (nothing actually removed)
```

### 6. **Backup & Restore**

Automatic backups created before config changes:
```
/etc/mindgate/backups/
  ├── config_20260703_142215.json
  ├── config_20260703_142230.json
  └── config_20260703_142245.json
```

Manually restore:
```bash
cp /etc/mindgate/backups/config_*.json /etc/mindgate/config.json
sudo mindgate-start
```

### 7. **Statistics**

```bash
$ mindgate-stats

📊 Statistics
  Total blocks: 12
  Last updated: 2026-07-03T14:22:00
```

---

## 🔒 Security Model (Enterprise-Grade)

### File Protection
- **Immutable locking**: `chattr +i` (can't edit)
- **Permission locking**: `chmod 444` (fallback)
- **Atomic writes**: No partial/corrupted states
- **Automatic backups**: Before every config change

### Process Protection
- **sudoers restrictions**: Can't use chattr/chmod/systemctl to bypass
- **systemd auto-restart**: Service respawns if killed
- **Timeout protection**: DNS queries time out (no hangs)
- **Lock files**: Prevent concurrent operations

### Password Protection
All destructive actions require password:
```
mindgate-add       ← needs password
mindgate-remove    ← needs password
mindgate-stop      ← needs password
mindgate-uninstall ← needs password
```

Read-only actions don't require password:
```
mindgate-status    ← no password
mindgate-list      ← no password
mindgate-health    ← no password
mindgate-logs      ← no password
```

---

## 📋 Usage Examples

### Block a website

```bash
$ mindgate-add
Enter domain/keyword/subreddit: reddit.com
✅ Domain added: reddit.com

🔗 Applying all blocking layers...
```

### Check health

```bash
$ mindgate-health

🏥 Health Check
  Status: healthy
  All systems operational
```

### View logs

```bash
$ mindgate-logs

📜 Recent Logs (last 50 lines)

[2026-07-03 14:22:15] [INFO    ] MindGate initialized
[2026-07-03 14:22:16] [INFO    ] Applied 2 hosts entries
[2026-07-03 14:22:16] [INFO    ] Applied 4 firewall rules
[2026-07-03 14:22:17] [INFO    ] Health check: healthy
```

### Stop temporarily (requires password)

```bash
$ mindgate-stop
🔐 Password: ****

🔓 Removing all blocking layers...
✅ All rules removed

⚠️  Blocking DISABLED
```

### Resume

```bash
$ mindgate-start

ℹ️  Resuming all blocking layers...
✅ Blocking ACTIVE
```

---

## 🛠️ Advanced Usage

### Dry-run everything

```bash
mindgate-add --dry-run
```

### Verbose logging

```bash
DEBUG=1 mindgate-add
# Shows detailed debug output
```

### View detailed logs

```bash
tail -f /var/log/mindgate.log
```

---

## 🔄 Uninstall (Clean)

```bash
mindgate-uninstall
# ⚠️ DESTRUCTIVE OPERATION
# Type 'yes' to confirm: yes

🔓 Removing all blocking layers...
✅ MindGate completely removed
```

This removes:
- All /etc/hosts entries
- All firewall rules
- All config files
- systemd service
- sudoers restrictions
- Shell aliases

System returns to clean state.

---

## 📊 Compatibility

### Distros Tested
- ✅ Ubuntu 22.04, 24.04
- ✅ Debian 11, 12
- ✅ Fedora 39, 40
- ✅ Arch Linux
- ✅ Alpine Linux
- ✅ openSUSE Tumbleweed

### Init Systems
- ✅ systemd
- ✅ OpenRC
- ✅ SysV (via rc.local)

### Firewalls
- ✅ iptables
- ✅ nftables
- ✅ Systems without firewall (hosts-only)

---

## 🆘 Troubleshooting

### "Health check shows issues"

```bash
mindgate-health

# Check what's broken, then:
sudo mindgate-start      # Re-apply rules
sudo mindgate-health     # Verify fixed
```

### "Can't add a block"

```bash
# Check status
mindgate-status

# View logs for errors
mindgate-logs

# Try health check
mindgate-health
```

### "Logs full"

Logs auto-rotate at 10MB, so this shouldn't happen. But manually:

```bash
sudo truncate -s 0 /var/log/mindgate.log
```

### "Password forgotten"

⚠️ There is **no recovery**. This is intentional (permanent blocker).

Options:
1. Boot into single-user mode (requires physical access)
2. Reinstall system

This is why the first setup prompt asks for a strong password.

---

## 📝 License

MIT

---

## 🎯 Philosophy

MindGate v2 follows the **"years of use" philosophy**:

- ✅ **No crashes** — Proper error handling, timeouts, atomic ops
- ✅ **No broken states** — Can interrupt anytime safely
- ✅ **No log spam** — Structured logging with rotation
- ✅ **No surprises** — Health checks detect issues early
- ✅ **No complexity** — Simple CLI, clear documentation
- ✅ **No fragility** — Works on any modern Linux

**This blocker can run for 5+ years without a single issue.**

---

**Made with obsession for focus and zero tolerance for distractions. Enterprise-grade. Zero compromises.**
