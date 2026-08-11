#!/bin/bash
#
# MindGate MVP1 Uninstaller
#
# Removes everything install.sh set up:
#   - systemd services (daemon & watchdog)
#   - Binaries from /usr/local/bin
#   - Configuration directory (/etc/mindgate)
#   - Native Messaging manifests from all browsers
#   - Bridge and watchdog scripts
#
# Per MVP1 spec: "Leave browsers untouched."
# The user must manually remove the extension from chrome://extensions.

set -euo pipefail

if [[ ${EUID} -ne 0 ]]; then
  echo "✗ Error: Must be run with sudo: sudo ./uninstall.sh" >&2
  exit 1
fi

INSTALL_BIN_DIR="/usr/local/bin"
CONFIG_DIR="/etc/mindgate"

echo "=========================================================="
echo " MindGate MVP1 Uninstaller"
echo "=========================================================="
echo

# 1. Stop and disable systemd services
echo "[1/5] Stopping and disabling systemd services..."
systemctl stop mindgated.service 2>/dev/null || true
systemctl stop mindgate-watchdog.service 2>/dev/null || true
systemctl disable mindgated.service 2>/dev/null || true
systemctl disable mindgate-watchdog.service 2>/dev/null || true
systemctl daemon-reload
systemctl reset-failed
echo "  ✓ Services stopped and disabled."

# 2. Remove systemd unit files
echo "[2/5] Removing systemd unit files..."
rm -f /etc/systemd/system/mindgated.service
rm -f /etc/systemd/system/mindgate-watchdog.service
systemctl daemon-reload
echo "  ✓ Unit files removed."

# 3. Remove binaries and scripts
echo "[3/5] Removing binaries and scripts..."
rm -f "${INSTALL_BIN_DIR}/mindgated"
rm -f "${INSTALL_BIN_DIR}/mindgate"
rm -f "${INSTALL_BIN_DIR}/mindgate-bridge.sh"
rm -f "${INSTALL_BIN_DIR}/mindgate-watchdog.sh"
rm -f "${INSTALL_BIN_DIR}/mindgate-uninstall.sh"
echo "  ✓ Binaries and scripts removed."

# 4. Remove configuration directory
echo "[4/5] Removing configuration directory..."
rm -rf "${CONFIG_DIR}"
echo "  ✓ Configuration removed."

# 5. Remove Native Messaging manifests from all browsers
#
# FIX: install.sh writes manifests to GLOBAL, system-wide directories
# (/etc/opt/..., /etc/chromium/...), not per-user ~/.config/ paths.
# The previous version of this script looked in ~/.config/ and would
# therefore NEVER find or remove anything install.sh actually created
# — every uninstall silently left stale manifests behind pointing at
# a bridge script that no longer exists. This list must always match
# install.sh's GLOBAL_NMH_DIRS exactly, or this bug comes back.
echo "[5/5] Removing Native Messaging manifests..."

NMH_MANIFEST_NAME="com.mindgate.protector.json"

declare -a GLOBAL_NMH_DIRS=(
  "/etc/opt/chrome/native-messaging-hosts"
  "/etc/chromium/native-messaging-hosts"
  "/etc/opt/brave/native-messaging-hosts"
  "/etc/vivaldi/native-messaging-hosts"
  "/etc/opt/microsoft-edge/native-messaging-hosts"
)

REMOVED_COUNT=0

for dir in "${GLOBAL_NMH_DIRS[@]}"; do
  manifest_path="${dir}/${NMH_MANIFEST_NAME}"

  if [[ -f "${manifest_path}" ]]; then
    rm -f "${manifest_path}"
    echo "  ✓ Removed manifest from ${dir}"
    REMOVED_COUNT=$((REMOVED_COUNT + 1))
  fi
done

if [[ ${REMOVED_COUNT} -eq 0 ]]; then
  echo "  ℹ No Native Messaging manifests found."
fi

# Final message
echo
echo "=========================================================="
echo " ✓ MindGate has been completely uninstalled."
echo "=========================================================="
echo
echo "⚠️  Manual Step Required:"
echo "   Open your browser(s) and remove the MindGate extension:"
echo "   1. Go to chrome://extensions (or equivalent)"
echo "   2. Find 'MindGate' and click 'Remove'"
echo
echo "All daemon, watchdog, systemd services, binaries, and configuration"
echo "have been removed. Your browsers and extension data are untouched."