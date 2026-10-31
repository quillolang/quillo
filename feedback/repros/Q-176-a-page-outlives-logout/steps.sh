# Signs in, points at a link to a private page, logs out with a form (a new page, so
# the prefetch itself is gone), and follows a link to the same page.
chrome="${QUILLO_CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || chrome=$(command -v google-chrome || command -v chromium)
port=$((18800 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
profile=$(mktemp -d)
node ../../../tests/browser/run.js "$chrome" "http://127.0.0.1:$port/" "$profile" 30000 hover.js
echo
kill $pid; rm -rf "$profile" log.txt
