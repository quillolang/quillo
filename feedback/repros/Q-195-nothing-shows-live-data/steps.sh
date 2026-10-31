port=$((19800 + $$ % 90))
ulimit -S -n 256
"$QUILLO" run main.qwe --port $port --metrics --live-per-client 1000 > log.txt 2>&1 &
pid=$!
sleep 1
got=$(curl -s -m 2 127.0.0.1:$port/_quillo/metrics | grep -c '^quillo_live_pages')
[ "$got" = 1 ] && echo "page 300 opened" || echo "page 300 refused"
kill $pid 2> /dev/null
sleep 0.2
rm -f log.txt
