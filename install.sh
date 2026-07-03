#!/bin/bash
# MindGate v2 Installer — Enterprise-Grade Website Blocker (2026)
# Universal Linux support with production-grade setup

set -e

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

log()  { echo -e "${GREEN}✅${NC} $1"; }
warn() { echo -e "${YELLOW}⚠️${NC}  $1"; }
err()  { echo -e "${RED}❌${NC} $1"; exit 1; }
info() { echo -e "${CYAN}ℹ️${NC}  $1"; }

echo ""
echo -e "${BOLD}🧠 MindGate v2 Installer${NC} (Enterprise-Grade)"
echo "========================================"

[[ $EUID -ne 0 ]] && err "Run as root: sudo bash install.sh"

REAL_USER="${SUDO_USER:-root}"
REAL_HOME=$(getent passwd "$REAL_USER" 2>/dev/null | cut -d: -f6 || echo "/root")

# ── Detect environment ────────────────────────────────────────────────────────

detect_pkg_manager() {
    if   command -v apt-get &>/dev/null; then echo "apt"
    elif command -v dnf     &>/dev/null; then echo "dnf"
    elif command -v yum     &>/dev/null; then echo "yum"
    elif command -v pacman  &>/dev/null; then echo "pacman"
    elif command -v zypper  &>/dev/null; then echo "zypper"
    elif command -v apk     &>/dev/null; then echo "apk"
    else echo "unknown"
    fi
}

detect_init() {
    if command -v systemctl &>/dev/null && systemctl --version &>/dev/null 2>&1; then
        echo "systemd"
    elif command -v rc-service &>/dev/null; then
        echo "openrc"
    else
        echo "sysv"
    fi
}

detect_firewall() {
    if command -v nft &>/dev/null; then
        echo "nftables"
    elif command -v iptables &>/dev/null; then
        echo "iptables"
    else
        echo "none"
    fi
}

PKG_MGR=$(detect_pkg_manager)
INIT_SYS=$(detect_init)
FIREWALL=$(detect_firewall)

info "Package manager: $PKG_MGR"
info "Init system: $INIT_SYS"
info "Firewall: $FIREWALL"

# ── Install dependencies ──────────────────────────────────────────────────────

install_pkg() {
    local pkg="$1"
    case "$PKG_MGR" in
        apt)    apt-get install -y -qq "$pkg" 2>/dev/null ;;
        dnf)    dnf install -y -q "$pkg" 2>/dev/null ;;
        yum)    yum install -y -q "$pkg" 2>/dev/null ;;
        pacman) pacman -Sy --noconfirm --quiet "$pkg" 2>/dev/null ;;
        zypper) zypper install -y -q "$pkg" 2>/dev/null ;;
        apk)    apk add -q "$pkg" 2>/dev/null ;;
    esac
}

echo ""
echo "📦 Installing dependencies..."

if ! command -v python3 &>/dev/null; then
    install_pkg python3
fi
python3 --version &>/dev/null || err "python3 failed"

if [[ "$FIREWALL" == "none" ]]; then
    install_pkg iptables
fi

# chattr (graceful)
CHATTR_OK="false"
if ! command -v chattr &>/dev/null; then
    install_pkg e2fsprogs 2>/dev/null || true
fi

if command -v chattr &>/dev/null; then
    TEST_FILE=$(mktemp)
    if chattr +i "$TEST_FILE" 2>/dev/null; then
        chattr -i "$TEST_FILE" 2>/dev/null
        CHATTR_OK="true"
    fi
    rm -f "$TEST_FILE"
fi

[[ "$CHATTR_OK" == "true" ]] && log "chattr supported" || warn "chattr not supported (using chmod)"

log "Dependencies installed"

# ── Install MindGate ──────────────────────────────────────────────────────────

echo ""
echo "📂 Installing MindGate v2..."

mkdir -p /etc/mindgate /var/log
cp mindgate.py /usr/local/bin/mindgate.py
chmod 755 /usr/local/bin/mindgate.py

# Wrapper
cat > /usr/local/bin/mindgate << 'WRAPPER'
#!/bin/bash
exec python3 /usr/local/bin/mindgate.py "$@"
WRAPPER
chmod 755 /usr/local/bin/mindgate

# Commands
for cmd in setup status add remove list start stop health stats logs uninstall; do
    cat > /usr/local/bin/mindgate-$cmd << CMDEOF
#!/bin/bash
exec python3 /usr/local/bin/mindgate.py $cmd "\$@"
CMDEOF
    chmod 755 /usr/local/bin/mindgate-$cmd
done

log "MindGate installed to /usr/local/bin"

# ── Environment config ────────────────────────────────────────────────────────

cat > /etc/mindgate/env << EOF
CHATTR_OK=$CHATTR_OK
FIREWALL=$FIREWALL
INIT_SYS=$INIT_SYS
EOF

# ── Setup ─────────────────────────────────────────────────────────────────────

echo ""
echo "🔐 Running setup..."
python3 /usr/local/bin/mindgate.py setup

# ── Install service ───────────────────────────────────────────────────────────

echo ""
echo "⚙️  Installing service ($INIT_SYS)..."

case "$INIT_SYS" in
    systemd)
        cp mindgate.service /etc/systemd/system/
        chmod 644 /etc/systemd/system/mindgate.service
        systemctl daemon-reload
        systemctl enable mindgate 2>/dev/null || true
        systemctl start mindgate 2>/dev/null || true
        log "systemd service installed"
        ;;
    openrc)
        cat > /etc/init.d/mindgate << 'OPENRC'
#!/sbin/openrc-run
description="MindGate Website Blocker"
command="/usr/bin/python3"
command_args="/usr/local/bin/mindgate.py start"
depend() { need net; }
start() {
    ebegin "Starting MindGate"
    $command $command_args
    eend $?
}
OPENRC
        chmod +x /etc/init.d/mindgate
        rc-update add mindgate default 2>/dev/null || true
        rc-service mindgate start 2>/dev/null || true
        log "OpenRC service installed"
        ;;
    *)
        warn "Unknown init system — add to /etc/rc.local manually"
        ;;
esac

# ── Sudoers restrictions ──────────────────────────────────────────────────────

echo ""
echo "🔒 Adding sudoers restrictions..."

cat > /etc/sudoers.d/mindgate << 'SUDOERS'
ALL ALL=(ALL) ALL, !/usr/bin/chattr, !/bin/chattr, !/usr/bin/chmod, !/bin/chmod, !/bin/systemctl, !/usr/bin/systemctl
SUDOERS
chmod 440 /etc/sudoers.d/mindgate
log "sudoers restrictions added"

# ── Shell aliases ─────────────────────────────────────────────────────────────

echo ""
echo "🐚 Adding shell aliases..."

ALIASES='
# MindGate commands
alias mindgate-setup="sudo mindgate setup"
alias mindgate-status="sudo mindgate status"
alias mindgate-add="sudo mindgate add"
alias mindgate-remove="sudo mindgate remove"
alias mindgate-list="sudo mindgate list"
alias mindgate-start="sudo mindgate start"
alias mindgate-stop="sudo mindgate stop"
alias mindgate-health="sudo mindgate health"
alias mindgate-stats="sudo mindgate stats"
alias mindgate-logs="sudo mindgate logs"
alias mindgate-uninstall="sudo mindgate uninstall"
'

for rc in "$REAL_HOME/.bashrc" "$REAL_HOME/.zshrc" "/root/.bashrc" "/etc/bash.bashrc"; do
    [[ -f "$rc" ]] || continue
    if ! grep -q "mindgate-setup" "$rc" 2>/dev/null; then
        echo "$ALIASES" >> "$rc"
        info "Aliases → $rc"
    fi
done

log "Aliases added"

# ── Final checks ──────────────────────────────────────────────────────────────

echo ""
echo "🔍 Verifying installation..."

rc=0
[[ -f /etc/mindgate/config.json ]] && log "Config: OK" || (warn "Config: pending" && rc=1)
[[ -f /etc/mindgate/password.hash ]] && log "Password: OK" || (warn "Password: pending" && rc=1)
[[ -x /usr/local/bin/mindgate.py ]] && log "Binary: OK" || (warn "Binary: FAILED" && rc=1)

if [[ "$INIT_SYS" == "systemd" ]]; then
    systemctl is-active mindgate &>/dev/null && log "Service: running" || log "Service: will start on reboot"
fi

# ── Done ──────────────────────────────────────────────────────────────────────

echo ""
echo "========================================"
echo -e "${BOLD}${GREEN}🧠 MINDGATE V2 INSTALLED!${NC}"
echo "========================================"
echo ""
echo -e "${BOLD}Quick Start:${NC}"
echo "  mindgate-status      Check blocking status"
echo "  mindgate-add         Block a website"
echo "  mindgate-list        View all blocks"
echo "  mindgate-health      Run health check"
echo ""
echo -e "${BOLD}Features:${NC}"
echo "  ✅ 3-layer blocking (hosts + firewall + DNS)"
echo "  ✅ Logging system (/var/log/mindgate.log)"
echo "  ✅ Health checks and auto-recovery"
echo "  ✅ Atomic operations (safe anytime)"
echo "  ✅ Colorized output"
echo "  ✅ Dry-run mode (--dry-run)"
echo "  ✅ Statistics tracking"
echo "  ✅ Backup/restore"
echo ""
echo -e "${BOLD}Password-Protected:${NC}"
echo "  • mindgate-add"
echo "  • mindgate-remove"
echo "  • mindgate-stop"
echo "  • mindgate-uninstall"
echo ""
echo "========================================"
echo ""

exit $rc
