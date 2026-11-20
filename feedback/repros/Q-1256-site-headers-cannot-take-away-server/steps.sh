# Each route's server and x-frame-options headers.
port=$((18900 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/ && break; sleep 0.2; done
for p in / /own; do
  h=$(curl -s -D - -o /dev/null localhost:$port$p | tr -d '\r' | grep -i '^server:\|^x-frame-options:' | tr '\n' ' ')
  echo "$p: ${h:-(no server)}"
done
kill $pid; wait $pid 2>/dev/null
rm -f log.txt
