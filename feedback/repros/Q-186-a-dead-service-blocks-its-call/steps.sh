port=$((18900 + $$ % 90))
svc=$((port + 100))
rm -f front.db* ledger.db*
export QUILLO_SERVICE_SECRET=a-secret-of-the-repro
"$QUILLO" run ledger.qwe --port $svc --as-service --service-name ledger > svc.txt 2>&1 &
a=$!
"$QUILLO" run main.qwe --port $port --service-url ledger=http://127.0.0.1:$svc > log.txt 2>&1 &
b=$!
sleep 1
curl -s -o /dev/null 127.0.0.1:$port/post/ana
sleep 2.2
kill -9 $a
wait $a 2> /dev/null
sqlite3 ledger.db 'update _quillo_calls set at = at - 700000'
"$QUILLO" run ledger.qwe --port $svc --as-service --service-name ledger > svc.txt 2>&1 &
a=$!
sleep 22
curl -s 127.0.0.1:$port/waiting
echo
kill $a $b
sleep 0.2
rm -f log.txt svc.txt front.db* ledger.db*
