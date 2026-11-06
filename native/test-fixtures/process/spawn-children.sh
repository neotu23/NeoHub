#!/bin/sh
# Spawns two grandchildren in the same process group, then keeps running.
sh -c 'while :; do sleep 0.2; done' &
echo "[fixture] child $!"
sh -c 'while :; do sleep 0.2; done' &
echo "[fixture] child $!"
echo "[fixture] parent $$"
while :; do sleep 0.2; done
