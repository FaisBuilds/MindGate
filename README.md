# 🛡️ Ferocious — Permanent Website Blocker

Ferocious is a **password-protected, system-wide website blocker** for Linux. It blocks domains, keywords, and specific subreddits across all browsers and applications. Once installed, all management actions require the administrator password.

---

## 🔥 Features

| Feature                  | Description                                                 |
| ------------------------ | ----------------------------------------------------------- |
| 🌐 System-wide Blocking  | Works across all browsers and applications                  |
| 🔒 Password Protection   | Prevents unauthorized changes or removal                    |
| 🚫 Domain Blocking       | Block entire websites (e.g., `reddit.com`)                  |
| 🔍 Keyword Blocking      | Block URLs containing specific keywords                     |
| 📌 Subreddit Blocking    | Block individual subreddits while keeping Reddit accessible |
| 🛡️ Immutable Protection | Critical files locked using `chattr +i`                     |
| 🔄 Auto-Restart          | Automatically respawns through systemd if stopped           |
| 🧹 Clean Uninstall       | Removes all Ferocious files and services                    |

---

## 📦 Installation

### Clone or Download

```bash
git clone <repository-url>
cd ferocious
```

### Install Ferocious

```bash
sudo ./install.sh
```

During installation you will be prompted to create an administrator password.

After installation, Ferocious starts automatically and launches at system boot.

---

# ⚡ Commands

## Check Status

```bash
ferocious-status
```

Displays whether Ferocious is currently running and active.

---

## Add a Block Rule

```bash
ferocious-add
```

Example:

```text
Enter domain, keyword, or subreddit to block:
reddit.com

✅ Domain 'reddit.com' added.
```

Supported formats:

```text
reddit.com
porn
r/gonewild
```

---

## Import a Blocklist

```bash
ferocious-import
```

Opens a file picker and imports domains, keywords, and subreddit rules from a text file.

Example blocklist:

```text
reddit.com
facebook.com
porn
r/gonewild
```

---

## Edit Blocklist

```bash
ferocious-edit
```

Opens the blocklist editor.

Administrator password required.

---

## Start Ferocious

```bash
ferocious-start
```

Starts the blocking service.

---

## Stop Ferocious

```bash
ferocious-stop
```

Temporarily stops the service.

Administrator password required.

---

## Restart Ferocious

```bash
ferocious-restart
```

Reloads all rules and restarts the service.

Administrator password required.

---

## View Current Rules

```bash
ferocious-list
```

Displays all active domains, keywords, and subreddit rules.

---

## Change Password

```bash
ferocious-password
```

Updates the administrator password.

---

## Uninstall Ferocious

```bash
ferocious-uninstall
```

Completely removes:

* Systemd service
* Block rules
* Configuration files
* Password database
* Executables

Administrator password required.

---

# 📁 Block Types

## Domain Blocking

Blocks an entire website:

```text
reddit.com
twitter.com
facebook.com
```

---

## Keyword Blocking

Blocks URLs containing specific words:

```text
porn
nsfw
gambling
```

Examples:

```text
https://example.com/porn
https://site.com/nsfw-content
```

---

## Subreddit Blocking

Blocks specific subreddits while keeping Reddit accessible:

```text
r/gonewild
r/nsfw
r/porn
```

Example:

✅ Allowed

```text
reddit.com/r/linux
```

❌ Blocked

```text
reddit.com/r/gonewild
```

---

# 🔒 Security Model

Ferocious is designed to resist casual bypass attempts:

* Password required for all management actions
* Protected systemd service
* Immutable configuration files
* Automatic service recovery
* System-wide blocking independent of browser extensions

---

# 📄 License

MIT License

