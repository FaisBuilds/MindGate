#!/bin/bash
# Ferocious — One-Command Installer
# Run: chmod +x install.sh && sudo ./install.sh

set -e  # Stop on error

echo "🛡️  Ferocious Installer"
echo "========================"

# 1. Install dependencies
echo "📦 Installing mitmproxy..."
apt update -qq
apt install -y mitmproxy python3 python3-pip

# 2. Copy files to system
echo "📂 Installing Ferocious files..."
cp ferocious.py /usr/local/bin/
cp ferocious-proxy.py /usr/local/bin/
chmod +x /usr/local/bin/ferocious.py
chmod +x /usr/local/bin/ferocious-proxy.py

# 3. Run Python setup (sets password, creates config)
echo "🔐 Setting up Ferocious..."
python3 /usr/local/bin/ferocious.py --setup

# 4. Install systemd service
echo "⚙️ Installing systemd service..."
cp ferocious.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable ferocious
systemctl start ferocious

# 5. Configure system proxy
echo "🌐 Configuring system proxy..."
gsettings set org.gnome.system.proxy mode 'manual'
gsettings set org.gnome.system.proxy.http host '127.0.0.1'
gsettings set org.gnome.system.proxy.http port 8080
gsettings set org.gnome.system.proxy.https host '127.0.0.1'
gsettings set org.gnome.system.proxy.https port 8080
gsettings set org.gnome.system.proxy lock true 2>/dev/null || echo "  (proxy lock not supported on this system)"

# 6. Lock files (make immutable)
echo "🔒 Locking Ferocious files..."
chattr +i /etc/ferocious/config.json 2>/dev/null || echo "  (skipping lock on config)"
chattr +i /etc/ferocious/password.hash 2>/dev/null || echo "  (skipping lock on password)"

# 7. Add bash aliases for current user
echo "🐚 Adding Ferocious commands to ~/.bashrc..."
ALIAS_LINES=(
    "# Ferocious commands"
    "alias ferocious-status='sudo python3 /usr/local/bin/ferocious.py --status'"
    "alias ferocious-uninstall='sudo python3 /usr/local/bin/ferocious.py --uninstall'"
    "alias ferocious-edit='sudo python3 /usr/local/bin/ferocious.py --edit'"
    "alias ferocious-stop='sudo python3 /usr/local/bin/ferocious.py --stop'"
    "alias ferocious-start='sudo python3 /usr/local/bin/ferocious.py --start'"
    "alias ferocious-add='sudo python3 /usr/local/bin/ferocious.py --add'"
    "alias ferocious-import='sudo python3 /usr/local/bin/ferocious.py --import'"
)

# Check if aliases already exist
if ! grep -q "Ferocious commands" ~/.bashrc; then
    for line in "${ALIAS_LINES[@]}"; do
        echo "$line" >> ~/.bashrc
    done
    echo "✅ Aliases added to ~/.bashrc"
else
    echo "ℹ️  Aliases already exist in ~/.bashrc"
fi

# 8. Add sudoers restrictions
echo "🔒 Adding sudoers restrictions..."
echo "faisal ALL=(ALL) ALL, !/usr/bin/chattr, !/usr/bin/chmod, !/bin/systemctl, !/usr/bin/gsettings" | sudo tee /etc/sudoers.d/ferocious
sudo chmod 440 /etc/sudoers.d/ferocious

echo "✅ Now you cannot:"
echo "   - chattr -i (unlock files)"
echo "   - systemctl stop ferocious"
echo "   - gsettings (change proxy)"

# 9. Verify it's running
echo ""
echo "✅ Ferocious is ACTIVE!"
echo "========================"
echo "   Status: $(systemctl is-active ferocious)"
echo "   Proxy: localhost:8080"
echo ""
echo "📌 Commands (after 'source ~/.bashrc' or new terminal):"
echo "   ferocious-status      - Check if blocking"
echo "   ferocious-add         - Add single domain/keyword/subreddit"
echo "   ferocious-import      - Import blocklist.txt file"
echo "   ferocious-edit        - Edit config.json (requires password)"
echo "   ferocious-stop        - Stop service (requires password)"
echo "   ferocious-start       - Start service"
echo "   ferocious-uninstall   - Remove Ferocious (requires password)"
echo ""
echo "📌 To reload aliases now: source ~/.bashrc"
echo "========================"
