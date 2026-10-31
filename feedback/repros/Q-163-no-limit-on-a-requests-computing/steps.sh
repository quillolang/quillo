# The loop under a limit of a second: is the request ended as a 500?
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port --max-compute 1s > log.txt 2>&1 &
pid=$!
sleep 1
if kill -0 $pid 2>/dev/null; then
    echo "answer with a limit: $(curl -s -o /dev/null -w '%{http_code}' --max-time 4 127.0.0.1:$port/forever)"
    kill -9 $pid
else
    echo "no such limit: $(head -1 log.txt)"
fi
rm -f log.txt
