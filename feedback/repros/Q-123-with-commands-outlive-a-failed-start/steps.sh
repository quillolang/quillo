#!/bin/sh
# The program does not parse, so quillo exits at once; its --with command should go with it.
"$QUILLO" run main.qwe --with 'exec sleep 4242' >/dev/null 2>&1
sleep 1
if ps -ax -o command | grep -v grep | grep -q 'sleep 4242'; then echo "still running"; pkill -f 'sleep 4242'; else echo "stopped"; fi
