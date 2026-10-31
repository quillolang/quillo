port=$((19700 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
for i in 1 2 3 4 5 6; do curl -s -o /dev/null 127.0.0.1:$port/; done
kill $pid
sleep 0.2
echo "loader runs: $(grep -c '^ran$' log.txt)"
rm -f log.txt
