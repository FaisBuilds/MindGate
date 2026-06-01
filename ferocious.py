#!/usr/bin/env python3
"""
Ferocious — Permanent Website Blocker
- Runs as daemon
- Blocks domains and keywords
- Requires password to stop
- No timer — permanent until YOU decide
"""

import os
import sys
import json
import hashlib
import subprocess
from pathlib import Path

# Auto-elevate to sudo if not already root
if os.geteuid() != 0:
    print("🛡️  Ferocious needs root privileges. Elevating with sudo...")
    args = sys.argv.copy()
    os.execvp('sudo', ['sudo', 'python3'] + args)
    sys.exit(1)  # Never reached

# Configuration
CONFIG_DIR = "/etc/ferocious"
CONFIG_FILE = f"{CONFIG_DIR}/config.json"
PASSWORD_FILE = f"{CONFIG_DIR}/password.hash"
LOCK_FILE = f"{CONFIG_DIR}/active"
LOG_FILE = "/var/log/ferocious.log"

def setup():
    """Initial setup — run once as root"""
    os.makedirs(CONFIG_DIR, exist_ok=True)
    
    # Set password
    if not os.path.exists(PASSWORD_FILE):
        pwd = input("Set a password for Ferocious: ")
        pwd_hash = hashlib.sha256(pwd.encode()).hexdigest()
        with open(PASSWORD_FILE, 'w') as f:
            f.write(pwd_hash)
        os.chmod(PASSWORD_FILE, 0o600)
        print("✓ Password set.")
    
    # Create default config if not exists
    if not os.path.exists(CONFIG_FILE):
        config = {
            "blocked_domains": [
                "reddit.com",
                "youtube.com",
                "twitter.com",
                "facebook.com",
                "instagram.com"
            ],
            "blocked_keywords": [
                "porn",
                "nsfw",
                "xxx",
                "18+"
            ],
            "blocked_subreddits": [
                "porn",
                "nsfw",
                "gonewild"
            ]
        }
        with open(CONFIG_FILE, 'w') as f:
            json.dump(config, f, indent=2)
        print("✓ Default config created.")
    
    # Mark as active
    with open(LOCK_FILE, 'w') as f:
        f.write("active")
    
    # Lock files (kernel-level immutable)
    os.system(f"chattr +i {CONFIG_FILE}")
    os.system(f"chattr +i {PASSWORD_FILE}")
    os.system(f"chattr +i {LOCK_FILE}")
    
    print("\n" + "="*50)
    print("FEROCIOUS IS ACTIVE.")
    print("="*50)
    print(f"Config: {CONFIG_FILE}")
    print(f"To uninstall: sudo python3 {sys.argv[0]} --uninstall")
    print("="*50)

def verify_password():
    """Verify password before allowing sensitive Action"""
    pwd_attempt = input("Enter Ferocious password to continue: ")
    with open(PASSWORD_FILE, 'r') as f:
        stored_hash = f.read().strip()
    return hashlib.sha256(pwd_attempt.encode()).hexdigest() == stored_hash

def uninstall():
    """Uninstall Ferocious completely — NO TRASH LEFT"""
    print("\n⚠️  WARNING: This will disable ALL protection.")
    if not verify_password():
        print("❌ Incorrect password. Ferocious remains active.")
        sys.exit(1)
    
    print("✓ Password correct. Uninstalling...")
    
    # 1. Stop and disable systemd service
    print("  ⚙️ Stopping service...")
    os.system("systemctl stop ferocious 2>/dev/null")
    os.system("systemctl disable ferocious 2>/dev/null")
    os.system("rm -f /etc/systemd/system/ferocious.service")
    os.system("systemctl daemon-reload")
    
    # 2. Unlock files
    print("  🔓 Unlocking files...")
    os.system(f"chattr -i {CONFIG_FILE} 2>/dev/null")
    os.system(f"chattr -i {PASSWORD_FILE} 2>/dev/null")
    os.system(f"chattr -i {LOCK_FILE} 2>/dev/null")
    
    # 3. Remove entire Ferocious directory
    print("  🗑️ Removing /etc/ferocious/...")
    os.system(f"rm -rf {CONFIG_DIR}")
    
    # 4. Remove scripts
    print("  🗑️ Removing scripts...")
    os.system("rm -f /usr/local/bin/ferocious.py")
    os.system("rm -f /usr/local/bin/ferocious-proxy.py")
    
    # 5. Remove sudoers restriction
    print("  🗑️ Removing sudoers restriction...")
    os.system("rm -f /etc/sudoers.d/ferocious")
    
    # 6. Reset proxy
    print("  🌐 Resetting proxy...")
    os.system('gsettings set org.gnome.system.proxy mode "none" 2>/dev/null')
    os.system('gsettings set org.gnome.system.proxy lock false 2>/dev/null')
    
    # 7. Remove ALL bash aliases (including add/import)
    print("  🐚 Removing bash aliases...")
    os.system("sed -i '/# Ferocious commands/d' ~/.bashrc")
    os.system("sed -i '/alias ferocious-/d' ~/.bashrc")
    
    # 8. Optional: remove mitmproxy
    print("  📦 Optional: Remove mitmproxy? (y/n)")
    choice = input("  Remove mitmproxy package? (y/n): ").strip().lower()
    if choice == 'y':
        os.system("apt remove -y mitmproxy")
        print("  ✓ mitmproxy removed.")
    else:
        print("  ℹ️  mitmproxy kept (other apps may need it)")
    
    print("\n" + "="*50)
    print("✅ Ferocious COMPLETELY REMOVED.")
    print("="*50)
    print("   Removed:")
    print("   - /etc/ferocious/ (config, password, blocklist)")
    print("   - /usr/local/bin/ferocious*.py")
    print("   - /etc/systemd/system/ferocious.service")
    print("   - /etc/sudoers.d/ferocious")
    print("   - All ferocious-* aliases from ~/.bashrc")
    print("   - System proxy reset to 'none'")
    if choice == 'y':
        print("   - mitmproxy package")
    print("")
    print("📌 You may need to restart your browser for proxy changes.")
    print("="*50)

def status():
    """Check if Ferocious is active"""
    if os.path.exists(LOCK_FILE):
        print("🛡️  Ferocious is ACTIVE")
        with open(CONFIG_FILE, 'r') as f:
            config = json.load(f)
        print(f"   Blocked domains: {len(config.get('blocked_domains', []))}")
        print(f"   Blocked keywords: {len(config.get('blocked_keywords', []))}")
    else:
        print("⚠️  Ferocious is NOT active")


def edit_config():
    """Edit the blocklist — requires password"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted.")
        return
    
    # Unlock the config file temporarily
    os.system(f"chattr -i {CONFIG_FILE} 2>/dev/null")
    
    # Open in editor
    editor = os.environ.get('EDITOR', 'nano')
    os.system(f"{editor} {CONFIG_FILE}")
    
    # Lock it back
    os.system(f"chattr +i {CONFIG_FILE}")
    print("✓ Config updated and locked")

def stop_service():
    """Stop Ferocious service — requires password"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted.")
        return
    
    os.system("systemctl stop ferocious")
    print("⚠️ Ferocious service stopped. Run 'sudo systemctl start ferocious' to resume.")

def start_service():
    """Start Ferocious service — no password needed"""
    os.system("systemctl start ferocious")
    print("✅ Ferocious service started.")

def add_to_blocklist():
    """Add a website, keyword, or subreddit to config.json"""
    if not verify_password():
        print("❌ Incorrect password. Action Aborted")
        return
    
    entry = input("Enter domain, keyword, or subreddit to block: ").strip().lower()
    if not entry:
        print("❌ Nothing entered.")
        return
    
    # Unlock config
    os.system(f"chattr -i {CONFIG_FILE} 2>/dev/null")
    
    # Load existing config
    with open(CONFIG_FILE, 'r') as f:
        config = json.load(f)
    
    # Determine where to put it
    if 'reddit.com/r/' in entry:
        if entry not in config['blocked_subreddits']:
            config['blocked_subreddits'].append(entry)
            print(f"✅ Subreddit '{entry}' added.")
    elif '.' in entry and ' ' not in entry and not entry.startswith('http'):
        if entry not in config['blocked_domains']:
            config['blocked_domains'].append(entry)
            print(f"✅ Domain '{entry}' added.")
    else:
        if entry not in config['blocked_keywords']:
            config['blocked_keywords'].append(entry)
            print(f"✅ Keyword '{entry}' added.")
    
    # Save and lock
    with open(CONFIG_FILE, 'w') as f:
        json.dump(config, f, indent=2)
    os.system(f"chattr +i {CONFIG_FILE}")

def import_blocklist():
    """Import a plain text blocklist.txt file (one entry per line)"""
    if not verify_password():
        print("❌ Incorrect password. Action Aborted")
        return
    
    # Open file picker dialog
    try:
        import tkinter as tk
        from tkinter import filedialog
        root = tk.Tk()
        root.withdraw()  # Hide the main window
        file_path = filedialog.askopenfilename(
            title="Select blocklist.txt file",
            filetypes=[("Text files", "*.txt"), ("All files", "*.*")]
        )
        root.destroy()
    except:
        # Fallback if tkinter not available
        file_path = input("Enter path to blocklist.txt file: ").strip()
    
    if not file_path:
        print("❌ No file selected.")
        return
    
    if not os.path.exists(file_path):
        print(f"❌ File not found: {file_path}")
        return
    
    # Unlock config
    os.system(f"chattr -i {CONFIG_FILE} 2>/dev/null")
    
    # Load existing config
    with open(CONFIG_FILE, 'r') as f:
        config = json.load(f)
    
    # Read and parse the plain text file
    added_counts = {"domains": 0, "keywords": 0, "subreddits": 0}
    
    with open(file_path, 'r') as f:
        for line in f:
            line = line.strip().lower()
            if not line or line.startswith('#'):
                continue
            
            # Determine type and add
            if 'reddit.com/r/' in line:
                if line not in config['blocked_subreddits']:
                    config['blocked_subreddits'].append(line)
                    added_counts['subreddits'] += 1
            elif '.' in line and ' ' not in line and not line.startswith('http'):
                if line not in config['blocked_domains']:
                    config['blocked_domains'].append(line)
                    added_counts['domains'] += 1
            else:
                if line not in config['blocked_keywords']:
                    config['blocked_keywords'].append(line)
                    added_counts['keywords'] += 1
    
    # Save and lock
    with open(CONFIG_FILE, 'w') as f:
        json.dump(config, f, indent=2)
    os.system(f"chattr +i {CONFIG_FILE}")
    
    print(f"\n✅ Import complete!")
    print(f"   Domains added: {added_counts['domains']}")
    print(f"   Keywords added: {added_counts['keywords']}")
    print(f"   Subreddits added: {added_counts['subreddits']}")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage:")
        print("  sudo ferocious.py --setup       # First time setup")
        print("  sudo ferocious.py --status      # Check status")
        print("  sudo ferocious.py --uninstall   # Remove Ferocious")
        print("  sudo ferocious.py --edit        # Edit config.json")
        print("  sudo ferocious.py --add         # Add single entry")
        print("  sudo ferocious.py --import      # Import blocklist.txt file")
        print("  sudo ferocious.py --stop        # Stop service")
        print("  sudo ferocious.py --start       # Start service")
    elif "--uninstall" in sys.argv:
        uninstall()
    elif "--status" in sys.argv:
        status()
    elif "--setup" in sys.argv:
        setup()
    elif "--edit" in sys.argv:
        edit_config()
    elif "--add" in sys.argv:
        add_to_blocklist()
    elif "--import" in sys.argv:
        import_blocklist()
    elif "--stop" in sys.argv:
        stop_service()
    elif "--start" in sys.argv:
        start_service()
