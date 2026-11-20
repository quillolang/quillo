# Makes tenant acme, opens its page, then asks /_quillo/ready with the health token.
port=$((18900 + $$ % 90)); T=abcdefghijklmnop1234
rm -rf data && mkdir data
QUILLO_HEALTH_TOKEN=$T "$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
for i in $(seq 50); do curl -s -o /dev/null localhost:$port/_quillo/health && break; sleep 0.2; done
curl -s -w ' [%{http_code}]\n' -X POST localhost:$port/teams
curl -s -w ' [%{http_code}]\n' -H 'Host: acme.localhost' localhost:$port/
curl -s -w ' [%{http_code}]\n' -H "Authorization: Bearer $T" localhost:$port/_quillo/ready
kill $pid; wait $pid 2>/dev/null
rm -rf data log.txt
