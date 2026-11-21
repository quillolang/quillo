# What quillo check says, then the page in headless Chrome (no --watch).
echo "check: $("$QUILLO" check main.qwe 2>&1)"
port=$((18700 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_up && break; sleep 0.2; done
PORT=$port node ../../tools/drive.mjs page.mjs
echo "server log: $(grep -c 'a ticket is drawn' log.txt) ticket(s) drawn"
kill $pid; wait $pid 2>/dev/null; rm -f log.txt
