# Three loans, a backup, a mistake; a clean stop; the backup copied over the file; the count after.
port=$((18900 + $$ % 90)); rm -rf library.db library.db-wal library.db-shm backups; mkdir backups
up() { for i in $(seq 50); do curl -s -o /dev/null localhost:$port/count && return; sleep 0.2; done; }
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!; up
for i in 1 2 3; do curl -s -o /dev/null -X POST localhost:$port/loan; done
curl -s -o /dev/null -X POST localhost:$port/backup; curl -s -o /dev/null -X POST localhost:$port/mistake
echo "before the stop: $(curl -s localhost:$port/count)"
kill -INT $pid; wait $pid 2>/dev/null
for f in library.db library.db-wal library.db-shm; do [ -e $f ] && echo "after the stop: $f"; done
cp backups/good.db library.db
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 & pid=$!; up
echo "count after the restore: $(curl -s localhost:$port/count)"
kill -INT $pid; wait $pid 2>/dev/null
rm -rf library.db library.db-wal library.db-shm backups log.txt
