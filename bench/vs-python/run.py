#!/usr/bin/env python3
"""Quillo against Python, on the same programs written the way each is usually written.

    python3 bench/vs-python/run.py [quillo binary] [name ...]

The Python is whichever runs this script: `python3.13 bench/vs-python/run.py` for that one.
Each case is run three times and the middle time kept, process start included. A case that
gives another answer in each language fails, as a fast wrong answer is worth nothing.
"""
import json, os, random, statistics, subprocess, sys, tempfile, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))

CASES = [
    ("fib", "fn fib(n: int) -> int = n < 2 ? n : fib(n - 1) + fib(n - 2)\nprint(fib(35))\n",
     "def fib(n):\n    return n if n < 2 else fib(n - 1) + fib(n - 2)\nprint(fib(35))\n"),
    ("float loop", "fn f(n: int) -> float {\n    let s = 0.0\n    let i = 1\n    while i <= n {\n        s += 1.0 / i\n        i += 1\n    }\n    s\n}\nprint(round(f(50000000), 6))\n",
     "def f(n):\n    s = 0.0\n    i = 1\n    while i <= n:\n        s += 1.0 / i\n        i += 1\n    return s\nprint(round(f(50000000), 6))\n"),
    ("sum list", "let xs = range(0, 5000000)\nprint(sum(xs))\n", "xs = list(range(5000000))\nprint(sum(xs))\n"),
    ("sum by loop", "fn f(xs) {\n    let t = 0\n    for x in xs {\n        t += x\n    }\n    t\n}\nprint(f(range(0, 5000000)))\n",
     "def f(xs):\n    t = 0\n    for x in xs:\n        t += x\n    return t\nprint(f(list(range(5000000))))\n"),
    ("filter map sum", "print(range(0, 3000000) | filter((x) => x % 3 == 0) | map((x) => x * 2) | sum())\n",
     "print(sum(x * 2 for x in range(3000000) if x % 3 == 0))\n"),
    ("append, pipe", "fn f(n) {\n    let xs = []\n    for i in 0..n {\n        xs = xs | push(i)\n    }\n    len(xs)\n}\nprint(f(1000000))\n",
     "def f(n):\n    xs = []\n    for i in range(n):\n        xs.append(i)\n    return len(xs)\nprint(f(1000000))\n"),
    ("append, top level", "let xs = []\nfor i in 0..300000 {\n    xs = xs | push(i)\n}\nprint(len(xs))\n",
     "xs = []\nfor i in range(300000):\n    xs.append(i)\nprint(len(xs))\n"),
    ("map of lists", "fn f(n) {\n    let m = {}\n    for i in 0..n {\n        let k = str(i % 100)\n        m = m | set(k, get(m, k, []) | push(i))\n    }\n    len(m)\n}\nprint(f(100000))\n",
     "def f(n):\n    m = {}\n    for i in range(n):\n        k = str(i % 100)\n        m.setdefault(k, []).append(i)\n    return len(m)\nprint(f(100000))\n"),
    ("grid writes", "fn f(n) {\n    let g = range(0, 1000) | map((_y) => repeat_zero(1000))\n    for i in 0..n {\n        let y = i % 1000\n        let x = (i * 7) % 1000\n        g = set(g, y, set(g[y], x, i))\n    }\n    g[999][993]\n}\nfn repeat_zero(n) = range(0, n) | map((_x) => 0)\nprint(f(100000))\n",
     "def f(n):\n    g = [[0] * 1000 for _ in range(1000)]\n    for i in range(n):\n        y = i % 1000\n        x = (i * 7) % 1000\n        g[y][x] = i\n    return g[999][993]\nprint(f(100000))\n"),
    ("field read", "fn f(n) {\n    let row = { id: 1, name: \"a\", total: 5, paid: true }\n    let t = 0\n    for i in 0..n {\n        t += row.total\n    }\n    t\n}\nprint(f(1000000))\n",
     "def f(n):\n    row = {'id': 1, 'name': 'a', 'total': 5, 'paid': True}\n    t = 0\n    for i in range(n):\n        t += row['total']\n    return t\nprint(f(1000000))\n"),
    ("group json", "let rows = json.parse(read_file(\"rows.json\"))\nlet by = group_by(rows, (r) => r.region)\nprint(len(by))\nprint(sum(by[\"r1\"] | map((r) => r.total)))\n",
     "import json\nrows = json.load(open('rows.json'))\nby = {}\nfor r in rows:\n    by.setdefault(r['region'], []).append(r)\nprint(len(by))\nprint(sum(r['total'] for r in by['r1']))\n"),
    ("word frequency", "let counts = {}\nfor w in re.split(lower(read_file(\"words.txt\")), \"\\\\W+\") {\n    counts = counts | set(w, get(counts, w, 0) + 1)\n}\nprint(len(counts))\n",
     "import re\ncounts = {}\nfor w in re.split(r'\\W+', open('words.txt').read().lower()):\n    counts[w] = counts.get(w, 0) + 1\nprint(len(counts))\n"),
    ("sieve", "fn sieve(n) {\n    let flags = range(0, n + 1) | map((_i) => true)\n    let i = 2\n    while i * i <= n {\n        if flags[i] {\n            for j in range(i * i, n + 1, i) {\n                flags = flags | set(j, false)\n            }\n        }\n        i += 1\n    }\n    count(flags, (f) => f) - 2\n}\nprint(sieve(2000000))\n",
     "def sieve(n):\n    flags = [True] * (n + 1)\n    i = 2\n    while i * i <= n:\n        if flags[i]:\n            for j in range(i * i, n + 1, i):\n                flags[j] = False\n        i += 1\n    return sum(flags) - 2\nprint(sieve(2000000))\n"),
    ("string +=", "fn f(n) {\n    let s = \"\"\n    for i in 0..n {\n        s += \"x\"\n    }\n    len(s)\n}\nprint(f(1000000))\n",
     "def f(n):\n    s = ''\n    for i in range(n):\n        s += 'x'\n    return len(s)\nprint(f(1000000))\n"),
    ("s[i]", "fn f(s) {\n    let c = 0\n    for i in 0..len(s) {\n        if s[i] == \"x\" {\n            c += 1\n        }\n    }\n    c\n}\nprint(f(repeat(\"x\", 300000)))\n",
     "def f(s):\n    c = 0\n    for i in range(len(s)):\n        if s[i] == 'x':\n            c += 1\n    return c\nprint(f('x' * 300000))\n"),
    ("while over a text", "fn f(s) {\n    let i = 0\n    let c = 0\n    while i < len(s) {\n        if s[i] == \"a\" {\n            c += 1\n        }\n        i += 1\n    }\n    c\n}\nprint(f(repeat(\"ab\", 100000)))\n",
     "def f(s):\n    i = 0\n    c = 0\n    while i < len(s):\n        if s[i] == 'a':\n            c += 1\n        i += 1\n    return c\nprint(f('ab' * 100000))\n"),
    ("empty program", "print(1)\n", "print(1)\n"),
]


def data(d):
    r = random.Random(7)
    words = [f"w{i}" for i in range(5000)]
    with open(os.path.join(d, "words.txt"), "w") as f:
        f.write(" ".join(r.choice(words) for _ in range(1200000)))
    rows = [{"id": i, "region": f"r{i % 50}", "total": i % 100, "name": f"n{i}"} for i in range(300000)]
    with open(os.path.join(d, "rows.json"), "w") as f:
        json.dump(rows, f)


def run(cmd, d, limit):
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
    d = tempfile.mkdtemp(prefix="vs-python-")
    data(d)
    print(f"{quillo}: {subprocess.run([quillo, '--version'], capture_output=True, text=True).stdout.strip()}")
    print(f"{sys.executable}: {sys.version.split()[0]}\n")
    print(f"{'case':<20}{'python':>9}{'quillo':>9}  {'ratio':>7}")
    behind, wrong = [], []
    for name, q, py in CASES:
        if only and name not in only:
            continue
        with open(os.path.join(d, "main.qwe"), "w") as f:
            f.write(q)
        with open(os.path.join(d, "main.py"), "w") as f:
            f.write(py)
        pt = [run([sys.executable, "main.py"], d, 120) for _ in range(3)]
        qt = [run([quillo, "run", "main.qwe", "--no-check"], d, 40) for _ in range(3)]
        p, q_ = statistics.median(t for t, _ in pt), statistics.median(t for t, _ in qt)
        same = pt[0][1].split() == qt[0][1].split()
        note = "" if same else f"  ANSWERS DIFFER: {pt[0][1][:30]!r} / {qt[0][1][:50]!r}"
        if not same:
            wrong.append(name)
        if q_ > p:
            behind.append(name)
        print(f"{name:<20}{p:>8.2f}s{q_:>8.2f}s  {q_ / p:>6.2f}x{'  behind' if q_ > p else ''}{note}", flush=True)
    print(f"\nbehind Python: {len(behind)} {behind}")
    if wrong:
        print(f"answers differ: {wrong}")


if __name__ == "__main__":
    main()
