"""Assemble the Quillo book's chapters from USERGUIDE.md, README.md and the website's guides.

Writes book/chapters/NN-*.md for the generated chapters; the hand-written ones
(preface, part pages, appendix A, about the author) are left alone but their
headings take part in the ID check. Prints links it could not resolve.
"""
import re, sys, os, glob

APP = os.path.expanduser("~/dev/quillo-app1")
SITE = os.path.expanduser("~/dev/quillo-website")
OUT = os.path.join(APP, "book/chapters")
EXAMPLES = "https://github.com/quillolang/examples"

guide = open(f"{APP}/USERGUIDE.md").read().split("\n")
readme = open(f"{APP}/README.md").read().split("\n")

def gh_slug(t):
    t = re.sub(r"`", "", t).lower()
    return "".join(c if c.isalnum() or c in "-_" else "-" if c == " " else "" for c in t)

def pandoc_id(t):
    t = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", t)
    t = re.sub(r"[`*_](?=\S)|(?<=\S)[`*_]", "", t)
    t = "".join(c if c.isalnum() or c in "_-." else "-" if c.isspace() else "" for c in t).lower()
    t = re.sub(r"^[^a-z]+", "", t)
    return t or "section"

def headings(lines):
    """(index, level, text) for each heading outside code fences."""
    out, fence = [], False
    for i, l in enumerate(lines):
        if l.startswith("```"):
            fence = not fence
        elif not fence:
            m = re.match(r"(#+) (.*?)\s*$", l)
            if m:
                out.append((i, len(m.group(1)), m.group(2)))
    return out

# --- the guide's pages: each `## ` section, and the part before the first ---
gh = headings(guide)
tops = [h for h in gh if h[1] == 2]
pages = {}          # page slug -> lines (as in the guide)
starts = [(0, "introduction")] + [(i, gh_slug(t)) for i, _, t in tops]
for n, (i, s) in enumerate(starts):
    end = starts[n + 1][0] if n + 1 < len(starts) else len(guide)
    pages[s] = guide[i:end]

def site_guide(name):
    return open(f"{SITE}/guides/{name}.md").read().rstrip("\n").split("\n")

def shift(lines, by):
    """Change heading levels by `by` (outside code fences)."""
    out, fence = [], False
    for l in lines:
        if l.startswith("```"):
            fence = not fence
        elif not fence:
            m = re.match(r"(#+) ", l)
            if m:
                l = "#" * max(1, len(m.group(1)) + by) + l[len(m.group(1)):]
        out.append(l)
    return out

def replace_once(lines, old, new):
    text = "\n".join(lines)
    if old not in text:
        sys.exit(f"text to replace not found: {old!r}")
    return text.replace(old, new, 1).split("\n")

# --- chapters: (file, key, source page or guide, lines) ---
chapters = []

def add(file, key, lines, src):
    chapters.append({"file": file, "key": key, "lines": lines, "src": src})

# 1. Introduction: the README's opening, features and performance
rm = readme
a = rm.index("---") + 1
b = next(i for i, l in enumerate(rm) if l == "## Contents")
c = next(i for i, l in enumerate(rm) if l == "## Features")
d = next(i for i, l in enumerate(rm) if l == "## Getting started")
intro = ["# Introduction", ""] + rm[a:b] + shift(rm[c + 1:d], -1)
intro = ["## Performance" if l == "# Performance" else l for l in intro]
add("11-introduction.md", "introduction", intro, ("readme", None))

add("12-installing.md", "install", replace_once(replace_once(shift(site_guide("install"), -1),
    "This page lists", "This chapter lists"),
    "The rest of this page is for", "The rest of this chapter is for"), ("guide", "install"))

add("13-possible-or-not.md", "possible-or-not", shift(pages["possible-or-not"], -1), ("page", "possible-or-not"))
add("14-language-tour.md", "language-tour", shift(pages["language-tour"], -1), ("page", "language-tour"))
add("21-components.md", "components", shift(pages["components"], -1), ("page", "components"))
add("22-reactivity.md", "reactivity", shift(pages["reactivity"], -1), ("page", "reactivity"))
add("23-async.md", "async", shift(site_guide("async"), -1), ("guide", "async"))
add("31-routes.md", "routes", shift(pages["routes"], -1), ("page", "routes"))
add("32-route-groups.md", "route-groups", replace_once(shift(site_guide("route-groups"), -1),
    "Everything on this page is one program", "Everything in this chapter is one program"), ("guide", "route-groups"))
add("33-headers-cookies-sessions.md", "headers-cookies-sessions", replace_once(shift(site_guide("headers-cookies-sessions"), -1),
    "This page shows each of them", "This chapter shows each of them"), ("guide", "headers-cookies-sessions"))
hc = chapters[-1]
hc["lines"] = replace_once(hc["lines"], "](#requiring-a-session)", "](#sessions)")   # a bold paragraph on the site, not a heading
add("34-realtime.md", "realtime", shift(site_guide("realtime"), -1), ("guide", "realtime"))
pf = shift(site_guide("prefetching"), -1)
pf = replace_once(pf, "> **Try it here.** This site is a Quillo program, and its pages prefetch.",
    "> **Try it on quillolang.org.** The site is a Quillo program, and its pages prefetch.")
add("35-prefetching.md", "prefetching", pf, ("guide", "prefetching"))
add("36-validation.md", "validation", replace_once(shift(site_guide("validation"), -1),
    "Everything on this page is one program", "Everything in this chapter is one program"), ("guide", "validation"))
add("41-database.md", "database", replace_once(shift(site_guide("database"), -1),
    "This page goes through it", "This chapter goes through it"), ("guide", "database"))
add("42-money.md", "money", replace_once(shift(site_guide("money"), -1),
    "This page builds", "This chapter builds"), ("guide", "money"))
st = shift(site_guide("structure"), -1)
st = replace_once(st, "on this page, and each one", "in this chapter, and each one")
add("51-structure.md", "structure", st, ("guide", "structure"))
add("52-microservices.md", "microservices", shift(site_guide("microservices"), -1), ("guide", "microservices"))
add("53-rust.md", "rust", shift(pages["rust"], -1), ("page", "rust"))
add("54-hosting.md", "hosting", shift(site_guide("hosting"), -1), ("guide", "hosting"))
add("55-shared-hosting.md", "shared-hosting", replace_once(shift(site_guide("shared-hosting"), -1),
    "This page covers how", "This chapter covers how"), ("guide", "shared-hosting"))

# Reference: the guide's opening (commands, options, settings, editors) as "Running quillo"
run = pages["introduction"]
k = next(i for i, l in enumerate(run) if l.startswith("Quillo is a single binary"))
e = next(i for i, l in enumerate(run) if l.startswith("Examples, in [quillolang/examples]"))
run = ["# Running quillo", ""] + shift(run[k:e], -1)
while run[-1] == "":
    run.pop()
add("61-running-quillo.md", "running-quillo", run, ("page", "introduction"))
add("62-standard-library.md", "standard-library", shift(pages["standard-library"], -1), ("page", "standard-library"))
add("63-checking.md", "checking", shift(pages["checking"], -1), ("page", "checking"))
add("64-concurrency-and-reloading.md", "concurrency-and-reloading", shift(pages["concurrency-and-reloading"], -1), ("page", "concurrency-and-reloading"))
add("65-cached-views.md", "cached-views", shift(pages["cached-views"], -1), ("page", "cached-views"))
add("66-compiled-numbers.md", "compiled-numbers", shift(pages["compiled-numbers"], -1), ("page", "compiled-numbers"))

nb = shift(pages["not-built-yet"], -1)
nb[0] = "# Appendix B: Not built yet"
add("72-appendix-not-built-yet.md", "not-built-yet", nb, ("page", "not-built-yet"))
ly = shift(pages["layout"], -1)
ly[0] = "# Appendix C: How Quillo is built"
add("73-appendix-source-layout.md", "layout", ly, ("page", "layout"))

# The "options at the start of this guide" are now a chapter of their own
po = next(ch for ch in chapters if ch["key"] == "possible-or-not")
po["lines"] = replace_once(po["lines"], "See the options at the start of this guide",
    "See the options in [Running quillo](#running-quillo)")

# --- IDs: chapter titles first, then sections in book order; collisions made explicit ---
handwritten = sorted(set(glob.glob(f"{OUT}/*.md")) - {f"{OUT}/{ch['file']}" for ch in chapters})
hand = [{"file": os.path.basename(p), "key": None, "lines": open(p).read().split("\n"), "src": None} for p in handwritten]
book = sorted(chapters + hand, key=lambda ch: ch["file"])

taken = set()
explicit = []
for ch in book:
    ch["heads"] = []
    for i, lvl, text in headings(ch["lines"]):
        m = re.search(r"\s*\{#([^}\s]+)[^}]*\}$", text)
        if m:
            ch["heads"].append({"i": i, "lvl": lvl, "text": text[:m.start()], "id": m.group(1), "fixed": True})
        else:
            ch["heads"].append({"i": i, "lvl": lvl, "text": text, "id": None, "fixed": False})
for ch in book:      # chapter titles
    for h in ch["heads"]:
        if h["lvl"] == 1:
            if not h["fixed"]:
                h["id"] = pandoc_id(re.sub(r"^(Appendix [A-Z]|Part [IVX]+): ", "", h["text"])) if ch["key"] is None else ch["key"]
                if h["id"] != pandoc_id(h["text"]):
                    h["fixed"] = True
            assert h["id"] not in taken, h
            taken.add(h["id"])
for ch in book:      # sections
    for h in ch["heads"]:
        if h["lvl"] > 1 and not h["fixed"]:
            base = pandoc_id(h["text"])
            if base in taken:
                h["id"], h["fixed"] = f"{ch['key'] or 'x'}-{base}", True
                explicit.append((ch["file"], h["text"], h["id"]))
            else:
                h["id"] = base
        taken.add(h["id"])

# --- where the old links pointed: (page, github slug) -> book id ---
target = {}
def remember(page, ch):
    seen = {}
    for h in ch["heads"]:
        s = gh_slug(h["text"])
        n = seen.get(s, 0)
        seen[s] = n + 1
        target.setdefault((page, s if n == 0 else f"{s}-{n}"), h["id"])
for ch in chapters:
    kind, name = ch["src"]
    if kind in ("page", "guide"):
        remember(name, ch)
        target[(name, "")] = ch["heads"][0]["id"]
target[("introduction", "")] = "running-quillo"
# the guide's own `#anchor` links cover the whole guide, whatever page
whole = {}
for (page, s), i in target.items():
    if page in pages or page == "introduction":
        whole.setdefault(s, i)

EX_FILES = {"services": f"{EXAMPLES}/tree/main/services", "rust": f"{EXAMPLES}/blob/main/rust.qwer"}
unresolved = []

def resolve(url, ch):
    if url.startswith("#"):
        s = url[1:]
        kind, name = ch["src"] or ("", "")
        if kind == "guide":
            return "#" + target.get((name, s)) if (name, s) in target else None
        return "#" + whole[s] if s in whole else None
    if url in ("README.md",):
        return "#introduction"
    m = re.match(r"/docs/examples/([^/#]+)/?(#.*)?$", url)
    if m:
        return EX_FILES.get(m.group(1), f"{EXAMPLES}/blob/main/{m.group(1)}.qwe")
    if url.startswith("/docs/reference/"):
        return "https://quillolang.org" + url
    m = re.match(r"/docs/([^#]*?)/?(?:#(.*))?$", url)
    if m:
        page = m.group(1) or "introduction"
        s = m.group(2) or ""
        if (page, s) in target:
            return "#" + target[(page, s)]
        if s in whole:
            return "#" + whole[s]
        return None
    return url

def relink(line, ch):
    def sub(m):
        new = resolve(m.group(2), ch)
        if new is None:
            unresolved.append((ch["file"], m.group(2)))
            return m.group(0)
        return f"]({new})"
    return re.sub(r"\]\(()([^)\s]+)\)", sub, line)

for ch in book:
    if ch["key"] is None:
        continue
    lines = ch["lines"]
    for h in ch["heads"]:
        if h["fixed"]:
            lines[h["i"]] = "#" * h["lvl"] + " " + h["text"] + " {#" + h["id"] + "}"
    fence = False
    for i, l in enumerate(lines):
        if l.startswith("```"):
            fence = not fence
        elif not fence:
            lines[i] = relink(l, ch)
    while lines and lines[-1] == "":
        lines.pop()
    open(f"{OUT}/{ch['file']}", "w").write("\n".join(lines) + "\n")

# links in the hand-written files must point at real IDs
for ch in hand:
    for l in ch["lines"]:
        for u in re.findall(r"\]\((#[^)\s]+)\)", l):
            if u[1:] not in taken:
                unresolved.append((ch["file"], u))

for f, t, i in explicit:
    print(f"explicit id  {f}: {t} -> {i}")
for f, u in unresolved:
    print(f"UNRESOLVED   {f}: {u}")
print(f"{len(chapters)} chapters written, {len(hand)} hand-written files, {len(unresolved)} unresolved")
