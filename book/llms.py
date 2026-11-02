"""Write quillolang.org's llms.txt and llms-full.txt, for AI coding assistants.

llms.txt is book/llms.md (written by hand) and every builtin, one line each, as
src/builtin_docs.rs gives them to editors. llms-full.txt adds the book's chapters, all
but the copyright page, the preface, the part pages and the author's page. Both go to
the website's folder, which serves them at /llms.txt and /llms-full.txt.

Run it whenever llms.md, a builtin or a chapter changes, as assemble.py is run. It
writes nothing unless every example in llms.md passes `quillo check` ($QUILLO, else
target/release/quillo).

`python3 llms.py v0.6.0` writes a release's own, from that tag, into the website's
v0.6.0/ folder, served at /v0.6.0/llms.txt and /v0.6.0/llms-full.txt (two routes in the
site's main.qwe for each release): an assistant on a
release reads what that release has (FEEDBACK46.md, Q-533). A tag with book/llms.md is
written as above, its examples already checked when it was made; an older one, which has
none (0.6.0), is its USERGUIDE.md and the builtins its check.rs knows, with their types.
"""
import re, os, glob, sys, shutil, subprocess, tempfile

APP = os.path.expanduser("~/dev/quillo-app1")
SITE = os.path.expanduser("~/dev/quillo-website")
RELEASE = sys.argv[1] if len(sys.argv) > 1 else None
OUT, LINK = SITE, "https://quillolang.org"
if RELEASE:
    if not re.fullmatch(r"v\d+\.\d+\.\d+", RELEASE):
        sys.exit(f"llms.py: `{RELEASE}` is not a release tag (v0.6.0)")
    TAG_DIR = tempfile.mkdtemp(prefix="llms-tag-")
    archive = subprocess.run(["git", "-C", APP, "archive", RELEASE], capture_output=True, check=True).stdout
    subprocess.run(["tar", "-x", "-C", TAG_DIR], input=archive, check=True)
    APP, OUT, LINK = TAG_DIR, f"{SITE}/{RELEASE}", f"https://quillolang.org/{RELEASE}"
    os.makedirs(OUT, exist_ok=True)

def builtins():
    """The builtins, grouped as builtin_docs.rs groups them."""
    src = open(f"{APP}/src/builtin_docs.rs").read()
    body = src[src.index("pub const DOCS"):]
    body = body[body.index("[") + 1 : body.index("\n];")]
    lit = r'"((?:[^"\\]|\\.)*)"'
    entry = re.compile(r"\(\s*" + lit + r",\s*" + lit + r",\s*" + lit + r",?\s*\)", re.S)
    unescape = lambda s: re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), s)
    out, count = [], 0
    for m in re.finditer(r"^[ \t]*// ([^\n]*)$|" + entry.pattern, body, re.M | re.S):
        if m.group(1) is not None:
            out += ["", f"### {m.group(1)}", ""]
            continue
        name, params, what = (unescape(m.group(i)) for i in (2, 3, 4))
        out.append(f"- `{name}({params})`: {what}")
        count += 1
    return count, out

def chapters():
    skip = re.compile(r"^(00-|01-|\d0-part|90-)")
    files = sorted(glob.glob(f"{APP}/book/chapters/*.md"))
    return [open(f).read().rstrip("\n") for f in files if not skip.match(os.path.basename(f))]

# What the guide's examples call but do not define, and the table they use.
STUBS = """fn save(u) = u
fn search(q: str, page: int) = { q, page }
"""
TABLES = """create table todos (id integer primary key, text text not null, done int not null default 0);
create table tasks (id integer primary key, text text not null);
create table accounts (id integer primary key, cents int not null);
"""
# The files the structure example names, beside the two it shows.
FILES = {
    "lib/notes.qwe": '// notes\nserver fn load_notes() -> list with db = db.query("select * from todos")\n'
        'server fn add_note(text: str) with db = db.exec("insert into todos (text) values (?)", text)\n',
    "ui/page.qwe": "// the page\nview Page(title: str, children) =\n"
        "    <html><head><title>{title}</title></head><body>{children}</body></html>\n",
}
NOTE_LIST = """
component NoteList() {
    state notes = load_notes()
    <ul>{for n in notes ?? [] => <li key={n.id} @click={() => { let _ = add_note("x") }}>{n.text}</li>}</ul>
}
"""

def check_examples(guide):
    """Every code block in llms.md passes `quillo check` with no warning: those that
    are a file (`// main.qwe`) as one program, the rest as another. A line that says
    `quillo check reports it` is the example of a mistake, and left out."""
    quillo = os.environ.get("QUILLO", f"{APP}/target/release/quillo")
    blocks = re.findall(r"```quillo\n(.*?)```", guide, re.S)
    named = [b for b in blocks if re.match(r"// [\w/]+\.qwe", b)]
    rest = [b for b in blocks if b not in named]
    tmp = tempfile.mkdtemp(prefix="llms-")
    try:
        for d in ("one", "files"):
            os.makedirs(f"{tmp}/{d}/migrations")
            open(f"{tmp}/{d}/migrations/001_init.sql", "w").write(TABLES)
        lines, opened = [], False
        for l in "\n".join(rest).split("\n"):
            if "quillo check reports it" in l or (opened and re.match(r"db\.(open|migrate)\(", l)):
                continue
            opened = opened or l.startswith("db.migrate(")
            lines.append(l)
        open(f"{tmp}/one/main.qwe", "w").write("\n".join(lines[:2] + [STUBS] + lines[2:]))
        files = dict(FILES)
        for b in named:
            path = re.match(r"// ([\w/]+\.qwe)", b).group(1)
            files[path] = b + (NOTE_LIST if path == "ui/note_list.qwe" else "")
        if "ui/note_list.qwe" in files:
            files["main.qwe"] = files["main.qwe"].replace('use "ui/page.qwe"', 'use "ui/page.qwe"\nuse "ui/note_list.qwe"')
        for path, text in files.items():
            os.makedirs(os.path.dirname(f"{tmp}/files/{path}"), exist_ok=True)
            open(f"{tmp}/files/{path}", "w").write(text)
        bad = False
        for d in ("one", "files"):
            r = subprocess.run([quillo, "check", "main.qwe"], cwd=f"{tmp}/{d}", capture_output=True, text=True)
            said = (r.stdout + r.stderr).strip()
            print(f"llms.md's examples ({'the files' if d == 'files' else 'the rest'}): {said.splitlines()[-1] if said else ''}")
            if r.returncode != 0 or "warning" in said:
                print(said, file=sys.stderr)
                bad = True
        if bad:
            sys.exit("llms.py: an example in llms.md does not pass quillo check; nothing written")
    finally:
        shutil.rmtree(tmp)

def typed_builtins():
    """A tag without builtin_docs.rs: the names, parameter types and result in check.rs's table."""
    src = open(f"{APP}/src/check.rs").read()
    body = src[src.index("const BUILTINS"):]
    body = body[: body.index("\n];")]
    out = [f"- `{n}({p})` gives {r or 'nothing'}" for n, p, r in re.findall(r'^\s*\("([a-z_.0-9]+)", "([^"]*)", "([^"]*)"\)', body, re.M) if not n.startswith("__")]
    return len(out), out

if RELEASE and not os.path.exists(f"{APP}/book/llms.md"):
    version = RELEASE[1:]
    count, lines = typed_builtins()
    short = f"# Quillo {version}\n\nThe user guide of Quillo {version}, as its release has it, for AI coding assistants. " \
        + "The guide at https://quillolang.org/llms.txt describes main, the next release, which has more: " \
        + f"write for {version} from this one.\n\n## Builtins\n\nAll {count} of them, with the types they take and give.\n" \
        + "\n".join(lines) + "\n\n# The user guide\n\n" + open(f"{APP}/USERGUIDE.md").read().rstrip("\n") + "\n"
    for name in ("llms.txt", "llms-full.txt"):
        open(f"{OUT}/{name}", "w").write(short)
        print(f"{OUT}/{name}: {len(short.split())} words")
    shutil.rmtree(APP)
    sys.exit(0)

guide = open(f"{APP}/book/llms.md").read().rstrip("\n")
if not RELEASE:
    check_examples(guide)
count, lines = builtins()
short = guide + f"\n\n## Builtins\n\nAll {count} of them. A name with a dot is a module's (`db.query`).\n" \
    + "\n".join(lines) + "\n\n## More\n\n" \
    + f"- [The whole manual in one file]({LINK}/llms-full.txt)\n" \
    + "- [Documentation](https://quillolang.org/docs/)\n" \
    + "- [Plugins](https://quillolang.org/plugins/)\n" \
    + "- [A program to start from](https://github.com/quillolang/starter)\n"
full = short + "\n\n# The manual\n\n" + "\n\n".join(chapters()) + "\n"

for name, text in (("llms.txt", short), ("llms-full.txt", full)):
    open(f"{OUT}/{name}", "w").write(text)
    print(f"{OUT}/{name}: {len(text.split())} words")
print(f"{count} builtins")
if RELEASE:
    shutil.rmtree(APP)
