port=$((18800 + $$ % 90))
rm -rf data quillo-system.db*
mkdir data
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null 127.0.0.1:$port/rollback/bob
sleep 3
curl -s 127.0.0.1:$port/sent
echo
kill $pid
sleep 0.2
rm -rf log.txt data quillo-system.db*
