# app/main.qwe run from this folder (as run.sh, which doesn't cd, runs it): POST /backup.
port=$((18900 + $$ % 90)); rm -f app/lib.db app/lib.db-wal app/lib.db-shm app/backups/b.db
"$QUILLO" run app/main.qwe --port $port > log.txt 2>&1 & pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_quillo/health && break; sleep 0.2; done
curl -s -X POST localhost:$port/backup; echo
grep -o 'unable to open database[^"]*' log.txt | head -1
kill $pid; wait $pid 2>/dev/null; rm -f log.txt app/lib.db app/lib.db-wal app/lib.db-shm app/backups/b.db
