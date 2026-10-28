#!/bin/sh
# One service, scaled without changing its code: how many calls a second, which
# copy answered each, and whether any call ran twice (each run is a row in a
# shared SQLite file, with the caller's number).
#
#   sh feedback/tools/service-scale/run.sh            all of it (needs limer for the last three)
#   LIMER=~/dev/limer/limer QUILLO=./target/release/quillo sh .../run.sh
#
# 1. risk in the app's process   2. one copy elsewhere   3. three copies, listed
# 4. limer, round_robin, every 5th call slow past its timeout (retries)
# 5. the same, limer hashing on x-quillo-call   6. a third copy joining mid-run
set -u
here=$(cd "$(dirname "$0")" && pwd); root=$(cd "$here/../../.." && pwd)
Q=${QUILLO:-$root/target/release/quillo}; L=${LIMER:-$HOME/dev/limer/limer}
work=$(mktemp -d); cp "$here"/*.qwe "$here"/load.py "$work"/; cd "$work"
export QUILLO_SERVICE_SECRET=0123456789abcdef0123456789abcdef0123456789abcdef
export LIMER_ADMIN_SECRET=limer-admin-secret-0123456789abcdef0123456789
echo "$($Q --version)"

stop() { for p in $(pgrep -f "quillo run (app|risk).qwe"); do kill "$p"; done; sleep 1.5; pkill -f "limer run -c $work" 2>/dev/null; sleep 0.5; }
fresh() { rm -f runs.db runs.db-wal runs.db-shm; export RUNS_DB="$work/runs.db"; }
copies() { for p in "$@"; do ($Q run risk.qwe --as-service --port "$p" $REG > "r$p.log" 2>&1 &); done; sleep 2; }
app() { ($Q run app.qwe --port 19400 --service-url "risk=$1" > app.log 2>&1 &); sleep 1.5; }
twice() { sqlite3 runs.db "select count(*), count(distinct n), (select count(*) from (select n from runs group by n having count(*) > 1)) from runs" \
  | awk -F'|' '{print "   "$1" runs of "$2" calls; "$3" ran more than once"}'; }
limer() { sed "s/POLICY/$1/" "$here/Limerfile.in" > "$work/Limerfile"; ($L run -c "$work/Limerfile" > limer.log 2>&1 &); sleep 16; } # past old members' TTL
REG=""

fresh; export RISK_TIMEOUT=5s; app http://unused; stop
($Q run app.qwe --port 19400 > app.log 2>&1 &); sleep 1.5; echo "1. risk in the app's process"; python3 load.py http://127.0.0.1:19400 200 16; twice; stop
fresh; copies 19411; app http://127.0.0.1:19411; echo "2. one copy elsewhere"; python3 load.py http://127.0.0.1:19400 200 16; twice; stop
fresh; copies 19411 19412 19413; app http://127.0.0.1:19411,http://127.0.0.1:19412,http://127.0.0.1:19413
echo "3. three copies, listed"; python3 load.py http://127.0.0.1:19400 200 16; twice; stop

[ -x "$L" ] || { echo "no limer at $L: 4 to 6 skipped"; exit 0; }
REG="--register http://127.0.0.1:19390"; export RISK_TIMEOUT=300ms
fresh; limer round_robin; copies 19421 19422 19423; app http://127.0.0.1:19380
echo "4. limer round_robin; every 5th call 600 ms, its timeout 300 ms, 2 retries"; python3 load.py http://127.0.0.1:19400 50 8 600; twice; stop
fresh; limer "hash header x-quillo-call"; copies 19431 19432 19433; app http://127.0.0.1:19380
echo "5. limer hash header x-quillo-call; the same calls"; python3 load.py http://127.0.0.1:19400 50 8 600; twice; stop
fresh; export RISK_TIMEOUT=5s; limer round_robin; copies 19441 19442; app http://127.0.0.1:19380
echo "6. two copies through limer, a third started 2 s into 900 calls"
(python3 load.py http://127.0.0.1:19400 900 16 0 10000 > join.out &); sleep 2; copies 19443; sleep 8; cat join.out; stop
rm -rf "$work"
