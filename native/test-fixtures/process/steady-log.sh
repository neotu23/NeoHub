#!/bin/sh
# Synthetic long-running process: fixed log lines with a stable prefix.
# Used to prove PTY streaming and clean shutdown. Never touches user projects.
i=0
while :; do
  i=$((i + 1))
  echo "[fixture] tick $i"
  sleep 0.1
done
