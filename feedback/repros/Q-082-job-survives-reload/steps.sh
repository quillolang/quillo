# Runs a copy under --watch, edits it once the job runs, and asks for n after the reload.
port=$((18900 + $$ % 90 + 1))
dir=$(mktemp -d)
cp main.qwe "$dir/main.qwe"
"$QUILLO" run "$dir/main.qwe" --port $port --watch > "$dir/log.txt" 2>&1 &
pid=$!
sleep 3
echo "before: $(curl -s localhost:$port/)"
sed -i.bak 's/version: 1/version: 2/' "$dir/main.qwe"
sleep 3
echo "after: $(curl -s localhost:$port/)"
kill -9 $pid
rm -rf "$dir"
