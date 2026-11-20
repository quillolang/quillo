# Opens the page at 127.0.0.1, where Chrome takes WebTransport (at localhost it tries ::1 and
# falls back to the WebSocket: Q-1266), stops the program 4 s in (SIGTERM) and starts it again,
# then posts "1", "2", ... to /say a second apart. The page says the first post it heard.
chrome="${QUILLO_CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || chrome=$(command -v google-chrome || command -v chromium)
port=$((18800 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
profile=$(mktemp -d)
node ../../../tests/browser/run.js "$chrome" "http://127.0.0.1:$port/" "$profile" 60000 listen.js &
page=$!
sleep 4
kill $pid; wait $pid 2>/dev/null
"$QUILLO" run main.qwe --port $port >> log.txt 2>&1 &
pid=$!
( n=0; while [ $n -lt 45 ]; do sleep 1; n=$((n + 1)); curl -s -o /dev/null -d "text=$n" "http://127.0.0.1:$port/say"; done ) &
posts=$!
wait $page
echo
kill $posts 2>/dev/null; kill $pid; wait $pid 2>/dev/null; rm -rf "$profile" log.txt
