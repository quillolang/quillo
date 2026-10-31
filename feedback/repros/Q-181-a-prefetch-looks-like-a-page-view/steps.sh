port=$((18400 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null -H 'Sec-Purpose: prefetch' 127.0.0.1:$port/
curl -s -o /dev/null 127.0.0.1:$port/
sleep 0.2
kill $pid
sleep 0.2
grep 'GET /' log.txt
echo "log lines naming a prefetch: $(grep -c -i prefetch log.txt)"
rm -f log.txt
