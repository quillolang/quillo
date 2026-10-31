# Chrome with no debugger attached: with DevTools on the page it refuses to prerender
# (PrerenderingDisabledByDevTools), so the usual driver shows nothing. Nothing is clicked.
chrome="${QUILLO_CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || chrome=$(command -v google-chrome || command -v chromium)
port=$((18600 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
profile=$(mktemp -d)
"$chrome" --headless=new --disable-gpu --no-first-run --no-sandbox --user-data-dir="$profile" "http://127.0.0.1:$port/" > /dev/null 2>&1 &
browser=$!
sleep 6
curl -s 127.0.0.1:$port/counts
echo
kill $browser $pid
sleep 1
rm -rf "$profile" log.txt
