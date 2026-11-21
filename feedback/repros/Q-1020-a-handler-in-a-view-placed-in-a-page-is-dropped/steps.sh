# What quillo check says, then three real clicks on each button in headless Chrome.
echo "check: $("$QUILLO" check main.qwe 2>&1)"
port=$((18700 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_up && break; sleep 0.2; done
PORT=$port node ../../tools/drive.mjs page.mjs
kill $pid; wait $pid 2>/dev/null; rm -f log.txt
