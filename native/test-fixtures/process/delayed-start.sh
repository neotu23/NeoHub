#!/bin/sh
# Prints nothing for 1s, then announces itself — proves readiness/start races.
sleep 1
echo "[fixture] started after delay"
i=0
while :; do
  i=$((i + 1))
  echo "[fixture] tick $i"
  sleep 0.1
done
