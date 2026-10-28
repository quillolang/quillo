# load.py BASE N CONCURRENCY [slow_ms] -> calls per second, and which copies answered
import sys, time, urllib.request, collections
from concurrent.futures import ThreadPoolExecutor
base, n, conc = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
slow = sys.argv[4] if len(sys.argv) > 4 else "0"
start = int(sys.argv[5]) if len(sys.argv) > 5 else 0
def one(i):
    try: return urllib.request.urlopen(f"{base}/go/{start + i}?slow={slow if i % 5 == 0 else 0}", timeout=30).read().decode()
    except Exception as e: return f"err {e}"
t = time.time()
with ThreadPoolExecutor(conc) as ex: res = list(ex.map(one, range(n)))
dt = time.time() - t
by = collections.Counter(r.split()[1] if r.startswith("ok") else "ERR" for r in res)
errs = [r for r in res if not r.startswith("ok")][:2]
print(f"{n} calls in {dt:.2f}s = {n/dt:.0f}/s; by copy: {dict(by)}" + (f"; e.g. {errs}" if errs else ""))
