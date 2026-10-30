#!/bin/sh
# Builds Quillo.app for the iOS simulator around a Quillo program, and with --run
# installs it on a booted (or the first available) iPhone simulator and opens it.
#
#   mobile/ios/build.sh app            the program in app/ (its main.qwe)
#   mobile/ios/build.sh app --run
#
# The program's folder is bundled whole, but for its databases and any subfolder
# that is a program of its own (has a main.qwe). No Xcode project: swiftc links
# the runtime (mobile/quillo-mobile, a static library) into the app.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
prog=$(cd "${1:-$root/app}" && pwd)
run=""; [ "$2" = "--run" ] && run=1
target=aarch64-apple-ios-sim

echo "== the runtime, for $target"
(cd "$root/mobile/quillo-mobile" && cargo build --release --target $target)
lib="$root/mobile/quillo-mobile/target/$target/release/libquillo_mobile.a"

out="$here/build/Quillo.app"
rm -rf "$out"; mkdir -p "$out/program"
echo "== the program, from $prog"
skip=$(cd "$prog" && find . -mindepth 2 -name main.qwe -exec dirname {} \; | sed 's#^\./##')
(cd "$prog" && find . -type f \
    ! -name '*.db' ! -name '*.db-wal' ! -name '*.db-shm' ! -name '*.sqlite' ! -path './uploads/*' ! -path './.quillo/*' \
    | while read -r f; do
        rel=${f#./}; keep=1
        for s in $skip; do case "$rel" in "$s"/*) keep="";; esac; done
        if [ -n "$keep" ]; then mkdir -p "$out/program/$(dirname "$rel")"; cp "$f" "$out/program/$rel"; fi
      done; true)
echo "   $(find "$out/program" -type f | wc -l | tr -d ' ') files, leaving out: $(echo $skip | tr '\n' ' ')"

echo "== the app"
sdk=$(xcrun --sdk iphonesimulator --show-sdk-path)
xcrun -sdk iphonesimulator swiftc -O -parse-as-library -target arm64-apple-ios17.0-simulator -sdk "$sdk" \
    -import-objc-header "$here/Quillo/quillo.h" "$here"/Quillo/*.swift "$lib" \
    -framework WebKit -framework UIKit -framework Security -framework SystemConfiguration -framework CoreFoundation -lc++ \
    -o "$out/Quillo"
cp "$here/Quillo/Info.plist" "$out/Info.plist"
codesign --force --sign - "$out" >/dev/null
echo "   $out ($(du -sh "$out" | cut -f1))"

[ -z "$run" ] && exit 0
echo "== on the simulator"
dev=$(xcrun simctl list devices booted | grep -o '[0-9A-F-]\{36\}' | head -1)
if [ -z "$dev" ]; then
    dev=$(xcrun simctl list devices available | grep 'iPhone' | grep -o '[0-9A-F-]\{36\}' | head -1)
    xcrun simctl boot "$dev"
fi
xcrun simctl bootstatus "$dev" -b >/dev/null
xcrun simctl install "$dev" "$out"
xcrun simctl launch "$dev" org.quillolang.app
echo "   running on $dev"
