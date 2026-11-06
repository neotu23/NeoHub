#!/bin/sh
# Emits coloured + CR-overwritten output so the parser (not a regex) must handle it.
printf '\033[32mstarting\033[0m green line\n'
printf '\033[33mwarn\033[0m yellow line\n'
printf 'progress 0%%\rprogress 50%%\rprogress 100%%\n'
printf '\033[1;34mbold blue\033[0m done\n'
i=0
while :; do
  i=$((i + 1))
  printf '\033[2m[fixture] tick %s\033[0m\n' "$i"
  sleep 0.1
done
