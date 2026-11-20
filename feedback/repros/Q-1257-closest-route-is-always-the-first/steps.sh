# A JSON post missing `message`, and one with "updates": "yes": which miss does the log name?
port=$((18900 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/contact && break; sleep 0.2; done
curl -s -o /dev/null -w '%{http_code}\n' -H 'content-type: application/json' -d '{"name":"Bo","email":"bo@example.com"}' localhost:$port/contact
curl -s -o /dev/null -w '%{http_code}\n' -H 'content-type: application/json' -d '{"name":"Bo","email":"bo@example.com","message":"hi","updates":"yes"}' localhost:$port/contact
sleep 0.3
kill $pid; wait $pid 2>/dev/null
grep -o 'POST /contact (main.qwe:[0-9]*): .*' log.txt
rm -f log.txt
