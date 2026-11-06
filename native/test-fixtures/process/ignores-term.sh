#!/bin/sh
# Ignores SIGTERM so the TERM -> grace -> KILL escalation is exercised.
trap '' TERM
echo "[fixture] ignoring TERM"
while :; do sleep 0.1; done
