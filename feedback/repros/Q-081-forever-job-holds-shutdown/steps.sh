# Starts the server, lets the job begin, stops it, and says how long that took.
port=$((18900 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 3
start=$(date +%s)
kill -TERM $pid
while kill -0 $pid 2>/dev/null; do sleep 0.2; [ $(( $(date +%s) - start )) -gt 20 ] && kill -9 $pid; done
echo "stopped after $(( $(date +%s) - start ))."
grep -c 'still running; skipped' log.txt | sed 's/^/skip lines: /'
grep 'internal error' log.txt
rm -f log.txt
