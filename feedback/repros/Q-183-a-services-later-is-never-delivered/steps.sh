port=$((18600 + $$ % 90))
rm -f main.db* svc.db*
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null 127.0.0.1:$port/go
sleep 3
curl -s 127.0.0.1:$port/seen
echo
kill $pid
sleep 0.2
rm -f log.txt main.db* svc.db*
