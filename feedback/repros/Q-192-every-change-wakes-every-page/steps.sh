port=$((19400 + $$ % 90))
rm -f live.db*
QUILLO_SECRET=a-secret-of-the-repro-long-enough-to-use "$QUILLO" run main.qwe --port $port --metrics  > log.txt 2>&1 &
pid=$!
sleep 1
node ../../tools/live-pages.mjs $port wakes
kill $pid
sleep 0.2
echo "loader runs: $(grep -c '^ran$' log.txt)"
rm -f log.txt live.db*
