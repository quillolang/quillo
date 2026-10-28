#!/usr/bin/env python3
"""Hostile calls to Quillo Stays' server fns, as each kind of visitor, and races.

    python3 app/stays/tests/hostile.py http://localhost:8080 <path of stays.db>

Needs STAYS_OFFLINE=1 STAYS_DEMO=1, freshly seeded. The database's path is for
making a hold run out without waiting 10 minutes. Prints each check and its answer."""

import http.cookiejar, json, sqlite3, sys, threading, time, urllib.request, urllib.error, urllib.parse
from datetime import date, timedelta

BASE = sys.argv[1].rstrip("/")
DB = sys.argv[2]
results = []
TODAY = date.today()


def day(n):
    return (TODAY + timedelta(days=n)).isoformat()


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None

    def http_error_303(self, req, fp, code, msg, headers):
        return fp

    http_error_302 = http_error_301 = http_error_303


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
        # The session cookie is Secure: browsers send it to http://localhost,
        # Python's cookie jar does not, so here it may.
        for c in self.jar:
            c.secure = False
        return status

    def sign_in(self, email):
        return self.post_form("/login", {"email": email, "password": "password", "next": "/"})

    def rpc(self, fn, *args):
        req = urllib.request.Request(BASE + "/_quillo/rpc/" + fn, data=json.dumps(list(args)).encode(), headers={"content-type": "application/json"})
        try:
            r = self.http.open(req)
            return json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            text = e.read().decode(errors="replace")
            try:
                return {"status": e.code, **json.loads(text)}
            except ValueError:
                return {"status": e.code, "text": text[:120]}


def check(name, ok, got):
    results.append(bool(ok))
    print(("PASS " if ok else "FAIL ") + name + ("" if ok else f"   got: {json.dumps(got)[:300]}"))


def err(r, text=""):
    return isinstance(r, dict) and "err" in r and text in str(r["err"])


nobody = Visitor("nobody")
sam = Visitor("sam")
sam.sign_in("sam@stays.test")
lee = Visitor("lee")
lee.sign_in("lee@stays.test")
ana = Visitor("ana")
ana.sign_in("ana@stays.test")
db = sqlite3.connect(DB)
q = lambda sql, *a: db.execute(sql, a).fetchall()  # noqa: E731

# ---- out of turn -------------------------------------------------------------------
for fn, args in [("hold_stay", [1, day(60), day(62), 2]), ("pay_booking", [1, {"number": "4242424242424242", "expiry": "12/30", "cvc": "123"}]), ("cancel_booking", [1]), ("set_nights", [1, [day(70)], "close", ""]),
                 ("decide_request", [1, True]), ("add_listing", [{}]), ("book_activity", [1, 1]), ("toggle_saved", [1]), ("leave_review", [1, 5, "lovely place"])]:
    check(f"signed out: {fn} refused", err(nobody.rpc(fn, *args), "Sign in first"), nobody.rpc(fn, *args))
check("signed out: load_trips is null", nobody.rpc("load_trips") == {"ok": None}, nobody.rpc("load_trips"))

lee_request = q("select id from bookings where status = 'requested'")[0][0]
lee_kyoto = q("select id from bookings where listing_id = 14 and status = 'confirmed' and start_day > ?", TODAY.isoformat())[0][0]
check("a guest cannot answer a host's request", err(sam.rpc("decide_request", lee_request, True), "Only hosts"), sam.rpc("decide_request", lee_request, True))
check("a guest cannot change a host's calendar", err(sam.rpc("set_nights", 1, [day(70)], "close", ""), "Only hosts"), None)
check("a guest cannot read a host's calendar", sam.rpc("load_calendar", 1, day(0)[:7]) == {"ok": None}, sam.rpc("load_calendar", 1, day(0)[:7]))
check("another host cannot change Ana's calendar", err(Visitor("x").rpc("set_nights", 1, [day(70)], "close", ""), ""), None)
kenji = Visitor("kenji")
kenji.sign_in("kenji@stays.test")
check("Kenji cannot change Ana's calendar", err(kenji.rpc("set_nights", 1, [day(70)], "close", ""), "Not your place"), kenji.rpc("set_nights", 1, [day(70)], "close", ""))
check("Kenji cannot answer Ana's request", err(kenji.rpc("decide_request", lee_request, True), "No such request"), None)
check("Sam cannot see Lee's booking", sam.rpc("load_booking", lee_kyoto) == {"ok": None}, sam.rpc("load_booking", lee_kyoto))
check("Sam cannot cancel Lee's booking", err(sam.rpc("cancel_booking", lee_kyoto), "No such booking"), None)
check("Sam cannot preview Lee's refund", err(sam.rpc("cancel_preview", lee_kyoto), "No such booking"), None)
check("Ana cannot book her own place", err(ana.rpc("hold_stay", 1, day(60), day(62), 2), "your own place"), ana.rpc("hold_stay", 1, day(60), day(62), 2))

# ---- dates and guests --------------------------------------------------------------
for name, args, says in [
    ("nights in the past", [3, day(-5), day(-2), 2], "passed"),
    ("leaving before arriving", [3, day(30), day(28), 2], "comes after"),
    ("no nights", [3, day(30), day(30), 2], "comes after"),
    ("30 February", [3, f"{TODAY.year + 1}-02-30", f"{TODAY.year + 1}-03-02", 2], "Choose the dates"),
    ("a day that is not a day", [3, "tomorrow", day(31), 2], "Choose the dates"),
    ("more than a year ahead", [3, day(400), day(403), 2], "a year ahead"),
    ("fewer nights than the place takes", [2, day(30), day(32), 2], "3 nights or more"),
    ("more nights than it takes", [3, day(30), day(62), 2], "at most 30 nights"),
    ("no guests", [3, day(30), day(31), 0], "At least one"),
    ("more guests than beds", [3, day(30), day(31), 9], "sleeps 2 guests"),
    ("a night the host closed", [1, day(41), day(44), 2], "closed the night"),
    ("a night that is booked", [1, day(16), day(19), 2], "just booked"),
    ("no such place", [999, day(30), day(31), 2], "not taking bookings"),
]:
    r = sam.rpc("hold_stay", *args)
    check(f"hold: {name}", err(r, says), r)
r = sam.rpc("hold_stay", "1", day(30), day(31), 2)
check("hold: an id as text is a 400, by the fn's types", r.get("status") == 400, r)
r = sam.rpc("hold_stay", 1, day(30), day(31))
check("hold: an argument missing is a 400", r.get("status") == 400, r)

# ---- a hold: paying, running out ----------------------------------------------------
h = sam.rpc("hold_stay", 3, day(50), day(53), 2)
check("Sam holds three nights", "ok" in h, h)
hid = h.get("ok")
r = lee.rpc("hold_stay", 3, day(52), day(55), 2)
check("Lee cannot hold a night Sam holds", err(r, "just booked"), r)
for name, card, says in [
    ("a typo", {"number": "4242 4242 4242 4241", "expiry": "12/30", "cvc": "123"}, "typo"),
    ("declined", {"number": "4000 0000 0000 0002", "expiry": "12/30", "cvc": "123"}, "declined"),
    ("expired", {"number": "4242424242424242", "expiry": f"01/{(TODAY.year - 2001) % 100:02d}", "cvc": "123"}, "expired"),
    ("month 13", {"number": "4242424242424242", "expiry": "13/30", "cvc": "123"}, "MM/YY"),
    ("no cvc", {"number": "4242424242424242", "expiry": "12/30", "cvc": ""}, "3 digits"),
    ("no cvc field at all", {"number": "4242424242424242", "expiry": "12/30"}, "-"),
    ("a card number as a number", {"number": 4242424242424242, "expiry": "12/30", "cvc": "123"}, ""),
]:
    r = sam.rpc("pay_booking", hid, card)
    check(f"pay: {name} refused", err(r, says) or r.get("status") == 400, r)
check("Lee cannot pay for Sam's hold", err(lee.rpc("pay_booking", hid, {"number": "4242424242424242", "expiry": "12/30", "cvc": "123"}), "No such booking"), None)
db.execute("update bookings set held_until = ? where id = ?", (int(time.time() * 1000) - 1000, hid))
db.commit()
r = sam.rpc("pay_booking", hid, {"number": "4242424242424242", "expiry": "12/30", "cvc": "123"})
check("pay: a hold that ran out is refused", err(r, "ran out"), r)
check("its nights are free again", q("select count(*) from booking_nights where booking_id = ?", hid)[0][0] == 0, q("select * from booking_nights where booking_id = ?", hid))
r = lee.rpc("hold_stay", 3, day(52), day(55), 2)
check("so Lee can hold them now", "ok" in r, r)
lid = r.get("ok")
r = lee.rpc("pay_booking", lid, {"number": "4242-4242-4242-4242", "expiry": "12 / 30", "cvc": "123"})
check("Lee pays (instant book)", r == {"ok": "confirmed"}, r)
r = lee.rpc("pay_booking", lid, {"number": "4242424242424242", "expiry": "12/30", "cvc": "123"})
check("paying twice is refused", err(r, "paid for already"), r)
bk = q("select total, breakdown, card_last4 from bookings where id = ?", lid)[0]
check("the total kept is the quote's", json.loads(bk[1])["total"] == bk[0] and bk[2] == "4242", bk)

# ---- cancelling ---------------------------------------------------------------------
p = lee.rpc("cancel_preview", lid)
check("flexible, 52 days ahead: all back", p.get("ok", {}).get("minor") == bk[0], p)
r = lee.rpc("cancel_booking", lid)
check("cancelled, with all of it back", r.get("ok", {}).get("minor") == bk[0], r)
check("cancelling twice is refused", err(lee.rpc("cancel_booking", lid), "not one you can cancel"), None)
check("the nights are free again", q("select count(*) from booking_nights where booking_id = ?", lid)[0][0] == 0, None)

# ---- reviews ------------------------------------------------------------------------
past = q("select b.id from bookings b left join reviews r on r.booking_id = b.id where b.guest_id = 5 and b.status = 'confirmed' and b.end_day <= ? and r.id is null", TODAY.isoformat())
coming = q("select id from bookings where guest_id = 5 and status = 'confirmed' and start_day > ?", TODAY.isoformat())[0][0]
check("a review before the stay ends is refused", err(sam.rpc("leave_review", coming, 5, "Looking forward to it!"), "day you leave"), None)
reviewed = q("select booking_id from reviews where guest_id = 5")[0][0]
check("a second review of a stay is refused", err(sam.rpc("leave_review", reviewed, 5, "Even better the second time."), "already"), sam.rpc("leave_review", reviewed, 5, "Even better the second time."))
check("Lee cannot review Sam's stay", err(lee.rpc("leave_review", reviewed, 1, "I was never there at all."), "Only a stay you made"), None)
check("six stars are refused", err(sam.rpc("leave_review", coming, 6, "Six stars for this one"), ""), None)

# ---- the host's calendar -------------------------------------------------------------
booked = q("select day from booking_nights where listing_id = 1 and day > ? order by day limit 2", TODAY.isoformat())
r = ana.rpc("set_nights", 1, [booked[0][0], booked[1][0], day(90)], "close", "")
check("closing booked nights leaves them be", r == {"ok": {"changed": 1, "skipped": 2}}, r)
r = ana.rpc("set_nights", 1, [day(91)], "price", "95.123")
check("a price with too many decimals for euros is refused", err(r, "in EUR"), r)
r = kenji.rpc("set_nights", 14, [day(91)], "price", "18000.50")
check("yen with cents are refused", err(r, "in JPY"), r)
r = kenji.rpc("set_nights", 14, [day(91)], "price", "18000")
check("whole yen are taken", r == {"ok": {"changed": 1, "skipped": 0}}, r)
r = ana.rpc("set_nights", 1, [day(i) for i in range(0, 400)], "close", "")
check("more than a year at once is refused", err(r, "a year"), r)
r = ana.rpc("set_nights", 1, ["drop table listings", day(92)], "close", "")
check("days that are not days are left out", r == {"ok": {"changed": 1, "skipped": 0}}, r)
r = ana.rpc("set_nights", 1, [day(93)], "explode", "")
check("an unknown action is refused", err(r, ""), r)

# ---- races --------------------------------------------------------------------------
guests = []
for i in range(20):
    v = Visitor(f"g{i}")
    v.post_form("/signup", {"name": f"Racer {i}", "email": f"racer{i}-{int(time.time())}@stays.test", "password": "password1", "next": "/"})
    guests.append(v)
answers = [None] * 20
start = threading.Barrier(20)
def race(i):
    start.wait()
    answers[i] = guests[i].rpc("hold_stay", 5, day(70), day(74), 2)
threads = [threading.Thread(target=race, args=(i,)) for i in range(20)]
t0 = time.time()
[t.start() for t in threads]
[t.join() for t in threads]
won = [a for a in answers if "ok" in a]
lost = [a for a in answers if err(a, "just booked")]
check(f"20 holds of the same nights at once: one wins ({time.time() - t0:.2f}s)", len(won) == 1 and len(lost) == 19, answers)
check("and the database has its nights once", q("select count(*) from booking_nights where listing_id = 5 and day >= ? and day < ?", day(70), day(74))[0][0] == 4, None)

slot = q("select id, capacity, taken from activity_slots where day = ? order by id limit 1", day(3))[0]
left = slot[1] - slot[2]
answers = [None] * 20
start = threading.Barrier(20)
def race_slot(i):
    start.wait()
    answers[i] = guests[i].rpc("book_activity", slot[0], 1)
threads = [threading.Thread(target=race_slot, args=(i,)) for i in range(20)]
[t.start() for t in threads]
[t.join() for t in threads]
took = q("select taken, capacity from activity_slots where id = ?", slot[0])[0]
check(f"20 people at once for {left} places: {left} get one", len([a for a in answers if "ok" in a]) == min(left, 20) and took[0] <= took[1], (answers, took))
r = sam.rpc("book_activity", slot[0], 1)
check("then it is full", err(r, "full") or (left > 20), r)
check("eleven people at once are refused", err(sam.rpc("book_activity", slot[0], 11), "1 to 10"), None)

# ---- searching ----------------------------------------------------------------------
for name, f in [("an empty place", {"where": ""}), ("markup", {"where": "<script>alert(1)</script>"}), ("a long text", {"where": "x" * 5000}),
                ("guests as text", {"where": "Lisbon", "guests": "many"}), ("dates as numbers", {"where": "Lisbon", "start": 5, "end": 9}),
                ("kinds not a list", {"where": "Lisbon", "kinds": "house"})]:
    r = nobody.rpc("search_stays", f)
    check(f"search: {name} does not fail", "ok" in r, r)
r = nobody.rpc("search_stays", {"where": "Lisbon", "start": day(14), "end": day(18), "guests": 2})
check("search: a place booked on those dates is left out", "ok" in r and 1 not in [l["id"] for l in r["ok"]["results"]], [l["id"] for l in r.get("ok", {}).get("results", [])])

print(f"\n{sum(results)}/{len(results)} passed")
sys.exit(0 if all(results) else 1)
