# Points at a link, changes what it leads to from the same page, and follows it.
chrome="${QUILLO_CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$chrome" ] || chrome=$(command -v google-chrome || command -v chromium)
port=$((18700 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
for s in write logout; do
    profile=$(mktemp -d)
    node ../../../tests/browser/run.js "$chrome" "http://127.0.0.1:$port/" "$profile" 30000 $s.js
    echo
    rm -rf "$profile"
done
kill $pid; rm -f log.txt
