# A request for a tenant nobody signed up.
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
echo "unknown tenant: $(curl -s -o body.txt -w '%{http_code}' -H 'Host: ghost.example.com' 127.0.0.1:$port/)"
cat body.txt; echo
kill $pid; rm -rf log.txt body.txt data
