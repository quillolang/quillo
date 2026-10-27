#!/bin/sh
# Checks that this machine can build and run Quillo, and says what to install
# for anything missing. Run it from anywhere in a checkout:
#
#   sh scripts/doctor.sh            check the tools (a few seconds)
#   sh scripts/doctor.sh --build    then build quillo and run the sample app
#
# macOS and Linux (and WSL). On Windows, scripts/doctor.ps1 does the same.
#
# The optional tools are looked for where the tests look for them, and the
# same variables point at them: QUILLO_CHROME, QUILLO_TEST_PG_BIN,
# QUILLO_TEST_MYSQL_BIN.
set -u

NODE_MIN=22       # tests/browser/run.js uses Node's built-in WebSocket
DISK_MIN_GB=5     # a release build's target/ folder, with room to spare

root=$(cd "$(dirname "$0")/.." && pwd)
RUST_MIN=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' "$root/Cargo.toml")   # from Cargo.toml
os=$(uname -s)
missing=0

if [ -t 1 ]; then g='\033[32m' y='\033[33m' r='\033[31m' d='\033[2m' n='\033[0m'; else g='' y='' r='' d='' n=''; fi
ok()   { printf "  ${g}ok${n}       %s\n" "$1"; }
need() { printf "  ${r}missing${n}  %s\n           ${d}%s${n}\n" "$1" "$2"; missing=$((missing + 1)); }
opt()  { printf "  ${y}--${n}       %s\n           ${d}%s${n}\n" "$1" "$2"; }
has()  { command -v "$1" >/dev/null 2>&1; }
# at_least 1.99.0 1.94: true when the first version is the second or later.
at_least() {
    printf '%s\n%s\n' "$1" "$2" | awk -F. 'NR == 1 { a = $1 * 10000 + $2 } NR == 2 { b = $1 * 10000 + $2 } END { exit !(a >= b) }'
}

case "$os" in
    Darwin) cc_fix="xcode-select --install" ;;
    *) if has apt-get; then cc_fix="sudo apt-get install build-essential"
       elif has dnf; then cc_fix="sudo dnf install gcc make"
       elif has pacman; then cc_fix="sudo pacman -S base-devel"
       else cc_fix="install a C compiler (gcc or clang) from your package manager"; fi ;;
esac

echo "To build and run Quillo"

if has cargo && has rustc; then
    v=$(rustc --version | awk '{ print $2 }')
    if at_least "$v" "$RUST_MIN"; then ok "Rust $v"
    else need "Rust $v is too old: Quillo needs $RUST_MIN or later" "rustup update stable"; fi
else
    need "Rust (cargo and rustc)" "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   (see https://rustup.rs)"
fi

# Some dependencies compile C (ring, the bundled SQLite, mimalloc): try it.
tmp=$(mktemp -d 2>/dev/null || mktemp -d -t quillo-doctor)
trap 'rm -rf "$tmp"' EXIT
cc=${CC:-cc}
printf 'int main(void) { return 0; }\n' > "$tmp/t.c"
if has "$cc" && "$cc" "$tmp/t.c" -o "$tmp/t" >/dev/null 2>&1; then
    ok "C compiler ($("$cc" --version 2>/dev/null | head -n 1))"
else
    need "a C compiler that works (some dependencies compile C)" "$cc_fix"
fi

free_kb=$(df -Pk "$root" | awk 'NR == 2 { print $4 }')
free_gb=$((free_kb / 1024 / 1024))
if [ "$free_gb" -ge "$DISK_MIN_GB" ]; then ok "$free_gb GB free on disk"
else need "only $free_gb GB free on disk" "a release build needs about $DISK_MIN_GB GB for target/"; fi

if has lsof && lsof -nP -iTCP:8080 -sTCP:LISTEN >/dev/null 2>&1; then
    opt "port 8080 is taken (by $(lsof -nP -iTCP:8080 -sTCP:LISTEN | awk 'NR == 2 { print $1 }'))" "quillo run listens on 8080 unless given --port N or PORT"
else
    ok "port 8080 is free (quillo run's default)"
fi

echo
echo "Optional: the tests, and extras"

if has node; then
    v=$(node --version | sed 's/^v//')
    if at_least "$v" "$NODE_MIN"; then ok "Node $v"
    else opt "Node $v: the browser tests need $NODE_MIN or later" "https://nodejs.org"; fi
else
    opt "Node $NODE_MIN or later: the component compiler's and browser tests, the VS Code extension" "https://nodejs.org"
fi

chrome=${QUILLO_CHROME:-}
if [ -z "$chrome" ]; then
    for c in "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
             "/Applications/Chromium.app/Contents/MacOS/Chromium" \
             /usr/bin/google-chrome /usr/bin/google-chrome-stable /usr/bin/chromium /usr/bin/chromium-browser; do
        [ -e "$c" ] && { chrome=$c; break; }
    done
fi
if [ -n "$chrome" ] && [ -e "$chrome" ]; then ok "Chrome ($chrome)"
else opt "Chrome or Chromium: the browser tests are skipped without it" "install it, or set QUILLO_CHROME to its path"; fi

pg=${QUILLO_TEST_PG_BIN:-}
if [ -z "$pg" ]; then
    for d in $(echo "${PATH:-}" | tr ':' ' ') /usr/lib/postgresql/*/bin /opt/homebrew/opt/*/bin /usr/local/opt/*/bin; do
        [ -x "$d/initdb" ] && [ -x "$d/pg_ctl" ] && { pg=$d; break; }
    done
fi
if [ -n "$pg" ] && [ -x "$pg/initdb" ]; then ok "PostgreSQL ($pg)"
else opt "PostgreSQL (initdb, pg_ctl): its tests are skipped without it" "brew install postgresql, or apt-get install postgresql; or set QUILLO_TEST_PG_BIN"; fi

my=""
for d in ${QUILLO_TEST_MYSQL_BIN:-/opt/homebrew/bin /usr/local/bin /usr/bin /usr/sbin}; do
    [ -x "$d/mariadbd" ] && [ -x "$d/mariadb-install-db" ] && { my=$d; break; }
done
if [ -n "$my" ]; then ok "MariaDB ($my)"
else opt "MariaDB (mariadbd, mariadb-install-db): its tests are skipped without it" "brew install mariadb, or apt-get install mariadb-server; or set QUILLO_TEST_MYSQL_BIN"; fi

if [ "$os" = Linux ]; then
    if has pkg-config && pkg-config --exists webkit2gtk-4.1 gtk+-3.0 2>/dev/null; then
        ok "WebKitGTK and GTK (for quillo app, a native window)"
    else
        opt "WebKitGTK 4.1 and GTK 3: only for quillo app (cargo build --features app)" "sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev"
    fi
fi

echo
if [ "$missing" -gt 0 ]; then
    printf "${r}%s thing(s) to install before Quillo builds.${n}\n" "$missing"
    exit 1
fi
echo "Everything Quillo needs is here."

[ "${1:-}" = "--build" ] || { echo "Next: cargo build --release   (or: sh scripts/doctor.sh --build)"; exit 0; }

echo
echo "Building (the first build takes a few minutes)"
(cd "$root" && cargo build --release) || { printf "${r}The build failed: see the errors above.${n}\n"; exit 1; }
quillo="$root/target/release/quillo"

echo
echo "Running the sample app"
# A copy, so its database is not left in app/.
cp -R "$root/app" "$tmp/app"
"$quillo" check "$tmp/app/main.qwe" >/dev/null || { need "quillo check app/main.qwe failed" "run it to see why"; exit 1; }
ok "quillo check app/main.qwe"

port=${DOCTOR_PORT:-18080}
"$quillo" run "$tmp/app/main.qwe" --port "$port" --log off >"$tmp/run.log" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null; rm -rf "$tmp"' EXIT
i=0
while ! curl -sf -o /dev/null "http://127.0.0.1:$port/" 2>/dev/null; do
    i=$((i + 1))
    if [ $i -gt 50 ] || ! kill -0 $pid 2>/dev/null; then
        need "the sample app did not answer on port $port" "$(tail -n 3 "$tmp/run.log")"
        exit 1
    fi
    sleep 0.2
done
if curl -sf "http://127.0.0.1:$port/" | grep -q "<h1>To do</h1>" && [ "$(curl -sf "http://127.0.0.1:$port/api/todos")" = "[]" ]; then
    ok "the sample app answers: its page, and its JSON from SQLite"
else
    need "the sample app answered, but not as expected" "run quillo run app/main.qwe and open http://localhost:8080"
    exit 1
fi

echo
printf "${g}Quillo builds and runs here.${n} Start with: ./target/release/quillo run app/main.qwe --watch\n"
