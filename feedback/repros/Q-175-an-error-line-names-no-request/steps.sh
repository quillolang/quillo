#!/bin/sh
# Does the log's error line carry the id the visitor was given?
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port "$port" --log json > log.txt 2>&1 &
server=$!
n=0
while [ $n -lt 50 ] && ! curl -s -m 1 -o /dev/null "http://127.0.0.1:$port/ok"; do
    sleep 0.1
    n=$((n + 1))
done
rid=$(curl -s -m 5 -o /dev/null -D - "http://127.0.0.1:$port/checkout" | tr -d '\r' | awk 'tolower($1) == "x-request-id:" { print $2 }')
sleep 0.2
kill "$server"
wait "$server" 2>/dev/null
echo "visitor was given: ${rid:-nothing}"
line=$(grep 'division by zero' log.txt | head -1)
echo "error line: $line"
case "$line" in
    "") echo "error line names the request: no line" ;;
    *"$rid"*) echo "error line names the request: yes" ;;
    *) echo "error line names the request: no" ;;
esac
rm -f log.txt
