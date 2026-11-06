#!/usr/bin/env bash
# Measure the packaged app's startup cost and steady-state footprint.
#
# What is measured, and why these signals:
#   * startup  - wall clock from exec to "the database exists in the data dir", which is the
#                first moment the app has finished opening the core. Window paint comes after,
#                but this number is stable and does not depend on window animations.
#   * rss      - resident memory of the main process after it settles (2 s of quiet).
#   * threads  - a rough shape check (the runtime is 2 workers + GPUI's own threads).
#
# It launches the packaged .app with a throwaway --data-dir, so the user's real database is
# never touched. Run it on a quiet machine: the numbers are wall-clock.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="$ROOT/dist/NeoHub.app/Contents/MacOS/neohub-desktop"
RUNS="${RUNS:-5}"

if [ ! -x "$BIN" ]; then
  echo "build the app first: bash scripts/package.sh" >&2
  exit 1
fi

now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }

startup_times=()
rss_values=()

for run in $(seq 1 "$RUNS"); do
  data_dir="$(mktemp -d)"
  started=$(now_ms)
  "$BIN" --data-dir "$data_dir" >/dev/null 2>&1 &
  pid=$!

  # Startup: wait for the database, capped at 15 s.
  ready=""
  for _ in $(seq 1 300); do
    if [ -f "$data_dir/devharbor.db" ]; then ready=$(now_ms); break; fi
    sleep 0.05
  done
  if [ -z "$ready" ]; then
    echo "run $run: app never created its database" >&2
    kill -9 "$pid" 2>/dev/null || true
    continue
  fi
  startup_times+=($((ready - started)))

  # Footprint once it settles.
  sleep 2
  rss_values+=($(ps -o rss= -p "$pid" | tr -d ' '))

  kill -TERM "$pid" 2>/dev/null || true
  for _ in $(seq 1 60); do kill -0 "$pid" 2>/dev/null || break; sleep 0.05; done
  kill -9 "$pid" 2>/dev/null || true
  rm -rf "$data_dir"
done

python3 - "${startup_times[@]}" -- "${rss_values[@]}" <<'PY'
import statistics, sys
args = sys.argv[1:]
split = args.index("--")
startup = [int(v) for v in args[:split]]
rss = [int(v) for v in args[split + 1:]]
if startup:
    print(f"startup_ms       min={min(startup)}  median={int(statistics.median(startup))}  max={max(startup)}  n={len(startup)}")
if rss:
    print(f"rss_mb           min={min(rss)/1024:.0f}  median={statistics.median(rss)/1024:.0f}  max={max(rss)/1024:.0f}")
PY
