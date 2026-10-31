port=$((18700 + $$ % 90))
svc=$((port + 100))
rm -f front.db*
export QUILLO_SERVICE_SECRET=a-secret-of-the-repro
"$QUILLO" run deployed.qwe --port $svc --as-service --service-name mailer > svc.txt 2>&1 &
a=$!
"$QUILLO" run main.qwe --port $port --service-url mailer=http://127.0.0.1:$svc > log.txt 2>&1 &
b=$!
sleep 1
curl -s -o /dev/null 127.0.0.1:$port/send/ana
sleep 2.5
curl -s 127.0.0.1:$port/kept
echo
kill $a $b
sleep 0.2
rm -f log.txt svc.txt front.db*
