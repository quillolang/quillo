# Serves on all addresses, as a kiosk would reach it, signs in once, and shows the cookie
# the browser is given and what the server said. A browser at http://<lan address>
# drops a Secure cookie, so the next page is the login again (curl keeps it).
port=$((18800 + $$ % 90))
"$QUILLO" run main.qwe --host 0.0.0.0 --port $port > log.txt 2>&1 &
pid=$!
sleep 1.5
cookie=$(curl -s -i -X POST http://127.0.0.1:$port/login | grep -i '^set-cookie' | sed 's/=[^;]*;/=…;/')
kill $pid; wait $pid 2>/dev/null
echo "$cookie"
grep -iE 'https|secure|plain http' log.txt | grep -v 'set-cookie' > said.txt
if echo "$cookie" | grep -q Secure && [ ! -s said.txt ]; then echo "Secure cookie over plain http: dropped silently"; else echo "told:"; cat said.txt; fi
rm -f log.txt said.txt
