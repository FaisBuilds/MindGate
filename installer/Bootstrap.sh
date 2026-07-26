#!/bin/bash
#
# MindGate bootstrap — this is the script the landing page's curl
# command actually points at. Its only job is: get the repo onto this
# machine, then hand off to the real installer/install.sh.
#
# Usage (what goes on the landing page):
#   curl -fsSL https://raw.githubusercontent.com/FrenzyDev-git/MindGate/main/installer/Bootstrap.sh | bash
#
# Deliberately does NOT need sudo itself — it just clones/updates a repo
# in the invoking user's own home directory. install.sh is the one that
# re-invokes itself with sudo, once it actually needs root.

set -euo pipefail

REPO_URL="https://github.com/FrenzyDev-git/MindGate.git"
INSTALL_DIR="${HOME}/.mindgate-src"

echo "MindGate installer"
echo "==================="
echo

if ! command -v git >/dev/null 2>&1; then
  echo "-> git not found, installing..."
  if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update -qq && sudo apt-get install -y -qq git
  elif command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y -q git
  elif command -v pacman >/dev/null 2>&1; then
    sudo pacman -Sy --noconfirm --needed git
  elif command -v zypper >/dev/null 2>&1; then
    sudo zypper --non-interactive install git
  else
    echo "Could not find a supported package manager to install git." >&2
    echo "Please install git manually, then re-run this command." >&2
    exit 1
  fi
fi

if [[ -d "${INSTALL_DIR}/.git" ]]; then
  echo "-> Existing MindGate source found, updating..."
  git -C "${INSTALL_DIR}" pull --ff-only
else
  echo "-> Downloading MindGate..."
  git clone --depth 1 "${REPO_URL}" "${INSTALL_DIR}"
fi

echo
echo "-> Running installer (you may be asked for your password)..."
echo
sudo "${INSTALL_DIR}/installer/install.sh"