#!/bin/bash
# Companion to mindgated.service's own Restart=always.
#
# Restart=always on mindgated.service already covers a crash or
# `kill -9 <pid>` — systemd notices the process died and respawns it
# on its own, no help needed. What it does NOT cover is a deliberate
# `systemctl stop mindgated` (or `systemctl kill mindgated`) — that's
# an intentional stop request, so systemd honors it and does NOT
# restart the unit, by design. This script closes THAT specific gap:
# it doesn't touch the mindgated process directly at all, it just
# checks whether the *unit* is active and starts it back up if not.
# That means "stop mindgate" isn't a single command anymore — you'd
# also have to stop, disable, or mask this watchdog first, or it just
# turns mindgated back on within CHECK_INTERVAL seconds.
#
# mindgated.rs (self_watch.rs) does the mirror-image check on THIS
# unit, so killing either one alone gets undone by the other within
# about 15 seconds. Killing/masking both at once still works — this
# is friction, not a sandbox — but it's two independent, deliberate
# steps instead of one.
#
# Deliberately dumb rather than doing real IPC health-checking — the
# whole point is to be a second, independent point of failure, not a
# second opinion on daemon health. `mindgate status` already reports
# real health; this only asks systemd "is the unit active," which is
# a much smaller thing to also have to disable.
set -uo pipefail

CHECK_INTERVAL=15

while true; do
  if ! systemctl is-active --quiet mindgated.service; then
    logger -t mindgate-watchdog "mindgated is not active — restarting it"
    systemctl start mindgated.service || true
  fi
  sleep "${CHECK_INTERVAL}"
done