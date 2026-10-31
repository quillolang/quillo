# Two signups for one name, then what that tenant's database holds.
port=$((18900 + $$ % 90 + 2))
"$QUILLO" run main.qwe --port $port > log.txt 2>&1 &
pid=$!
sleep 1
for i in 1 2; do
    curl -s -H 'Host: example.com' -H 'Origin: http://example.com' -d 'name=acme' 127.0.0.1:$port/signup; echo
done
echo "acme holds: $(curl -s -H 'Host: acme.example.com' 127.0.0.1:$port/plans)"
kill $pid; rm -rf log.txt data
awk '/^### Many tenants/{f=1} f&&/^- `tenant_from/{exit} f' ../../../USERGUIDE.md | grep -q new_tenant &&
    echo "the sample checks for an existing tenant: yes" || echo "the sample checks for an existing tenant: no"
