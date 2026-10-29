# Serves on all addresses and signs in from this machine's LAN address, as a
# kiosk would: shows the cookie a browser is given, and what the server said.
# A browser at http://<lan address> drops a Secure cookie, so the next page is
# the login again (curl keeps it). Then the same with --plain-http.
port=$((18800 + $$ % 90))
lan=$(ipconfig getifaddr en0 2>/dev/null || hostname -I 2>/dev/null | awk '{print $1}')
[ -z "$lan" ] && { echo "no LAN address to test from"; exit 0; }
for flags in "" "--plain-http"; do
  "$QUILLO" run main.qwe --host 0.0.0.0 --port $port $flags > log.txt 2>&1 &
  pid=$!
  sleep 1.5
  cookie=$(curl -s -i -X POST http://$lan:$port/login | grep -i '^set-cookie' | sed 's/=[^;]*;/=…;/')
  kill $pid 2>/dev/null; wait $pid 2>/dev/null
  echo "${flags:-(no flags)}: $cookie"
  grep -i 'warning' log.txt > said.txt
  if [ -z "$cookie" ]; then echo "  no answer: $(head -1 log.txt)"
  elif echo "$cookie" | grep -q Secure && [ ! -s said.txt ]; then echo "  Secure cookie over plain http: dropped silently"
  elif echo "$cookie" | grep -q Secure; then echo "  told: $(cut -c1-110 said.txt)"
  else echo "  not Secure: kept"; fi
done
rm -f log.txt said.txt
