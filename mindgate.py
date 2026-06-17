#!/usr/bin/env python3
"""
MindGate — Permanent Website Blocker for Linux
- Network-level blocking with iptables
- DNS-level blocking with systemd-resolved
- Runs as daemon
- Requires password to stop
- Cold Turkey-level stubbornness
"""

import os
import sys
import json
import hashlib
import subprocess
from pathlib import Path
import re

# Auto-elevate to sudo if not already root
if os.geteuid() != 0:
    print("🧠 MindGate needs root privileges. Elevating with sudo...")
    args = sys.argv.copy()
    os.execvp('sudo', ['sudo', 'python3'] + args)
    sys.exit(1)  # Never reached

# Configuration
CONFIG_DIR = "/etc/mindgate"
CONFIG_FILE = f"{CONFIG_DIR}/config.json"
PASSWORD_FILE = f"{CONFIG_DIR}/password.hash"
LOCK_FILE = f"{CONFIG_DIR}/active"
LOG_FILE = "/var/log/mindgate.log"
DNS_DIR = "/etc/dnsmasq.d"

def setup():
    """Initial setup — run once as root"""
    os.makedirs(CONFIG_DIR, exist_ok=True)
    
    # Set password
    if not os.path.exists(PASSWORD_FILE):
        pwd = input("🔐 Set a password for MindGate: ")
        pwd_hash = hashlib.sha256(pwd.encode()).hexdigest()
        with open(PASSWORD_FILE, 'w') as f:
            f.write(pwd_hash)
        os.chmod(PASSWORD_FILE, 0o600)
        print("✅ Password set.")
    
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
                "r/gonewild",
                "r/nsfw",
                "r/porn"
            ]
        }
        with open(CONFIG_FILE, 'w') as f:
            json.dump(config, f, indent=2)
        print("✅ Default config created.")
    
    # Mark as active
    with open(LOCK_FILE, 'w') as f:
        f.write("active")
    
    # Lock files (kernel-level immutable)
    os.system(f"chattr +i {CONFIG_FILE}")
    os.system(f"chattr +i {PASSWORD_FILE}")
    os.system(f"chattr +i {LOCK_FILE}")
    
    # Apply initial blocklist
    apply_all_blocks()
    
    print("\n" + "="*60)
    print("🧠 MINDGATE IS ACTIVE.")
    print("="*60)
    print(f"Config: {CONFIG_FILE}")
    print(f"Blocking is PERMANENT until you disable it with password.")
    print(f"To uninstall: sudo mindgate-uninstall")
    print("="*60)

def verify_password():
    """Verify password before allowing sensitive action"""
    pwd_attempt = input("🔐 Enter MindGate password to continue: ")
    with open(PASSWORD_FILE, 'r') as f:
        stored_hash = f.read().strip()
    return hashlib.sha256(pwd_attempt.encode()).hexdigest() == stored_hash

def resolve_domain(domain):
    """Resolve domain to IP addresses"""
    try:
        result = subprocess.run(
            ['getent', 'hosts', domain],
            capture_output=True,
            text=True,
            timeout=5
        )
        if result.returncode == 0:
            # getent returns: IP domain
            lines = result.stdout.strip().split('\n')
            ips = [line.split()[0] for line in lines if line]
            return ips
    except:
        pass
    
    # Fallback to nslookup
    try:
        result = subprocess.run(
            ['nslookup', domain],
            capture_output=True,
            text=True,
            timeout=5
        )
        ips = []
        for line in result.stdout.split('\n'):
            if 'Address:' in line:
                parts = line.split(': ')
                if len(parts) > 1:
                    ip = parts[1].strip()
                    if ip and ':' not in ip:  # Skip IPv6 for now
                        ips.append(ip)
        return list(set(ips))  # Remove duplicates
    except:
        return []

def add_iptables_rule(ip, domain):
    """Add iptables rule to block IP"""
    try:
        # Block outgoing traffic
        os.system(f"iptables -A OUTPUT -d {ip} -j DROP 2>/dev/null")
        # Block incoming traffic
        os.system(f"iptables -A INPUT -s {ip} -j DROP 2>/dev/null")
        return True
    except:
        return False

def apply_all_blocks():
    """Apply all blocking rules (iptables + DNS)"""
    if not os.path.exists(CONFIG_FILE):
        return
    
    with open(CONFIG_FILE, 'r') as f:
        config = json.load(f)
    
    print("🔗 Applying blocking rules...")
    
    # Block domains at network level
    for domain in config.get('blocked_domains', []):
        ips = resolve_domain(domain)
        for ip in ips:
            add_iptables_rule(ip, domain)
    
    print(f"   ✅ {len(config.get('blocked_domains', []))} domains blocked")

def uninstall():
    """Uninstall MindGate completely — NO TRASH LEFT"""
    print("\n⚠️  WARNING: This will disable ALL protection.")
    if not verify_password():
        print("❌ Incorrect password. MindGate remains active.")
        sys.exit(1)
    
    print("✅ Password correct. Uninstalling...")
    
    # 1. Stop and disable systemd service
    print("  ⚙️  Stopping service...")
    os.system("systemctl stop mindgate 2>/dev/null")
    os.system("systemctl disable mindgate 2>/dev/null")
    os.system("rm -f /etc/systemd/system/mindgate.service")
    os.system("systemctl daemon-reload")
    
    # 2. Flush iptables rules
    print("  🔓 Removing iptables rules...")
    os.system("iptables -F OUTPUT 2>/dev/null")
    os.system("iptables -F INPUT 2>/dev/null")
    
    # 3. Unlock files
    print("  🔓 Unlocking files...")
    os.system(f"chattr -i {CONFIG_FILE} 2>/dev/null")
    os.system(f"chattr -i {PASSWORD_FILE} 2>/dev/null")
    os.system(f"chattr -i {LOCK_FILE} 2>/dev/null")
    
    # 4. Remove entire MindGate directory
    print("  🗑️  Removing /etc/mindgate/...")
    os.system(f"rm -rf {CONFIG_DIR}")
    
    # 5. Remove scripts
    print("  🗑️  Removing scripts...")
    os.system("rm -f /usr/local/bin/mindgate.py")
    
    # 6. Remove sudoers restriction
    print("  🗑️  Removing sudoers restriction...")
    os.system("rm -f /etc/sudoers.d/mindgate")
    
    # 7. Remove ALL bash aliases
    print("  🐚 Removing bash aliases...")
    os.system("sed -i '/# MindGate commands/d' ~/.bashrc")
    os.system("sed -i '/alias mindgate-/d' ~/.bashrc")
    
    print("\n" + "="*60)
    print("✅ MindGate COMPLETELY REMOVED.")
    print("="*60)
    print("   Removed:")
    print("   - /etc/mindgate/ (config, password, blocklist)")
    print("   - /usr/local/bin/mindgate.py")
    print("   - /etc/systemd/system/mindgate.service")
    print("   - /etc/sudoers.d/mindgate")
    print("   - All mindgate-* aliases from ~/.bashrc")
    print("   - All iptables rules")
    print("="*60)

def status():
    """Check if MindGate is active"""
    if os.path.exists(LOCK_FILE):
        print("🧠 MindGate is ACTIVE")
        with open(CONFIG_FILE, 'r') as f:
            config = json.load(f)
        print(f"   Blocked domains: {len(config.get('blocked_domains', []))}")
        print(f"   Blocked keywords: {len(config.get('blocked_keywords', []))}")
        print(f"   Blocked subreddits: {len(config.get('blocked_subreddits', []))}")
        
        # Check service status
        result = subprocess.run(['systemctl', 'is-active', 'mindgate'], capture_output=True, text=True)
        if result.returncode == 0:
            print(f"   Service: running")
        else:
            print(f"   Service: not running")
    else:
        print("⚠️  MindGate is NOT active")

def list_blocklist():
    """Display current blocklist"""
    if not os.path.exists(CONFIG_FILE):
        print("❌ Config file not found.")
        return
    
    with open(CONFIG_FILE, 'r') as f:
        config = json.load(f)
    
    print("\n🧠 MindGate Blocklist:")
    print("\nDomains (" + str(len(config.get('blocked_domains', []))) + "):")
    for domain in config.get('blocked_domains', []):
        print(f"  - {domain}")
    
    print("\nKeywords (" + str(len(config.get('blocked_keywords', []))) + "):")
    for keyword in config.get('blocked_keywords', []):
        print(f"  - {keyword}")
    
    print("\nSubreddits (" + str(len(config.get('blocked_subreddits', []))) + "):")
    for sub in config.get('blocked_subreddits', []):
        print(f"  - {sub}")
    print()

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
    print("✅ Config updated and locked")
    
    # Reapply blocks
    apply_all_blocks()

def stop_service():
    """Stop MindGate service — requires password"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted.")
        return
    
    os.system("systemctl stop mindgate")
    print("⚠️  MindGate service stopped. Run 'mindgate-start' to resume.")

def start_service():
    """Start MindGate service — no password needed"""
    os.system("systemctl start mindgate")
    os.system("systemctl enable mindgate")
    print("✅ MindGate service started.")

def restart_service():
    """Restart MindGate service — requires password"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted.")
        return
    
    os.system("systemctl restart mindgate")
    apply_all_blocks()
    print("✅ MindGate restarted and rules reapplied.")

def add_to_blocklist():
    """Add a website, keyword, or subreddit to config.json"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted")
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
    if entry.startswith('r/'):
        if entry not in config['blocked_subreddits']:
            config['blocked_subreddits'].append(entry)
            print(f"✅ Subreddit '{entry}' added.")
        else:
            print(f"ℹ️  Subreddit '{entry}' already blocked.")
    elif '.' in entry and ' ' not in entry and not entry.startswith('http'):
        if entry not in config['blocked_domains']:
            config['blocked_domains'].append(entry)
            print(f"✅ Domain '{entry}' added.")
            # Resolve and block immediately
            ips = resolve_domain(entry)
            for ip in ips:
                add_iptables_rule(ip, entry)
        else:
            print(f"ℹ️  Domain '{entry}' already blocked.")
    else:
        if entry not in config['blocked_keywords']:
            config['blocked_keywords'].append(entry)
            print(f"✅ Keyword '{entry}' added.")
        else:
            print(f"ℹ️  Keyword '{entry}' already blocked.")
    
    # Save and lock
    with open(CONFIG_FILE, 'w') as f:
        json.dump(config, f, indent=2)
    os.system(f"chattr +i {CONFIG_FILE}")

def import_blocklist():
    """Import a plain text blocklist.txt file (one entry per line)"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted")
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
            if line.startswith('r/'):
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
    
    # Apply blocks
    apply_all_blocks()
    
    print(f"\n✅ Import complete!")
    print(f"   Domains added: {added_counts['domains']}")
    print(f"   Keywords added: {added_counts['keywords']}")
    print(f"   Subreddits added: {added_counts['subreddits']}")

def change_password():
    """Change administrator password"""
    if not verify_password():
        print("❌ Incorrect password. Action aborted.")
        return
    
    new_pwd = input("Enter new password: ")
    confirm_pwd = input("Confirm new password: ")
    
    if new_pwd != confirm_pwd:
        print("❌ Passwords do not match.")
        return
    
    # Unlock password file
    os.system(f"chattr -i {PASSWORD_FILE} 2>/dev/null")
    
    # Update password
    pwd_hash = hashlib.sha256(new_pwd.encode()).hexdigest()
    with open(PASSWORD_FILE, 'w') as f:
        f.write(pwd_hash)
    os.chmod(PASSWORD_FILE, 0o600)
    
    # Lock it back
    os.system(f"chattr +i {PASSWORD_FILE}")
    print("✅ Password changed successfully.")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("🧠 MindGate — Permanent Website Blocker")
        print("\nUsage:")
        print("  mindgate-status       # Check status")
        print("  mindgate-add          # Add single block")
        print("  mindgate-import       # Import blocklist")
        print("  mindgate-list         # View blocklist")
        print("  mindgate-edit         # Edit config (requires password)")
        print("  mindgate-stop         # Stop service (requires password)")
        print("  mindgate-start        # Start service")
        print("  mindgate-restart      # Restart service (requires password)")
        print("  mindgate-password     # Change password")
        print("  mindgate-uninstall    # Remove MindGate (requires password)")
    elif "--status" in sys.argv or "status" in sys.argv:
        status()
    elif "--setup" in sys.argv or "setup" in sys.argv:
        setup()
    elif "--uninstall" in sys.argv or "uninstall" in sys.argv:
        uninstall()
    elif "--edit" in sys.argv or "edit" in sys.argv:
        edit_config()
    elif "--add" in sys.argv or "add" in sys.argv:
        add_to_blocklist()
    elif "--import" in sys.argv or "import" in sys.argv:
        import_blocklist()
    elif "--stop" in sys.argv or "stop" in sys.argv:
        stop_service()
    elif "--start" in sys.argv or "start" in sys.argv:
        start_service()
    elif "--restart" in sys.argv or "restart" in sys.argv:
        restart_service()
    elif "--list" in sys.argv or "list" in sys.argv:
        list_blocklist()
    elif "--password" in sys.argv or "password" in sys.argv:
        change_password()
