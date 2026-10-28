#!/usr/bin/env python3
"""Time the chain: N transfers back and forth (gateway, payments, risk,
accounts, messages), then the pages that read the most.

    python3 bank/tests/scale.py http://localhost:8080 1000

Needs BANK_DEMO=1. Prints milliseconds: the median and the 95th percentile."""

import http.cookiejar, json, statistics, sys, time, urllib.request, urllib.parse

BASE, N = sys.argv[1].rstrip("/"), int(sys.argv[2])


def visitor(path, fields):
    jar = http.cookiejar.CookieJar()
    o = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))
    o.open(urllib.request.Request(BASE + path, data=urllib.parse.urlencode(fields).encode()))
    for c in jar:
        c.secure = False
    return o


def rpc(o, fn, *args):
    t = time.perf_counter()
    r = o.open(urllib.request.Request(BASE + "/_quillo/rpc/" + fn, data=json.dumps(list(args)).encode(), headers={"content-type": "application/json"}))
    body = json.loads(r.read())
    return (time.perf_counter() - t) * 1000, body


def summary(name, ms):
    ms = sorted(ms)
    print(f"{name:34s} n={len(ms):5d}  median {statistics.median(ms):7.1f} ms   p95 {ms[int(len(ms) * .95) - 1]:7.1f} ms")


ada = visitor("/login", {"number": "10000001", "pin": "246810"})
ben = visitor("/login", {"number": "10000002", "pin": "246810"})
omar = visitor("/staff/login", {"username": "omar", "password": "password"})
a_acct = rpc(ada, "load_home")[1]["ok"]["accounts"][0]
b_acct = rpc(ben, "load_home")[1]["ok"]["accounts"][0]

times, statuses = [], {}
start = time.perf_counter()
for i in range(N):
    who, frm, to = (ada, a_acct, b_acct) if i % 2 == 0 else (ben, b_acct, a_acct)
    ms, r = rpc(who, "send_money", frm["id"], to["number"], "1", f"scale {i}")
    times.append(ms)
    s = r.get("ok", {}).get("status") or r.get("err", "?")
    statuses[s] = statuses.get(s, 0) + 1
took = time.perf_counter() - start
summary("send_money (the whole chain)", times)
print(f"{'':34s} {N / took:.0f} transfers a second, one at a time; {statuses}")

summary("load_dashboard (staff)", [rpc(omar, "load_dashboard")[0] for _ in range(30)])
summary("load_home (client)", [rpc(ada, "load_home")[0] for _ in range(30)])
summary("load_account (100 lines)", [rpc(ada, "load_account", a_acct["id"])[0] for _ in range(30)])
summary("load_customer (staff)", [rpc(omar, "load_customer", "10000001")[0] for _ in range(30)])
health = json.loads(urllib.request.urlopen(BASE + "/health").read())
print("ledger after:", "adds up" if health["ok"] and health["ledger"] == [] else health)
