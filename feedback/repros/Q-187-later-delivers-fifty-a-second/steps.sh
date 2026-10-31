port=$((19000 + $$ % 90))
rm -f front.db*
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null 127.0.0.1:$port/many
sleep 2.5
curl -s 127.0.0.1:$port/left
echo
kill $pid
sleep 0.2
rm -f log.txt front.db*
