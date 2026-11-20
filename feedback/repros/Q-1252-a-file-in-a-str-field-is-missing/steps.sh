# Posts a file as `name` to both routes (multipart, as a browser's <input type="file" name="name"> sends it), then the log's warnings.
port=$((18900 + $$ % 90))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/ && break; sleep 0.2; done
printf 'hello\n' > hello.txt
curl -s -w ' [%{http_code}]\n' -F name=@hello.txt localhost:$port/a
curl -s -w ' [%{http_code}]\n' -F name=@hello.txt localhost:$port/b
kill $pid; wait $pid 2>/dev/null
grep 'warning' log.txt
rm -f log.txt hello.txt
