#!/bin/sh
# 30,000 of `reply ` before a number.
{ printf 'fn f() = '; i=0; while [ $i -lt 30000 ]; do printf 'reply '; i=$((i+1)); done; echo 200; } > chain.qwe
"$QUILLO" check chain.qwe 2>&1 | tail -2 | cut -c1-120
echo "exit $?"
rm -f chain.qwe
