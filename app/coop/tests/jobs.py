#!/usr/bin/env python3
"""The daily job: a fixed deposit that reached its day is paid into savings
with its interest, and last month's savings interest is posted, once each.

    python3 app/coop/tests/jobs.py http://127.0.0.1:8080 <path of coop.db>

Needs COOP_DEMO=1, freshly seeded. Moves a deposit's day to today in the
database rather than wait three months."""

import sqlite3, sys, time
sys.path.insert(0, __import__("os").path.dirname(__file__))
from hostile_lib import Visitor, check, refused, results

BASE, DB = sys.argv[1].rstrip("/"), sys.argv[2]


def q(sql, *args, write=False):
    con = sqlite3.connect(DB, timeout=10)
    try:
        cur = con.execute(sql, args)
        if write:
            con.commit()
        return cur.fetchall()
    finally:
        con.close()


carla = Visitor(BASE, "M-0003")
mario = Visitor(BASE, "mario")
clara = Visitor(BASE, "clara")

r = carla.rpc("open_deposit", {"amount": "20,000", "months": 3})
check("Carla opens a 3-month deposit", not refused(r), r)
deposit = r[1]["ok"]
savings_before = q("select balance from accounts where number = 'SV-0003'")[0][0]
check("not for the clerk to run", refused(clara.rpc("run_daily")))
q("update accounts set matures = date('now') where id = ?", deposit, write=True)

r = mario.rpc("run_daily")
check("the job pays out one deposit and posts the interest", not refused(r) and r[1]["ok"]["matured"] == 1 and r[1]["ok"]["interest"] >= 8, r)
# 3% a year on ₱20,000 for 3 months: ₱150.
after = q("select balance from accounts where number = 'SV-0003'")[0][0]
interest = q("select amount from entries where account_id = (select id from accounts where number = 'SV-0003') and kind = 'interest'")
check("Carla's savings get ₱20,150 and the month's interest", after - savings_before == 2015000 + sum(x[0] for x in interest), (savings_before, after, interest))
check("the deposit is matured, empty", q("select status, balance from accounts where id = ?", deposit)[0] == ("matured", 0))

r = mario.rpc("run_daily")
check("run again, it does nothing", not refused(r) and r[1]["ok"] == {"matured": 0, "interest": 0}, r)
check("each savings account got the interest once", q("select max(n) from (select count(*) as n from entries where kind = 'interest' group by account_id)")[0][0] == 1)

print(f"\n{sum(results)}/{len(results)} passed")
