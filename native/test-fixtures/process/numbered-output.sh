#!/bin/sh
# 1000 numbered lines (to test subscription high-water marks), then heartbeats so the
# task stays live while new output keeps arriving.
i=1
while [ "$i" -le 1000 ]; do
  printf 'line %04d\n' "$i"
  i=$((i + 1))
done
while :; do
  echo "[fixture] heartbeat"
  sleep 0.2
done
