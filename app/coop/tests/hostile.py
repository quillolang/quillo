#!/usr/bin/env python3
"""Hostile calls to Quillo Coop's server fns: each role calling what is not
theirs, another member's records, amounts and prices tampered with, votes and
releases made twice, and two clerks approving one applicant at once.

    python3 app/coop/tests/hostile.py http://127.0.0.1:8080 <path of coop.db>

Needs COOP_DEMO=1, freshly seeded. Prints each check and its answer."""

import http.cookiejar, json, sqlite3, sys, threading, urllib.request, urllib.error, urllib.parse

BASE = sys.argv[1].rstrip("/")
DB = sys.argv[2]
results = []


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None

    def http_error_303(self, req, fp, code, msg, headers):
        return fp

    http_error_302 = http_error_301 = http_error_303


class Visitor:
    def __init__(self, name=None):
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar), NoRedirect)
        if name:
            self.post_form("/login", {"username": name, "password": "coop1234", "next": "/"})

    def _plain(self):
        for c in self.jar:
            c.secure = False

    def post_form(self, path, fields):
        try:
            status = self.http.open(urllib.request.Request(BASE + path, data=urllib.parse.urlencode(fields).encode())).status
        except urllib.error.HTTPError as e:
            status = e.code
        self._plain()
        return status

    def get(self, path):
        try:
            r = self.http.open(BASE + path)
            return r.status, r.read().decode(errors="replace")
        except urllib.error.HTTPError as e:
            return e.code, e.read().decode(errors="replace")

    def rpc(self, fn, *args):
        req = urllib.request.Request(BASE + "/_quillo/rpc/" + fn, data=json.dumps(list(args)).encode(), headers={"content-type": "application/json"})
        try:
            r = self.http.open(req)
            self._plain()
            return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            text = e.read().decode(errors="replace")
            try:
                return e.code, json.loads(text)
            except ValueError:
                return e.code, text[:160]


def check(name, ok, got=None):
    results.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"   got: {got}"))


def refused(answer):
    status, body = answer
    return status >= 400 or (isinstance(body, dict) and "err" in body)


def err_text(answer):
    status, body = answer
    return body.get("err") if isinstance(body, dict) else body


def one(sql, *args):
    con = sqlite3.connect(DB)
    try:
        return con.execute(sql, args).fetchone()
    finally:
        con.close()


nobody = Visitor()
ana = Visitor("M-0001")       # employed at Acme; has a released salary loan L-00004
ben = Visitor("M-0002")       # L-00002 waits for Ana as co-maker
carla = Visitor("M-0003")
clara = Visitor("clara")
mario = Visitor("mario")
cruz = Visitor("dir.cruz")
acme = Visitor("acme.hr")
ltel = Visitor("ltel.hr")

loan2 = one("select id from loans where number = 'L-00002'")[0]
loan1 = one("select id from loans where number = 'L-00001'")[0]
loan4 = one("select id from loans where number = 'L-00004'")[0]
iris = one("select id from members where status = 'applied'")[0]
prov = one("select id from products where code = 'PROV'")[0]

# Signed out.
for fn, args in [("load_my_coop", []), ("apply_loan", [{"product": "EMR", "amount": "1000", "term": 1, "purpose": "anything here", "repay": "counter"}]),
                 ("counter_cash", [{"member": "M-0001", "account": "savings", "direction": "in", "amount": "1"}]), ("approve_member", [iris])]:
    a = nobody.rpc(fn, *args)
    check(f"signed out: {fn} gives nothing", refused(a) or a[1] in (None, {"ok": None}), a)

# Members calling the office's, the board's and the manager's.
for fn, args in [("approve_member", [iris]), ("reject_member", [iris, "no"]), ("review_loan", [loan2, "forward", ""]), ("vote_loan", [loan1, "approve", ""]),
                 ("release_loan", [loan1]), ("counter_cash", [{"member": "M-0003", "account": "savings", "direction": "out", "amount": "100"}]),
                 ("counter_payment", [loan4, "100"]), ("save_product", [None, {"code": "X1", "name": "x", "min_amount": "1", "max_amount": "2", "min_term": 1, "max_term": 2, "rate": "1", "method": "flat"}]),
                 ("vote_product", [prov, "approve", ""]), ("make_batch", [1, "2026-11"]), ("new_api_key", [1]), ("draw_raffle", [1]), ("mark_order", [1, "ready"]),
                 ("check_in", [1, "AAAAAA"]), ("create_raffle", [{"name": "x", "prize": "x", "ticket_price": "1", "max_tickets": 10, "closes": "2099-01-01"}])]:
    check(f"a member: {fn} refused", refused(ana.rpc(fn, *args)), ana.rpc(fn, *args))

# A clerk is not a director, nor the manager; a director is not the office.
check("a clerk cannot vote on a loan", refused(clara.rpc("vote_loan", loan1, "approve", "")))
check("a clerk cannot design products", refused(clara.rpc("save_product", None, {"code": "X2", "name": "x", "min_amount": "1", "max_amount": "2", "min_term": 1, "max_term": 2, "rate": "1", "method": "flat"})))
check("a director cannot release a loan", refused(cruz.rpc("release_loan", loan1)))
check("a director cannot take cash", refused(cruz.rpc("counter_cash", {"member": "M-0001", "account": "savings", "direction": "in", "amount": "100"})))
check("payroll staff cannot approve members", refused(acme.rpc("approve_member", iris)))

# Another member's records.
check("Carla cannot read Ana's loan", carla.rpc("load_loan", "L-00004")[1] == {"ok": None}, carla.rpc("load_loan", "L-00004"))
check("but Ana, a co-maker, may read Ben's", (ana.rpc("load_loan", "L-00002")[1].get("ok") or {}).get("loan", {}).get("number") == "L-00002")
check("Carla cannot pay Ana's loan from her savings", refused(carla.rpc("pay_from_savings", loan4, "100")), carla.rpc("pay_from_savings", loan4, "100"))
check("Carla cannot answer for Ana as co-maker", refused(carla.rpc("answer_comaker", loan2, True)))
check("Carla cannot withdraw Ben's application", refused(carla.rpc("cancel_loan", loan2)))
s, page = carla.get("/me/accounts/SV-0001")
check("Carla cannot open Ana's statement", s == 404, s)
check("a member gets no batch", ana.rpc("load_batch", 1)[1] == {"ok": None})
check("a member sees no office queue", ana.rpc("load_office_loans")[1] == {"ok": []})
check("a member sees no board agenda", ana.rpc("load_board")[1] == {"ok": None})

# Amounts and records tampered with.
for amount in ["-500", "0", "1e3", "12.345", "abc", "", "99999999999999999999"]:
    check(f"cash of {amount!r} refused", refused(clara.rpc("counter_cash", {"member": "M-0003", "account": "savings", "direction": "in", "amount": amount})))
check("a share capital withdrawal is refused", "paid back only" in str(err_text(clara.rpc("counter_cash", {"member": "M-0003", "account": "share", "direction": "out", "amount": "100"}))))
before = one("select balance from accounts where number = 'SV-0003'")[0]
check("savings cannot go below the least kept", refused(clara.rpc("counter_cash", {"member": "M-0003", "account": "savings", "direction": "out", "amount": str(before / 100)})))
check("and nothing moved", one("select balance from accounts where number = 'SV-0003'")[0] == before)
check("an extra field in a record is left out", refused(clara.rpc("counter_cash", {"member": "M-0003", "account": "loan", "direction": "in", "amount": "100"})))

rice = one("select id, member_price, stock from items where sku = 'RICE-25'")
agri_item = one("select id from items where sku = 'BOLO'")[0]
a = carla.rpc("place_order", {"store": "mart", "lines": [{"item": rice[0], "qty": 1, "price": 1}], "pay": "savings"})
check("a price sent by the page is not taken", not refused(a) and one("select total from orders where number = ?", a[1]["ok"])[0] == rice[1], a)
check("an item from another store is refused", refused(carla.rpc("place_order", {"store": "mart", "lines": [{"item": agri_item, "qty": 1}], "pay": "savings"})))
check("a negative quantity orders nothing", refused(carla.rpc("place_order", {"store": "mart", "lines": [{"item": rice[0], "qty": -5}], "pay": "savings"})))
check("more than the stock is refused", "left" in str(err_text(carla.rpc("place_order", {"store": "mart", "lines": [{"item": rice[0], "qty": 99}], "pay": "savings"}))))
check("salary installments without an employer are refused", refused(Visitor("M-0005").rpc("place_order", {"store": "mart", "lines": [{"item": rice[0], "qty": 1}], "pay": "salary", "installments": 3})))
check("7 installments are refused", refused(ana.rpc("place_order", {"store": "mart", "lines": [{"item": rice[0], "qty": 1}], "pay": "salary", "installments": 7})))

# Loans out of turn.
check("too much for a product is refused", "from ₱1,000.00 to ₱30,000.00" in str(err_text(carla.rpc("apply_loan", {"product": "EMR", "amount": "50000", "term": 6, "purpose": "testing limits", "repay": "counter"}))))
check("a retired or proposed product is refused", refused(carla.rpc("apply_loan", {"product": "PROV", "amount": "20000", "term": 12, "purpose": "testing limits", "repay": "counter", "comakers": ["M-0001"]})))
hugo = Visitor("M-0008")
check("one's own co-maker is refused", "your own co-maker" in str(err_text(hugo.rpc("apply_loan", {"product": "SAL", "amount": "20000", "term": 12, "purpose": "testing limits", "repay": "salary", "comakers": ["M-0008"]}))), hugo.rpc("apply_loan", {"product": "SAL", "amount": "20000", "term": 12, "purpose": "testing limits", "repay": "salary", "comakers": ["M-0008"]}))
check("salary deductions without an employer are refused", refused(Visitor("M-0005").rpc("apply_loan", {"product": "EMR", "amount": "5000", "term": 3, "purpose": "testing limits", "repay": "salary"})))
# Gina has 3 months: enough for an emergency loan (3), not a salary loan (6).
check("a new member cannot borrow before the months needed", "months of membership" in str(err_text(Visitor("M-0007").rpc("apply_loan", {"product": "SAL", "amount": "5000", "term": 3, "purpose": "testing limits", "repay": "counter", "comakers": ["M-0001"]}))))
check("forwarding before the co-maker agrees is refused", "has not agreed" in str(err_text(clara.rpc("review_loan", loan2, "forward", ""))))
check("rejecting without a reason is refused", refused(clara.rpc("review_loan", loan2, "reject", "")))
check("releasing a loan before the board is refused", refused(clara.rpc("release_loan", loan1)))
check("a director's vote counts once", not refused(cruz.rpc("vote_loan", loan1, "approve", "again")) and one("select count(*) from loan_votes where loan_id = ? and director_id = (select id from users where username = 'dir.cruz')", loan1)[0] == 1)
check("paying more than is owed is refused", "is owed" in str(err_text(clara.rpc("counter_payment", loan4, "999999"))))

# Payroll: one employer's batch is not another's.
b = clara.rpc("make_batch", 1, "2026-11")
check("the clerk makes Acme's batch", not refused(b), b)
batch = b[1]["ok"]["id"]
check("Luzon's staff cannot see Acme's batch", ltel.rpc("load_batch", batch)[1] == {"ok": None})
s, _ = ltel.get(f"/payroll/{batch}/csv")
check("nor download it", s == 404, s)
check("a bad month is refused", refused(clara.rpc("make_batch", 1, "2026-13")))

# Raffles and events.
r = carla.rpc("buy_tickets", 1, 21)
check("more tickets than one may buy are refused", "At most 20 tickets each" in str(err_text(r)), r)
check("a draw before sales close is refused", "draw it after" in str(err_text(clara.rpc("draw_raffle", 1))))
ev = one("select id from events where price > 0")[0]
check("a place is booked", not refused(carla.rpc("book_event", ev)))
check("not twice", "already" in str(err_text(carla.rpc("book_event", ev))))

# Two clerks approve one applicant at the same time: one member number, one sign-in.
clerk2 = Visitor("clara")
answers = []
def approve(v):
    answers.append(v.rpc("approve_member", iris))
threads = [threading.Thread(target=approve, args=(v,)) for v in (clara, clerk2)]
for t in threads: t.start()
for t in threads: t.join()
won = [a for a in answers if not refused(a)]
check("two clerks at once: one approval", len(won) == 1, answers)
check("one member number given", one("select count(*) from members where id = ? and number is not null", iris)[0] == 1)
check("one sign-in made", one("select count(*) from users where member_id = ?", iris)[0] == 1)

# The API.
def api(body, key, idem):
    req = urllib.request.Request(BASE + "/api/remittances", data=json.dumps(body).encode(),
        headers={"content-type": "application/json", "authorization": f"Bearer {key}", "idempotency-key": idem})
    try:
        r = urllib.request.urlopen(req)
        return r.status, json.loads(r.read())
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:200]
check("the API without a key", api({"period": "2026-11", "lines": []}, "", "k1")[0] == 401)
check("the API with a made-up key", api({"period": "2026-11", "lines": []}, "coop_guess", "k2")[0] == 401)

print(f"\n{sum(results)}/{len(results)} passed")
