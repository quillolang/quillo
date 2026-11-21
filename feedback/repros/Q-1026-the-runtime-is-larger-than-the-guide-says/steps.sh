# The runtime script of one counter (not --watch): its bytes raw and gzipped, its lines; then the guide's figure.
port=$((18800 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/ && break; sleep 0.2; done
url=$(curl -s localhost:$port/ | grep -o '/_quillo/r/[a-z0-9]*\.js' | head -1)
curl -s localhost:$port$url > runtime.js
raw=$(wc -c < runtime.js | tr -d ' ')
echo "runtime $url: $raw B raw, $(gzip -9c runtime.js | wc -c | tr -d ' ') B gzip -9, $(wc -l < runtime.js | tr -d ' ') lines, $(grep -o ' = ' runtime.js | wc -l | tr -d ' ') ' = '"
kill $pid; wait $pid 2>/dev/null; rm -f log.txt runtime.js
guide=$(grep -o 'about [0-9]* KB, [0-9]* KB gzipped' ../../../book/chapters/21-components.md | head -1)
echo "the guide says: ${guide:-no such figure}"
if [ "$raw" -gt 70000 ] && [ "$guide" = "about 35 KB, 11 KB gzipped" ]; then echo "verdict: open"; else echo "verdict: fixed"; fi
