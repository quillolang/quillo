#!/usr/bin/env python3
"""Hostile calls to the bank's server fns, as each kind of visitor.

    python3 bank/tests/hostile.py http://localhost:8080

Needs BANK_DEMO=1, freshly seeded. Prints each check and its answer."""

import http.cookiejar, json, sys, urllib.request, urllib.error, urllib.parse

BASE = sys.argv[1].rstrip("/")
results = []


class Visitor:
    def __init__(self, name):
        self.name = name
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar), NoRedirect)

    def post_form(self, path, fields):
        data = urllib.parse.urlencode(fields).encode()
        try:
            status = self.http.open(urllib.request.Request(BASE + path, data=data)).status
        except urllib.error.HTTPError as e:
            status = e.code
        # The session cookie is Secure (__Host-quillo_session): browsers send it
        # to http://localhost, Python's cookie jar does not, so here it may.
        for c in self.jar:
            c.secure = False
        return status

    def rpc(self, fn, *args, raw=None):
        body = raw if raw is not None else json.dumps(list(args)).encode()
        req = urllib.request.Request(BASE + "/_quillo/rpc/" + fn, data=body, headers={"content-type": "application/json"})
        try:
            r = self.http.open(req)
            for c in self.jar:
                c.secure = False
            return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            text = e.read().decode(errors="replace")
            try:
                return e.code, json.loads(text)
            except ValueError:
                return e.code, text[:120]


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None

    def http_error_303(self, req, fp, code, msg, headers):
        return fp

    http_error_302 = http_error_301 = http_error_303


def check(name, ok, got):
    results.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"   got: {got}"))


def refused(answer, *words):
    status, body = answer
    text = json.dumps(body)
    return status in (400, 403, 404) or ('"err"' in text and any(w in text for w in words)) or body == {"ok": None} or body == {"ok": []}


nobody = Visitor("nobody")
ben = Visitor("ben")
ada = Visitor("ada")
maria = Visitor("maria")
omar = Visitor("omar")
ben.post_form("/login", {"number": "10000002", "pin": "246810"})
ada.post_form("/login", {"number": "10000001", "pin": "246810"})
maria.post_form("/staff/login", {"username": "maria", "password": "password"})
omar.post_form("/staff/login", {"username": "omar", "password": "password"})

# Ada's accounts, as Ada sees them.
status, home = ada.rpc("load_home")
ada_accounts = home["ok"]["accounts"]
ada_everyday = ada_accounts[0]["id"]
_, bhome = ben.rpc("load_home")
ben_number = bhome["ok"]["accounts"][0]["number"]

# ---- nobody signed in ----
check("nobody: load_home is null", nobody.rpc("load_home") == (200, {"ok": None}), nobody.rpc("load_home"))
check("nobody: send_money refused", refused(nobody.rpc("send_money", ada_everyday, ben_number, "10", "x"), "Sign in"), nobody.rpc("send_money", ada_everyday, ben_number, "10", "x"))
check("nobody: load_dashboard is null", nobody.rpc("load_dashboard") == (200, {"ok": None}), nobody.rpc("load_dashboard"))
check("nobody: find_customers is empty", nobody.rpc("find_customers", "a") == (200, {"ok": []}), nobody.rpc("find_customers", "a"))
check("nobody: load_customer is null", nobody.rpc("load_customer", "10000001") == (200, {"ok": None}), nobody.rpc("load_customer", "10000001"))
check("nobody: cash_in refused", refused(nobody.rpc("cash_in", ada_everyday, "1000"), "Staff only"), nobody.rpc("cash_in", ada_everyday, "1000"))
check("nobody: desk_start refused", refused(nobody.rpc("desk_start", "open_account", ""), "Staff only"), nobody.rpc("desk_start", "open_account", ""))

# ---- the services' own fns are not the gateway's ----
for fn, args in [("transfer", [2, 1, "QB00", 100, ""]), ("sign_in", ["10000001", "246810"]), ("deposit", [1, 100000, "x", "x"]), ("assess", [1, 1, 2, 1]), ("decide", [1, True, "x"])]:
    a = nobody.rpc(fn, *args)
    check(f"a service's fn is not callable from outside: {fn}", a[0] in (400, 403, 404), a)

# ---- Ben, a client, out of turn ----
check("Ben cannot read Ada's account", ben.rpc("load_account", ada_everyday) == (200, {"ok": None}), ben.rpc("load_account", ada_everyday))
a = ben.rpc("send_money", ada_everyday, ben_number, "100", "mine now")
check("Ben cannot send from Ada's account", refused(a, "not your account"), a)
a = ben.rpc("decide_transfer", 1, True, "")
check("Ben cannot decide a transfer", refused(a, "Staff only"), a)
a = ben.rpc("set_account_status", ada_everyday, "frozen")
check("Ben cannot freeze Ada's account", refused(a, "Staff only"), a)
check("Ben's load_dashboard is null", ben.rpc("load_dashboard") == (200, {"ok": None}), ben.rpc("load_dashboard"))

# ---- odd input ----
for amount in ["-5", "0", "0.001", "1e3", "12,3,4", "١٠"]:
    a = ada.rpc("send_money", ada_everyday, ben_number, amount, "odd")
    check(f"amount {amount!r} is refused", refused(a, "amount", "Enter", "digits", "decimals", "nothing"), a)
a = ada.rpc("send_money", "1", ben_number, "5", "x")
check("an account id as text is a 400", a[0] == 400, a)
a = ada.rpc("send_money", ada_everyday, ben_number, 5, "x")
check("an amount as a number, not text, is a 400", a[0] == 400, a)
a = ada.rpc("send_money", ada_everyday, ben_number, "5", "x" * 141)
check("a 141-character reference is refused", refused(a, "140"), a)
a = ada.rpc("send_money", ada_everyday, "QB00 0000 0000 0000 0000", "5", "x")
check("a bad account number is refused", refused(a, "typo", "number"), a)
a = ada.rpc("send_money", ada_everyday, "", "5", "x", raw=b'{"not": "a list"}')
check("arguments not as a list is a 400", a[0] == 400, a)

# ---- a clerk is not a supervisor ----
_, held = omar.rpc("load_dashboard")
held_ids = [t["id"] for t in held["ok"]["held"]]
a = maria.rpc("decide_transfer", held_ids[0], True, "")
check("a clerk cannot decide a held transfer", refused(a, "Supervisors"), a)

# Two supervisors decide the same held transfer at once: it is decided once.
import threading
answers = []
omar2 = Visitor("omar2")
omar2.post_form("/staff/login", {"username": "omar", "password": "password"})
def decide(v):
    answers.append(v.rpc("decide_transfer", held_ids[0], True, ""))
threads = [threading.Thread(target=decide, args=(v,)) for v in (omar, omar2, omar, omar2)]
[t.start() for t in threads]
[t.join() for t in threads]
oks = [a for a in answers if a[0] == 200 and "ok" in a[1]]
check("four approvals at once decide it once", len(oks) == 1, answers)

# ---- desk sessions ----
_, code = maria.rpc("desk_start", "open_account", "")
code = code["ok"]
phone = Visitor("phone")
other = Visitor("other")
check("a wrong code is refused", refused(phone.rpc("desk_join", "000000"), "code"), phone.rpc("desk_join", "000000"))
check("the phone joins", phone.rpc("desk_join", code)[1].get("ok") == code, phone.rpc("desk_join", code))
check("a second phone cannot join", refused(other.rpc("desk_join", code), "Another phone"), other.rpc("desk_join", code))
check("the second phone cannot read the desk", other.rpc("load_desk", code, "client") == (200, {"ok": None}), other.rpc("load_desk", code, "client"))
check("the phone cannot read the clerk's side", phone.rpc("load_desk", code, "clerk") == (200, {"ok": None}), phone.rpc("load_desk", code, "clerk"))
check("the phone cannot fill in the form", refused(phone.rpc("desk_set", code, "first_name", "Mallory"), "Staff only"), phone.rpc("desk_set", code, "first_name", "Mallory"))
check("the clerk cannot choose the PIN", refused(maria.rpc("desk_pin", code, "135790", "135790"), "not the one"), maria.rpc("desk_pin", code, "135790", "135790"))
check("the clerk cannot set a field that is not on the form", refused(maria.rpc("desk_set", code, "pin_hash", "x"), "No such field"), maria.rpc("desk_set", code, "pin_hash", "x"))
maria.rpc("desk_set", code, "first_name", "Grace")
maria.rpc("desk_set", code, "last_name", "Hopper")
maria.rpc("desk_set", code, "birth_date", "1990-12-09")
check("asking about a part with a problem is refused", refused(maria.rpc("desk_ask", code, "address"), "Street"), maria.rpc("desk_ask", code, "address"))
maria.rpc("desk_ask", code, "personal")
_, view = phone.rpc("load_desk", code, "client")
ask_id = [s for s in view["ok"]["sections"] if s["id"] == "personal"][0]["ask"]["id"]
check("another phone cannot answer", refused(other.rpc("desk_answer", ask_id, True, ""), "not the one"), other.rpc("desk_answer", ask_id, True, ""))
check("the clerk cannot answer for the client", refused(maria.rpc("desk_answer", ask_id, True, ""), "not the one"), maria.rpc("desk_answer", ask_id, True, ""))
check("a 16-year-old cannot open an account", refused((maria.rpc("desk_set", code, "birth_date", "2015-01-01"), maria.rpc("desk_ask", code, "personal"))[1], "18"), maria.rpc("desk_ask", code, "personal"))
check("submitting before everything is approved is refused", refused(maria.rpc("desk_submit", code), "Not yet"), maria.rpc("desk_submit", code))
_, ben_view = ben.rpc("load_home")
a = ben.rpc("desk_join", code)
check("a signed-in customer cannot take over a new customer's session", refused(a, "Another phone"), a)

# A session for Ben: Ada cannot join it.
_, bcode = maria.rpc("desk_start", "withdrawal", "10000002")
a = ada.rpc("desk_join", bcode["ok"])
check("Ada cannot join Ben's withdrawal", refused(a, "sign in first, as them"), a)

# ---- wrong PINs lock the sign-in ----
mallory = Visitor("mallory")
codes = [mallory.post_form("/login", {"number": "10000003", "pin": f"00000{i}"}) for i in range(5)]
check("five wrong PINs are refused", codes == [401] * 5, codes)
check("then even the right PIN is locked out", mallory.post_form("/login", {"number": "10000003", "pin": "246810"}) == 401, "signed in")
check("an unknown number says the same as a wrong PIN", Visitor("x").post_form("/login", {"number": "99999999", "pin": "246810"}) == 401, "different")

# ---- the ledger after all of it ----
health = json.loads(urllib.request.urlopen(BASE + "/health").read())
check("the ledger adds up", health["ok"] and health["ledger"] == [], health)

print(f"\n{sum(results)} of {len(results)} pass")
