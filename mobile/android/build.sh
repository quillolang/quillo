#!/bin/sh
# Builds the Android app (a debug APK) around a Quillo program, and with --run
# installs it on a connected device or a running emulator and opens it.
#
#   mobile/android/build.sh app
#   mobile/android/build.sh app --run
#
# Needs: the Android SDK with platform 35, build-tools and the NDK (ANDROID_HOME,
# default ~/Library/Android/sdk; the newest NDK in it unless ANDROID_NDK_HOME says),
# a JDK 17 (JAVA_HOME), cargo-ndk (cargo install cargo-ndk), and the Rust targets
# aarch64-linux-android and x86_64-linux-android (rustup target add ...). Gradle comes
# from the wrapper. After --run, launch it again with scripts for the page:
#   adb shell am start -n org.quillolang.app/.MainActivity --es eval "location.href='/new'"
# (eval2, eval3, ... run on the pages loaded after; a long script goes as
# "base64:$(base64 < script.js)", which adb's shell passes untouched)
set -e
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
[ -n "$ANDROID_NDK_HOME" ] || export ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
[ -n "$JAVA_HOME" ] || { [ -d /opt/homebrew/opt/openjdk@17 ] && export JAVA_HOME=/opt/homebrew/opt/openjdk@17; }
adb="$ANDROID_HOME/platform-tools/adb"
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
prog=$(cd "${1:-$root/app}" && pwd)
run=""; [ "$2" = "--run" ] && run=1

echo "== the runtime, for Android"
(cd "$root/mobile/quillo-mobile" && cargo ndk -t arm64-v8a -t x86_64 -o "$here/app/src/main/jniLibs" build --release)

echo "== the program, from $prog"
assets="$here/app/src/main/assets/program"
rm -rf "$assets"; mkdir -p "$assets"
skip=$(cd "$prog" && find . -mindepth 2 -name main.qwe -exec dirname {} \; | sed 's#^\./##')
(cd "$prog" && find . -type f \
    ! -name '*.db' ! -name '*.db-wal' ! -name '*.db-shm' ! -name '*.sqlite' ! -path './uploads/*' ! -path './.quillo/*' \
    | while read -r f; do
        rel=${f#./}; keep=1
        for s in $skip; do case "$rel" in "$s"/*) keep="";; esac; done
        if [ -n "$keep" ]; then mkdir -p "$assets/$(dirname "$rel")"; cp "$f" "$assets/$rel"; fi
      done; true)

echo "== the app"
echo "sdk.dir=$ANDROID_HOME" > "$here/local.properties"
(cd "$here" && ./gradlew -q assembleDebug)
apk="$here/app/build/outputs/apk/debug/app-debug.apk"
echo "   $apk"

[ -z "$run" ] && exit 0
"$adb" install -r "$apk"
"$adb" shell am start -n org.quillolang.app/.MainActivity
