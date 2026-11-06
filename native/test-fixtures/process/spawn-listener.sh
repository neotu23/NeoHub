#!/bin/sh
# Starts listens-on-port.sh as a CHILD process, so the listening socket belongs to a
# grandchild of the task root — proves ancestry is followed, not just the root pid.
sh "$(dirname "$0")/listens-on-port.sh" "${1:-4324}" &
echo "[fixture] child $!"
while :; do sleep 0.2; done
