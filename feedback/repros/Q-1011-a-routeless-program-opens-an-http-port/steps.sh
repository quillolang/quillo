# The program twice: once with its port held by another listener, once with the port free.
port=$((18600 + $$ % 90))
python3 -c "import socket,time; s=socket.socket(); s.bind(('127.0.0.1', $port)); s.listen(); time.sleep(5)" & hold=$!
sleep 0.5
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!
sleep 1.5
echo "port taken: $(tr '\n' ' ' < log.txt)"
kill $pid 2>/dev/null; wait $pid 2>/dev/null
kill $hold 2>/dev/null; wait $hold 2>/dev/null
port=$((port + 1))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!
sleep 1.5
echo "port free: GET / -> $(curl -s -m 2 -o /dev/null -w '%{http_code}' localhost:$port/)"
echo "log: $(tr '\n' ' ' < log.txt)"
kill $pid 2>/dev/null; wait $pid 2>/dev/null; rm -f log.txt
