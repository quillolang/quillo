d=$(mktemp -d /tmp/q182.XXXXXX)
port=$((18500 + $$ % 90))
db=$((port + 100))
mariadb-install-db --datadir=$d/data --auth-root-authentication-method=normal --skip-test-db > /dev/null 2>&1
mariadbd --no-defaults --datadir=$d/data --port=$db --bind-address=127.0.0.1 --socket=$d/s --pid-file=$d/pid --skip-grant-tables > /dev/null 2>&1 &
my=$!
for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20; do
  mariadb --no-defaults -h 127.0.0.1 -P $db -u root -e 'create database ctl' 2> /dev/null && break
  sleep 0.5
done
REPRO_DB="mysql://root@127.0.0.1:$db/ctl" "$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1.5
curl -s 127.0.0.1:$port/go
echo
kill $pid
kill $my
sleep 0.5
rm -rf "$d" log.txt
