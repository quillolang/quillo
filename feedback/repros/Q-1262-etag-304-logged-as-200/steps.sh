# The page twice, the second time with If-None-Match: what curl gets, what the log and the metrics say.
port=$((18900 + $$ % 90)); T=abcdefghijklmnop1234
QUILLO_HEALTH_TOKEN=$T "$QUILLO" run main.qwe --port $port --metrics > log.txt 2>&1 & pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_quillo/health && break; sleep 0.2; done
echo "curl: $(curl -s -o /dev/null -w '%{http_code}' localhost:$port/help/hours), then $(curl -s -o /dev/null -w '%{http_code}' -H 'If-None-Match: "hours"' localhost:$port/help/hours)"
sleep 0.3; grep -o 'GET /help/hours -> [0-9]*' log.txt
curl -s -H "Authorization: Bearer $T" localhost:$port/_quillo/metrics | grep '^quillo_requests_total{status="[23]xx"}'
kill $pid; wait $pid 2>/dev/null; rm -f log.txt
