# Asks for a WebTransport ticket as a page at localhost does (which starts the UDP listener), and
# prints the ticket and the addresses the program then listens on. A page at http://localhost:PORT
# dials the ticket's port at its own host name, which Chrome resolves to [::1] for QUIC: nothing
# listens there, the connection is refused (net::ERR_CONNECTION_REFUSED), and the page takes the
# WebSocket for good. TCP falls back to 127.0.0.1 by itself; QUIC does not.
port=$((18900 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
curl -s -X POST -H "Origin: http://localhost:$port" -H "Sec-Fetch-Site: same-origin" "http://localhost:$port/_quillo/wt-ticket" | sed 's/"ticket":"[^"]*"/"ticket":"…"/; s/"hash":"[^"]*"/"hash":"…"/'
echo
sleep 0.5
lsof -nP -a -p $pid -iUDP -iTCP -sTCP:LISTEN | awk 'NR > 1 { print $8, $9 }'
kill $pid; wait $pid 2>/dev/null; rm -f log.txt
