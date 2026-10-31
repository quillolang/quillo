# One SIGTERM, with a request computing: does it stop within the guide's 10 s?
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null --max-time 20 127.0.0.1:$port/forever &
sleep 0.3
kill -TERM $pid
sleep 12
if kill -0 $pid 2>/dev/null; then
    echo "still running 12 s after one SIGTERM"
    kill -TERM $pid; sleep 0.5
    kill -0 $pid 2>/dev/null && echo "and after a second" || echo "a second SIGTERM stopped it"
    kill -9 $pid 2>/dev/null
else
    echo "stopped after one SIGTERM"
fi
rm -f log.txt
