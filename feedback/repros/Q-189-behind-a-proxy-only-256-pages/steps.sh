port=$((19600 + $$ % 90))
"$QUILLO" run main.qwe --port $port --trust-proxy 127.0.0.1 > log.txt 2>&1 &
pid=$!
sleep 1
echo "client: [$(curl -s -m 2 -H 'X-Forwarded-For: 203.0.113.9' 127.0.0.1:$port/ip)]"
kill $pid 2> /dev/null
sleep 0.2
rm -f log.txt
