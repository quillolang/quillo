#!/usr/bin/env python3
"""Quillo against Python on a database: SQLite, through each one's usual way.

    python3 bench/vs-python/db.py [quillo binary] [name ...]

Python uses its standard `sqlite3`, rows as tuples and `executemany`, as it is usually
written; Quillo `db`, rows as maps. Each case runs three times on a fresh copy of the same
database and the middle time is kept, process start included. A case whose answers differ
fails, as `run.py`'s do.
"""
import os, random, shutil, sqlite3, statistics, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

ROWS = 300000

CASES = [
    # Every row read, and one field summed in the program.
    ("scan 300k rows",
     'db.open("bench.db")\nlet t = 0\nfor r in db.query("select id, region, total, name from items") {\n    t += r.total\n}\nprint(t)\n',
     'import sqlite3\nc = sqlite3.connect("bench.db")\nt = 0\nfor r in c.execute("select id, region, total, name from items").fetchall():\n    t += r[2]\nprint(t)\n'),
    # One row at a time by its key, as a page asks for one thing.
    ("50k reads by key",
     'db.open("bench.db")\nlet n = 0\nfor i in 0..50000 {\n    let r = db.one("select name from items where id = ?", (i * 7) % 300000)\n    n += len(r.name)\n}\nprint(n)\n',
     'import sqlite3\nc = sqlite3.connect("bench.db")\nn = 0\nfor i in range(50000):\n    r = c.execute("select name from items where id = ?", ((i * 7) % 300000,)).fetchone()\n    n += len(r[0])\nprint(n)\n'),
    # Many writes in one transaction.
    ("100k inserts",
     'db.open("bench.db")\ndb.exec("create table added (id integer, name text)")\ndb.transaction(() => {\n    for i in 0..100000 {\n        db.exec("insert into added values (?, ?)", i, `n${i}`)\n    }\n})\nprint(db.one("select count(*) as n from added").n)\n',
     'import sqlite3\nc = sqlite3.connect("bench.db")\nc.execute("create table added (id integer, name text)")\nwith c:\n    c.executemany("insert into added values (?, ?)", ((i, f"n{i}") for i in range(100000)))\nprint(c.execute("select count(*) from added").fetchone()[0])\n'),
    # Work the database does: what is measured is the call around it.
    ("group by in SQL",
     'db.open("bench.db")\nlet rows = db.query("select region, sum(total) as t from items group by region order by region")\nprint(len(rows))\nprint(rows[0].t)\n',
     'import sqlite3\nc = sqlite3.connect("bench.db")\nrows = c.execute("select region, sum(total) as t from items group by region order by region").fetchall()\nprint(len(rows))\nprint(rows[0][1])\n'),
    # Rows as an API gives them: JSON objects.
    ("100k rows to JSON",
     'db.open("bench.db")\nlet rows = db.query("select id, region, total, name from items where id < 100000")\nprint(len(json.stringify(rows)))\n',
     'import sqlite3, json\nc = sqlite3.connect("bench.db")\nc.row_factory = sqlite3.Row\nrows = [dict(r) for r in c.execute("select id, region, total, name from items where id < 100000")]\nprint(len(json.dumps(rows, separators=(",", ":"))))\n'),
    # A query made with values, many times over.
    ("20k filtered queries",
     'db.open("bench.db")\nlet n = 0\nfor i in 0..20000 {\n    n += len(db.query("select id from items where region = ? and total = ? limit 5", `r${i % 50}`, i % 100))\n}\nprint(n)\n',
     'import sqlite3\nc = sqlite3.connect("bench.db")\nn = 0\nfor i in range(20000):\n    n += len(c.execute("select id from items where region = ? and total = ? limit 5", (f"r{i % 50}", i % 100)).fetchall())\nprint(n)\n'),
]


def database(path):
    c = sqlite3.connect(path)
    c.execute("create table items (id integer primary key, region text, total integer, name text)")
    c.execute("create index items_region on items (region, total)")
    c.executemany("insert into items values (?, ?, ?, ?)", ((i, f"r{i % 50}", i % 100, f"n{i}") for i in range(ROWS)))
    c.commit()
    c.close()


def run(cmd, d, master, limit):
    shutil.copy(master, os.path.join(d, "bench.db"))
    for extra in ("bench.db-wal", "bench.db-shm"):
        p = os.path.join(d, extra)
        if os.path.exists(p):
            os.remove(p)
    start = time.perf_counter()
    try:
        p = subprocess.run(cmd, cwd=d, capture_output=True, text=True, timeout=limit)
    except subprocess.TimeoutExpired:
        return limit, f"over {limit} s"
    return time.perf_counter() - start, p.stdout.strip() or p.stderr.strip()[:200]


def main():
    args = sys.argv[1:]
    quillo = os.path.abspath(args[0]) if args else os.path.join(ROOT, "target/release/quillo")
    only = args[1:]
    d = tempfile.mkdtemp(prefix="vs-python-db-")
    master = os.path.join(d, "master.db")
    database(master)
    print(f"{quillo}: {subprocess.run([quillo, '--version'], capture_output=True, text=True).stdout.strip()}")
    print(f"{sys.executable}: {sys.version.split()[0]}, sqlite {sqlite3.sqlite_version}\n")
    print(f"{'case':<22}{'python':>9}{'quillo':>9}  {'ratio':>7}")
    behind, wrong = [], []
    for name, q, py in CASES:
        if only and name not in only:
            continue
        with open(os.path.join(d, "main.qwe"), "w") as f:
            f.write(q)
        # Not `json.py` or `sqlite3.py`: a file named like a module would be imported instead.
        with open(os.path.join(d, "bench_case.py"), "w") as f:
            f.write(py)
        pt = [run([sys.executable, "-I", "bench_case.py"], d, master, 120) for _ in range(3)]
        qt = [run([quillo, "run", "main.qwe", "--no-check"], d, master, 120) for _ in range(3)]
        p, q_ = statistics.median(t for t, _ in pt), statistics.median(t for t, _ in qt)
        same = pt[0][1].split() == qt[0][1].split()
        note = "" if same else f"  ANSWERS DIFFER: {pt[0][1][:40]!r} / {qt[0][1][:60]!r}"
        if not same:
            wrong.append(name)
        if q_ > p:
            behind.append(name)
        print(f"{name:<22}{p:>8.3f}s{q_:>8.3f}s  {q_ / p:>6.2f}x{'  behind' if q_ > p else ''}{note}", flush=True)
    shutil.rmtree(d, ignore_errors=True)
    print(f"\nbehind Python: {len(behind)} {behind}")
    if wrong:
        print(f"answers differ: {wrong}")


if __name__ == "__main__":
    main()
