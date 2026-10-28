#!/usr/bin/env python3
"""A desk session submitted while the ledger is down, then again once it is
back: it carries on from where it stopped, with no second customer.

    python3 app/bank/tests/outage.py http://localhost:8080 <stop-accounts> <start-accounts>

The two commands take the accounts service down and bring it back (run split,
with a QUILLO_SERVICE_SECRET set, so a restarted service is trusted). Needs
BANK_DEMO=1. A killed service is started again by --split within a second, and
the call to it tried again, so the submit would not fail (FEEDBACK4.md, Q-042,
fixed on bank-fixes): hang it instead, which a restart cannot hide, with
`pkill -STOP -f 'accounts.qwe --port <port>'` and `pkill -CONT -f ...`. The
port is in the gateway's log: "service accounts (...) on http://127.0.0.1:<port>"."""

import http.cookiejar, json, subprocess, sys, time, urllib.request, urllib.error, urllib.parse

BASE, KILL, START = sys.argv[1].rstrip("/"), sys.argv[2], sys.argv[3]
results = []


class Visitor:
    def __init__(self):
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))

    def _secure_off(self):
        for c in self.jar:
            c.secure = False

    def post_form(self, path, fields):
        try:
            self.http.open(urllib.request.Request(BASE + path, data=urllib.parse.urlencode(fields).encode()))
        except urllib.error.HTTPError:
            pass
        self._secure_off()

    def rpc(self, fn, *args):
        req = urllib.request.Request(BASE + "/_quillo/rpc/" + fn, data=json.dumps(list(args)).encode(), headers={"content-type": "application/json"})
        try:
            r = self.http.open(req)
            self._secure_off()
            return json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            return {"http": e.code, "body": e.read().decode(errors="replace")[:200]}


def check(name, ok, got=None):
    results.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"   got: {got}"))


maria, phone = Visitor(), Visitor()
maria.post_form("/staff/login", {"username": "maria", "password": "password"})
customers_before = maria.rpc("load_dashboard")["ok"]["customers"]

code = maria.rpc("desk_start", "open_account", "")["ok"]
phone.rpc("desk_join", code)
for k, v in {"first_name": "Rosa", "last_name": "Parks", "birth_date": "1980-02-04", "email": "rosa@example.com",
             "street": "1 Bus Lane", "postcode": "D04", "city": "Dublin", "country": "Ireland",
             "account_name": "Everyday", "deposit": "100"}.items():
    maria.rpc("desk_set", code, k, v)
for section in ["personal", "address", "account", "deposit", "terms"]:
    maria.rpc("desk_ask", code, section)
    view = phone.rpc("load_desk", code, "client")["ok"]
    ask = [s for s in view["sections"] if s["id"] == section][0]["ask"]
    phone.rpc("desk_answer", ask["id"], True, "")
phone.rpc("desk_pin", code, "135790", "135790")
check("ready to submit", maria.rpc("load_desk", code, "clerk")["ok"]["missing"] == [], maria.rpc("load_desk", code, "clerk")["ok"]["missing"])

# The ledger goes down; submitting makes the customer, then stops.
subprocess.run(KILL, shell=True)
time.sleep(1)
first = maria.rpc("desk_submit", code)
check("submitting with the ledger down fails, and says why", "err" in first, first)
progress = maria.rpc("load_desk", code, "clerk")["ok"]["progress"]
check("what was done is remembered: the customer", progress.get("customer_id") is not None and progress.get("account_id") is None, progress)
check("the session is still open", maria.rpc("load_desk", code, "clerk")["ok"]["status"] == "open")

# Back up: submitting again carries on, and makes no second customer.
subprocess.Popen(START, shell=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
for _ in range(40):
    time.sleep(0.25)
    if "ok" in maria.rpc("load_dashboard") and maria.rpc("load_dashboard")["ok"]["ledger_problems"] == []:
        break
second = maria.rpc("desk_submit", code)
check("submitting again works", "ok" in second, second)
after = maria.rpc("load_dashboard")["ok"]
check("one new customer, not two", after["customers"] == customers_before + 1, (customers_before, after["customers"]))
done = maria.rpc("load_desk", code, "clerk")["ok"]
check("done, with the account", done["status"] == "done" and done["progress"].get("account_number", "").startswith("QB"), done["progress"])
check("its first deposit, once", any(t["currency"] == "EUR" for t in after["totals"]), after["totals"])
third = maria.rpc("desk_submit", code)
check("a third submit is refused (it is done)", "err" in third, third)
check("the ledger adds up", after["ledger_problems"] == [], after["ledger_problems"])

print(f"\n{sum(results)} of {len(results)} pass")
