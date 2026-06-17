#!/bin/bash
# MindGate — One-Command Installer
# Run: chmod +x install.sh && sudo ./install.sh

set -e  # Stop on error

echo "🧠 MindGate Installer"
echo "======================================="

# Check if running as root
if [[ $EUID -ne 0 ]]; then
   echo "❌ This script must be run as root (use sudo)"
   exit 1
fi

# 1. Install dependencies
echo "📦 Installing dependencies..."
apt update -qq
apt install -y python3 python3-pip iptables

# 2. Copy files to system
echo "📂 Installing MindGate files..."
cp mindgate.py /usr/local/bin/
chmod +x /usr/local/bin/mindgate.py

# 3. Run Python setup (sets password, creates config)
echo "🔐 Setting up MindGate..."
python3 /usr/local/bin/mindgate.py --setup

# 4. Install systemd service
echo "⚙️  Installing systemd service..."
cp mindgate.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable mindgate
systemctl start mindgate

# 5. Lock files (make immutable)
echo "🔒 Locking MindGate files..."
chattr +i /etc/mindgate/config.json 2>/dev/null || echo "  (skipping lock on config)"
chattr +i /etc/mindgate/password.hash 2>/dev/null || echo "  (skipping lock on password)"

# 6. Add bash aliases for current user
echo "🐚 Adding MindGate commands to ~/.bashrc..."
ALIAS_LINES=(
    "# MindGate commands"
    "alias mindgate-status='sudo python3 /usr/local/bin/mindgate.py --status'"
    "alias mindgate-add='sudo python3 /usr/local/bin/mindgate.py --add'"
    "alias mindgate-import='sudo python3 /usr/local/bin/mindgate.py --import'"
    "alias mindgate-list='sudo python3 /usr/local/bin/mindgate.py --list'"
    "alias mindgate-edit='sudo python3 /usr/local/bin/mindgate.py --edit'"
    "alias mindgate-stop='sudo python3 /usr/local/bin/mindgate.py --stop'"
    "alias mindgate-start='sudo python3 /usr/local/bin/mindgate.py --start'"
    "alias mindgate-restart='sudo python3 /usr/local/bin/mindgate.py --restart'"
    "alias mindgate-password='sudo python3 /usr/local/bin/mindgate.py --password'"
    "alias mindgate-uninstall='sudo python3 /usr/local/bin/mindgate.py --uninstall'"
)

# Check if aliases already exist
if ! grep -q "MindGate commands" ~/.bashrc; then
    for line in "${ALIAS_LINES[@]}"; do
        echo "$line" >> ~/.bashrc
    done
    echo "✅ Aliases added to ~/.bashrc"
else
    echo "ℹ️  Aliases already exist in ~/.bashrc"
fi

# 7. Add sudoers restrictions
echo "🔒 Adding sudoers restrictions..."
echo "ALL ALL=(ALL) ALL, !/usr/bin/chattr, !/usr/bin/chmod, !/bin/systemctl" | sudo tee /etc/sudoers.d/mindgate
sudo chmod 440 /etc/sudoers.d/mindgate

echo "✅ Now you cannot:"
echo "   - chattr -i (unlock files)"
echo "   - chmod (change permissions)"
echo "   - systemctl (stop service directly)"

# 8. Persist iptables rules
echo "💾 Persisting iptables rules..."
if ! command -v iptables-save &> /dev/null; then
    apt install -y iptables-persistent
fi
iptables-save > /etc/iptables/rules.v4

# 9. Verify it's running
echo ""
echo "✅ MindGate is ACTIVE!"
echo "======================================="
echo "   Status: $(systemctl is-active mindgate)"
echo "   Blocking: Network-level (iptables)"
echo ""
echo "📌 Commands (after 'source ~/.bashrc' or new terminal):"
echo "   mindgate-status      - Check if blocking"
echo "   mindgate-add         - Add single domain/keyword/subreddit"
echo "   mindgate-import      - Import blocklist.txt file"
echo "   mindgate-list        - View blocklist"
echo "   mindgate-edit        - Edit config (requires password)"
echo "   mindgate-stop        - Stop service (requires password)"
echo "   mindgate-start       - Start service"
echo "   mindgate-restart     - Restart service (requires password)"
echo "   mindgate-password    - Change password"
echo "   mindgate-uninstall   - Remove MindGate (requires password)"
echo ""
echo "📌 To reload aliases now: source ~/.bashrc"
echo "======================================="
