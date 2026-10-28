# Runs the app for up to 3 s: does it serve the shop's route, or exit?
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 2
if kill -0 $pid 2>/dev/null; then
    echo "serving: $([ "$(curl -s localhost:$port/shop/)" = "the shop" ] && echo yes || echo no-answer)"
    kill $pid
else
    wait $pid; echo "exit $?, serving: no"
fi
cat log.txt; rm -f log.txt
