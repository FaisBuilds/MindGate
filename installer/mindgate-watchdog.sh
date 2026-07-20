#!/bin/bash
#
# MindGate Watchdog Script
#
# This is the mirror image of self_watch.rs (which runs inside the daemon
# and restarts the watchdog if it dies). This script runs as a separate
# systemd unit and restarts the daemon if it crashes or is killed.
#
# Per MVP1 Stubbornness: "Daemon and watchdog monitor each other."
# Neither can restart itself if IT is the one that gets stopped — a
# deliberate `systemctl stop` is honored by systemd, not treated as a
# crash to recover from. Therefore, each watches the OTHER.
#
# This ensures that stopping MindGate requires deliberately taking down
# both independent units, adding friction to bypass attempts.

set -euo pipefail

DAEMON_UNIT="mindgated.service"
CHECK_INTERVAL=15

log() {
    logger -t mindgate-watchdog "$@"
}

log "Watchdog starting, monitoring ${DAEMON_UNIT} every ${CHECK_INTERVAL}s"

while true; do
    # Check if the daemon is active
    if ! systemctl is-active --quiet "${DAEMON_UNIT}"; then
        log "${DAEMON_UNIT} is not active — attempting to restart it"
        
        # Attempt restart
        if systemctl start "${DAEMON_UNIT}"; then
            log "Successfully restarted ${DAEMON_UNIT}"
        else
            log "Failed to restart ${DAEMON_UNIT}"
        fi
    fi
    
    # Sleep before next check
    sleep "${CHECK_INTERVAL}"
done