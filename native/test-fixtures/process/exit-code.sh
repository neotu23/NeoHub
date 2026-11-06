#!/bin/sh
# Exits with the code given as $1 (default 3) after one line of output.
echo "[fixture] exiting with ${1:-3}"
exit "${1:-3}"
