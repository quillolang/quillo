"""What the Python tests share: a signed-in visitor, and checks."""

import http.cookiejar, json, urllib.request, urllib.error, urllib.parse

results = []


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *a, **k):
        return None

    def http_error_303(self, req, fp, code, msg, headers):
        return fp

    http_error_302 = http_error_301 = http_error_303


class Visitor:
    def __init__(self, base, name=None, password="coop1234"):
        self.base = base
        self.jar = http.cookiejar.CookieJar()
        self.http = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar), NoRedirect)
        if name:
            self.post_form("/login", {"username": name, "password": password, "next": "/"})

    def _plain(self):
        # The session cookie is Secure; Python's jar would not send it over plain HTTP.
        for c in self.jar:
            c.secure = False

    def post_form(self, path, fields):
        try:
            status = self.http.open(urllib.request.Request(self.base + path, data=urllib.parse.urlencode(fields).encode())).status
        except urllib.error.HTTPError as e:
            status = e.code
        self._plain()
        return status

    def rpc(self, fn, *args):
        req = urllib.request.Request(self.base + "/_quillo/rpc/" + fn, data=json.dumps(list(args)).encode(), headers={"content-type": "application/json"})
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
