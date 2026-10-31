# The first connection goes to the first instance, which also accepts for both. It
# computes for ever: how many of ten new connections wait, with the second one idle?
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port --instances 2 > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -o /dev/null --max-time 20 127.0.0.1:$port/forever &
sleep 0.2
hung=0
for i in 1 2 3 4 5 6 7 8 9 10; do
    [ "$(curl -s --max-time 1 127.0.0.1:$port/ping)" = pong ] || hung=$((hung + 1))
done
echo "hung: $hung of 10"
kill -9 $pid; rm -f log.txt
