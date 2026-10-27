#!/bin/sh
# Makes the starting inputs in corpus/<target>/: the programs in examples/
# and the repository's other .qwe files, and a few requests, JSON texts
# and numbers. Run from anywhere; what fuzzing finds is added there.
set -e
cd "$(dirname "$0")"
root=..
for t in parse check; do
    mkdir -p corpus/$t
    find $root/examples $root/site $root/apps $root/tests/fixtures -name '*.qwe' 2>/dev/null | while read -r f; do
        cp "$f" "corpus/$t/$(echo "$f" | tr '/.' '__')"
    done
done
# Programs from the tests: the text of each `r#"..."#` that looks like one.
for f in $root/tests/*.rs; do
    awk -v out="corpus/parse/$(basename "$f" .rs)" '
        /r#"/ { on = 1; n++; sub(/.*r#"/, ""); buf = "" }
        on && /"#/ { sub(/"#.*/, ""); buf = buf $0; if (length(buf) > 20) print buf > (out "_" n); close(out "_" n); on = 0; next }
        on { buf = buf $0 "\n" }
    ' "$f"
done
cp corpus/parse/*_[0-9]* corpus/check/ 2>/dev/null || true

mkdir -p corpus/http corpus/json corpus/decimal corpus/decimal_ops
# http's input is arbitrary's: the bytes of the request come last, after a
# few that cut it into pieces.
n=0
for r in 'GET / HTTP/1.1\r\nHost: x\r\n\r\n' \
         'POST /notes HTTP/1.1\r\nHost: x\r\nContent-Length: 5\r\n\r\nhello' \
         'POST /up HTTP/1.1\r\nHost: x\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;x=1\r\npedia\r\n0\r\nX-T: 1\r\n\r\n' \
         'GET /a HTTP/1.1\r\nHost: x\r\n\r\nGET /b HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n' \
         'POST / HTTP/1.0\r\nContent-Length: 3\r\nConnection: keep-alive\r\nExpect: 100-continue\r\n\r\nabc'; do
    n=$((n + 1))
    printf '\003\001\005\007\000'"$r" > corpus/http/seed$n
done
for f in $root/examples/*.json; do cp "$f" corpus/json/; done
printf '{"a":[1,2.5,-0.0,1e300,18446744073709551615,"x\\u00e9",null,true],"$dec":"1.5"}' > corpus/json/mixed
printf '{"$int":"9007199254740993"}' > corpus/json/int
for d in 12.50 -0.05 3 2.5e-3 79228162514264337593543950335 0.0000000000000000000000000001 1E+5 .5 5. -7.9228162514264337593543950335; do
    printf '%s' "$d" > "corpus/decimal/$d"
done
head -c 64 /dev/urandom > corpus/decimal_ops/random1
head -c 64 /dev/urandom > corpus/decimal_ops/random2
echo "corpus/: $(find corpus -type f | wc -l | tr -d ' ') inputs"
