# The program twice, with no port asked for (no --port, $PORT unset): once with the default port,
# 8080, held by another listener, once without. A program with nothing to serve over HTTP takes no
# port either way (FEEDBACK72.md, Q-1011, the decision; branch feedback72-81).
unset PORT
python3 -c "import socket,time; s=socket.socket(); s.bind(('127.0.0.1', 8080)); s.listen(); time.sleep(5)" 2>/dev/null & hold=$!
sleep 0.5
"$QUILLO" run main.qwe > log.txt 2>&1 & pid=$!
sleep 1.5
echo "port taken: $(tr '\n' ' ' < log.txt)"
kill $pid 2>/dev/null; wait $pid 2>/dev/null
kill $hold 2>/dev/null; wait $hold 2>/dev/null
"$QUILLO" run main.qwe > log.txt 2>&1 & pid=$!
sleep 1.5
l=$(lsof -nP -a -p $pid -iTCP -sTCP:LISTEN 2>/dev/null | awk 'NR > 1 { print $9 }' | tr '\n' ' ')
echo "port free: TCP listeners: ${l:-none}"
echo "log: $(tr '\n' ' ' < log.txt)"
kill $pid 2>/dev/null; wait $pid 2>/dev/null; rm -f log.txt
