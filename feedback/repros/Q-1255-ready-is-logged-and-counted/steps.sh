# Asks /_quillo/ready twice and nothing else, then reads the request counter and the log.
port=$((18900 + $$ % 90)); T=abcdefghijklmnop1234
rm -f app.db app.db-wal app.db-shm
QUILLO_HEALTH_TOKEN=$T "$QUILLO" run main.qwe --port $port --metrics > log.txt 2>&1 &
pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_quillo/health && break; sleep 0.2; done
curl -s -o /dev/null localhost:$port/_quillo/ready
curl -s -o /dev/null localhost:$port/_quillo/ready
curl -s -H "Authorization: Bearer $T" localhost:$port/_quillo/metrics | grep '^quillo_requests_total{status="[25]xx"}'
echo "ready log lines: $(grep -c '_quillo/ready' log.txt)"
kill $pid; wait $pid 2>/dev/null
rm -f app.db app.db-wal app.db-shm log.txt
