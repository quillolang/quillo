# Opens the page in headless Chrome and runs page.js in it.
chrome="${QUILLO_CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || chrome=$(command -v google-chrome || command -v chromium)
port=$((18700 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
profile=$(mktemp -d)
node ../../../tests/browser/run.js "$chrome" "http://127.0.0.1:$port/" "$profile" 20000 page.js
echo
kill $pid; wait $pid 2>/dev/null; rm -rf "$profile" log.txt
