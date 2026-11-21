# The program with no port asked for (no --port, $PORT unset) while the default port, 8080, is held by
# another listener. A program with nothing to serve over HTTP runs without a port then (with it free
# it listens there, as 0.8.9 did, for health and metrics) (FEEDBACK72.md, Q-1011, the decision after
# review; branch feedback72-81).
unset PORT
python3 -c "import socket,time; s=socket.socket(); s.bind(('127.0.0.1', 8080)); s.listen(); time.sleep(5)" 2>/dev/null & hold=$!
sleep 0.5
"$QUILLO" run main.qwe > log.txt 2>&1 & pid=$!
sleep 1.5
echo "port taken: $(tr '\n' ' ' < log.txt)"
kill $pid 2>/dev/null; wait $pid 2>/dev/null
kill $hold 2>/dev/null; wait $hold 2>/dev/null; rm -f log.txt
