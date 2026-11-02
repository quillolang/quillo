#!/usr/bin/env python3
"""Quillo against Python on PostgreSQL and MariaDB: the cases of db.py, on a server.

    python bench/vs-python/db_servers.py [quillo binary] [pg|mysql ...]

Run it with a Python that has psycopg (3) and pymysql: `python -m venv v && v/bin/pip
install "psycopg[binary]" pymysql && v/bin/python bench/vs-python/db_servers.py`. It starts a
PostgreSQL and a MariaDB of its own in a temporary folder (initdb and pg_ctl, mariadbd from
Homebrew or the PATH), fills a table of 300,000 rows, and stops them at the end. Python is
written as it usually is: psycopg and pymysql, rows as tuples, `executemany`.
"""
import os, shutil, socket, statistics, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
ROWS = 300000


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def up(port, secs=30):
    end = time.time() + secs
    while time.time() < end:
        try:
            socket.create_connection(("127.0.0.1", port), 0.2).close()
            return True
        except OSError:
            time.sleep(0.1)
    return False


def find(names, roots):
    for r in roots:
        if all(os.path.exists(os.path.join(r, n)) for n in names):
            return r
    return None


def postgres(d):
    roots = [p for p in os.environ.get("PATH", "").split(":")] + [f"/opt/homebrew/opt/{v}/bin" for v in ("postgresql@17", "postgresql@16", "postgresql")]
    b = find(["initdb", "pg_ctl"], roots)
    if not b:
        return None, None
    data, port = os.path.join(d, "pg"), free_port()
    subprocess.run([f"{b}/initdb", "-D", data, "-U", "quillo", "--auth=trust", "-E", "UTF8", "--no-locale"], check=True, capture_output=True)
    subprocess.run([f"{b}/pg_ctl", "-D", data, "-o", f"-p {port} -k {d} -c listen_addresses=127.0.0.1 -c fsync=off", "-l", os.path.join(d, "pg.log"), "-w", "start"], check=True, capture_output=True)
    stop = lambda: subprocess.run([f"{b}/pg_ctl", "-D", data, "-m", "fast", "stop"], capture_output=True)
    return f"postgres://quillo@127.0.0.1:{port}/postgres", stop


def mariadb(d):
    b = find(["mariadbd", "mariadb-install-db"], ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/usr/sbin"])
    if not b:
        return None, None
    data, port = os.path.join(d, "my"), free_port()
    subprocess.run([f"{b}/mariadb-install-db", f"--datadir={data}", "--auth-root-authentication-method=normal", "--skip-test-db"], check=True, capture_output=True)
    p = subprocess.Popen([f"{b}/mariadbd", "--no-defaults", f"--datadir={data}", f"--port={port}", "--bind-address=127.0.0.1", f"--socket={d}/my.sock", f"--pid-file={d}/my.pid", f"--log-error={d}/my.log", "--skip-grant-tables", "--innodb-flush-log-at-trx-commit=0"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if not up(port):
        p.kill()
        return None, None
    def stop():
        p.terminate()
        p.wait(10)
    return f"mysql://root@127.0.0.1:{port}/bench", stop


def fill(kind, url):
    if kind == "pg":
        import psycopg
        with psycopg.connect(url.replace("postgres://", "postgresql://")) as c:
            c.execute("create table items (id integer primary key, region text, total integer, name text)")
            c.execute("create index items_region on items (region, total)")
            with c.cursor().copy("copy items from stdin") as cp:
                for i in range(ROWS):
                    cp.write_row((i, f"r{i % 50}", i % 100, f"n{i}"))
    else:
        import pymysql
        host, port = url.split("@")[1].split("/")[0].split(":")
        c = pymysql.connect(host=host, port=int(port), user="root")
        c.cursor().execute("create database bench")
        c.select_db("bench")
        cur = c.cursor()
        cur.execute("create table items (id integer primary key, region varchar(10), total integer, name varchar(20))")
        cur.execute("create index items_region on items (region, total)")
        rows = [(i, f"r{i % 50}", i % 100, f"n{i}") for i in range(ROWS)]
        for k in range(0, ROWS, 10000):
            cur.executemany("insert into items values (%s, %s, %s, %s)", rows[k:k + 10000])
        c.commit()
        c.close()


def connect_py(kind, url):
    if kind == "pg":
        return f"import psycopg\nc = psycopg.connect({url.replace('postgres://', 'postgresql://')!r}, autocommit=True)\n"
    host, port = url.split("@")[1].split("/")[0].split(":")
    return f"import pymysql\nc = pymysql.connect(host={host!r}, port={port}, user='root', database='bench', autocommit=True)\n"


def cases(kind, url):
    q = lambda body: f'db.open("{url}")\n{body}'
    p = lambda body: connect_py(kind, url) + body
    return [
        ("scan 300k rows",
         q('let t = 0\nfor r in db.query("select id, region, total, name from items") {\n    t += r.total\n}\nprint(t)\n'),
         p('cur = c.cursor()\ncur.execute("select id, region, total, name from items")\nt = 0\nfor r in cur.fetchall():\n    t += r[2]\nprint(t)\n')),
        ("10k reads by key",
         q('let n = 0\nfor i in 0..10000 {\n    let r = db.one("select name from items where id = ?", (i * 7) % 300000)\n    n += len(r.name)\n}\nprint(n)\n'),
         p('cur = c.cursor()\nn = 0\nfor i in range(10000):\n    cur.execute("select name from items where id = %s", ((i * 7) % 300000,))\n    n += len(cur.fetchone()[0])\nprint(n)\n')),
        # Each with its own batch call: Python's executemany, Quillo's db.exec_many.
        ("20k inserts",
         q('db.exec("drop table if exists added")\ndb.exec("create table added (id integer, name text)")\nlet _r = db.exec_many("insert into added values (?, ?)", for i in 0..20000 => [i, `n${i}`])\nprint(db.one("select count(*) as n from added").n)\n'),
         p('cur = c.cursor()\ncur.execute("drop table if exists added")\ncur.execute("create table added (id integer, name text)")\nc.autocommit = False\ncur.executemany("insert into added values (%s, %s)", [(i, f"n{i}") for i in range(20000)])\nc.commit()\ncur.execute("select count(*) from added")\nprint(cur.fetchone()[0])\n')),
        # One statement a call, in one transaction: a round trip each, on both sides.
        ("5k inserts one by one",
         q('db.exec("drop table if exists added")\ndb.exec("create table added (id integer, name text)")\ndb.transaction(() => {\n    for i in 0..5000 {\n        db.exec("insert into added values (?, ?)", i, `n${i}`)\n    }\n})\nprint(db.one("select count(*) as n from added").n)\n'),
         p('cur = c.cursor()\ncur.execute("drop table if exists added")\ncur.execute("create table added (id integer, name text)")\nc.autocommit = False\nfor i in range(5000):\n    cur.execute("insert into added values (%s, %s)", (i, f"n{i}"))\nc.commit()\ncur.execute("select count(*) from added")\nprint(cur.fetchone()[0])\n')),
        ("group by in SQL",
         q('let rows = db.query("select region, sum(total) as t from items group by region order by region")\nprint(len(rows))\nprint(rows[0].t)\n'),
         p('cur = c.cursor()\ncur.execute("select region, sum(total) as t from items group by region order by region")\nrows = cur.fetchall()\nprint(len(rows))\nprint(rows[0][1])\n')),
        ("100k rows to JSON",
         q('let rows = db.query("select id, region, total, name from items where id < 100000")\nprint(len(json.stringify(rows)))\n'),
         p('import json\ncur = c.cursor()\ncur.execute("select id, region, total, name from items where id < 100000")\ncols = [d[0] for d in cur.description]\nrows = [dict(zip(cols, r)) for r in cur.fetchall()]\nprint(len(json.dumps(rows, separators=(",", ":"))))\n')),
        ("5k filtered queries",
         q('let n = 0\nfor i in 0..5000 {\n    n += len(db.query("select id from items where region = ? and total = ? limit 5", `r${i % 50}`, i % 100))\n}\nprint(n)\n'),
         p('cur = c.cursor()\nn = 0\nfor i in range(5000):\n    cur.execute("select id from items where region = %s and total = %s limit 5", (f"r{i % 50}", i % 100))\n    n += len(cur.fetchall())\nprint(n)\n')),
    ]


def run(cmd, d, limit):
    start = time.perf_counter()
    try:
        p = subprocess.run(cmd, cwd=d, capture_output=True, text=True, timeout=limit)
    except subprocess.TimeoutExpired:
        return limit, f"over {limit} s"
    return time.perf_counter() - start, p.stdout.strip() or p.stderr.strip()[:200]


def main():
    args = sys.argv[1:]
    quillo = os.path.abspath(args[0]) if args and os.path.exists(args[0]) else os.path.join(ROOT, "target/release/quillo")
    kinds = [a for a in args if a in ("pg", "mysql")] or ["pg", "mysql"]
    d = tempfile.mkdtemp(prefix="vs-python-dbs-", dir=os.environ.get("BENCH_TMP"))
    print(f"{quillo}: {subprocess.run([quillo, '--version'], capture_output=True, text=True).stdout.strip()}")
    print(f"{sys.executable}: {sys.version.split()[0]}\n")
    behind, wrong = [], []
    try:
        for kind in kinds:
            url, stop = (postgres if kind == "pg" else mariadb)(d)
            if not url:
                print(f"{kind}: no server found, skipped")
                continue
            try:
                fill(kind, url)
                print(f"{'case (' + kind + ')':<22}{'python':>9}{'quillo':>9}  {'ratio':>7}")
                for name, q, py in cases(kind, url):
                    with open(os.path.join(d, "main.qwe"), "w") as f:
                        f.write(q)
                    with open(os.path.join(d, "bench_case.py"), "w") as f:
                        f.write(py)
                    pt = [run([sys.executable, "-I", "bench_case.py"], d, 120) for _ in range(3)]
                    qt = [run([quillo, "run", "main.qwe", "--no-check"], d, 120) for _ in range(3)]
                    p_, q_ = statistics.median(t for t, _ in pt), statistics.median(t for t, _ in qt)
                    same = pt[0][1].split() == qt[0][1].split()
                    note = "" if same else f"  ANSWERS DIFFER: {pt[0][1][:40]!r} / {qt[0][1][:60]!r}"
                    if not same:
                        wrong.append(f"{kind}: {name}")
                    if q_ > p_:
                        behind.append(f"{kind}: {name}")
                    print(f"{name:<22}{p_:>8.3f}s{q_:>8.3f}s  {q_ / p_:>6.2f}x{'  behind' if q_ > p_ else ''}{note}", flush=True)
                print()
            finally:
                stop()
    finally:
        shutil.rmtree(d, ignore_errors=True)
    print(f"behind Python: {len(behind)} {behind}")
    if wrong:
        print(f"answers differ: {wrong}")


if __name__ == "__main__":
    main()
