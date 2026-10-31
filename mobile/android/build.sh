#!/bin/sh
# Builds the Android app around a Quillo program: its name, id, version, build number
# and icon from app.toml in the program's folder (see ../app-config.sh).
#
#   mobile/android/build.sh app              a debug APK
#   mobile/android/build.sh app --run        ... installed on a connected phone or a running emulator, and opened
#   mobile/android/build.sh app --release    a signed APK and an App Bundle (.aab) for Google Play
#   mobile/android/build.sh app --release --run
#
# A release is signed with the upload key given by
#   QUILLO_ANDROID_KEYSTORE          the keystore (.jks)
#   QUILLO_ANDROID_KEY_ALIAS         the key in it
#   QUILLO_ANDROID_STORE_PASSWORD    its password (QUILLO_ANDROID_KEY_PASSWORD if the key's differs)
# Without them, a key is made once for the app's id in ~/.quillo/android/ and used from
# then on: keep it (and its .properties beside it) safe, Google Play knows the app by it.
#
# Needs: the Android SDK with platform 36, build-tools and the NDK (ANDROID_HOME,
# default ~/Library/Android/sdk; the newest NDK in it unless ANDROID_NDK_HOME says),
# a JDK 17 (JAVA_HOME), cargo-ndk (cargo install cargo-ndk), and the Rust targets
# aarch64-linux-android and x86_64-linux-android (rustup target add ...). Gradle comes
# from the wrapper. Icons are resized with sips (macOS) or ImageMagick.
#
# After --run, launch it again with scripts for the page (eval2, eval3, ... run on the
# pages loaded after; a long script goes as "base64:$(base64 < script.js)", which
# adb's shell passes untouched):
#   adb shell am start -n <id>/org.quillolang.app.MainActivity --es eval "\"location.href='/new'\""
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
prog=$(cd "${1:-$root/app}" && pwd)
[ $# -gt 0 ] && shift
run=""; release=""
for a in "$@"; do
    case "$a" in
        --run) run=1 ;;
        --release) release=1 ;;
        *) echo "build.sh: unknown option $a" >&2; exit 2 ;;
    esac
done
. "$root/mobile/app-config.sh"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
[ -n "$ANDROID_NDK_HOME" ] || export ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
[ -n "$JAVA_HOME" ] || { [ -d /opt/homebrew/opt/openjdk@17 ] && export JAVA_HOME=/opt/homebrew/opt/openjdk@17; }
adb="$ANDROID_HOME/platform-tools/adb"
echo "== $APP_NAME ($APP_ID) $APP_VERSION ($APP_BUILD)${APP_CONFIG:+, from app.toml}"
[ -z "$APP_CONFIG" ] && echo "   no app.toml in $prog: Quillo's name, id and icon are used"
export QUILLO_APP_NAME="$APP_NAME" QUILLO_APP_ID="$APP_ID" QUILLO_APP_VERSION="$APP_VERSION" QUILLO_APP_BUILD="$APP_BUILD" QUILLO_APP_MAIN="$APP_MAIN"

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

echo "== the icon"
# A launcher icon at each density, and an adaptive one (Android 8+) whose background is
# the whole icon, which the launcher masks to its own shape.
res="$here/app/build/generated/quillo/res"
rm -rf "$res"; mkdir -p "$res/mipmap-anydpi-v26"
resize() { # size, from, to
    if command -v sips >/dev/null 2>&1; then sips -s format png -z "$1" "$1" "$2" --out "$3" >/dev/null
    elif command -v magick >/dev/null 2>&1; then magick "$2" -resize "$1x$1!" "$3"
    elif command -v convert >/dev/null 2>&1; then convert "$2" -resize "$1x$1!" "$3"
    else echo "no sips or ImageMagick to resize the icon" >&2; exit 1; fi
}
for d in mdpi:1 hdpi:1.5 xhdpi:2 xxhdpi:3 xxxhdpi:4; do
    name=${d%%:*}; scale=${d#*:}
    mkdir -p "$res/mipmap-$name"
    resize "$(echo "48 * $scale / 1" | bc)" "$APP_ICON" "$res/mipmap-$name/ic_launcher.png"
    resize "$(echo "108 * $scale / 1" | bc)" "$APP_ICON" "$res/mipmap-$name/ic_launcher_full.png"
done
cat > "$res/mipmap-anydpi-v26/ic_launcher.xml" <<'XML'
<?xml version="1.0" encoding="utf-8"?>
<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
    <background android:drawable="@mipmap/ic_launcher_full" />
    <foreground android:drawable="@android:color/transparent" />
</adaptive-icon>
XML

if [ -n "$release" ] && [ -z "$QUILLO_ANDROID_KEYSTORE" ]; then
    # An upload key for this app, made once and kept outside the repository.
    keys="$HOME/.quillo/android"; ks="$keys/$APP_ID-upload.jks"; kp="$keys/$APP_ID-upload.properties"
    if [ ! -f "$ks" ]; then
        mkdir -p "$keys"; chmod 700 "$keys"
        pass=$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 32)
        "$JAVA_HOME/bin/keytool" -genkeypair -keystore "$ks" -storetype PKCS12 -alias upload -keyalg RSA -keysize 4096 \
            -validity 10000 -storepass "$pass" -keypass "$pass" -dname "CN=$APP_NAME, O=$APP_ID" >/dev/null 2>&1
        printf 'alias=upload\npassword=%s\n' "$pass" > "$kp"; chmod 600 "$ks" "$kp"
        echo "   made an upload key: $ks (password in $kp). Keep both safe and backed up: Google Play knows the app by this key."
    fi
    export QUILLO_ANDROID_KEYSTORE="$ks"
    export QUILLO_ANDROID_KEY_ALIAS=$(sed -n 's/^alias=//p' "$kp")
    export QUILLO_ANDROID_STORE_PASSWORD=$(sed -n 's/^password=//p' "$kp")
fi

echo "== the app"
echo "sdk.dir=$ANDROID_HOME" > "$here/local.properties"
if [ -n "$release" ]; then
    (cd "$here" && ./gradlew -q assembleRelease bundleRelease)
    name=$(echo "$APP_NAME" | tr -c 'A-Za-z0-9._-' '-' | sed 's/-*$//')
    out="$here/build/outputs"; mkdir -p "$out"
    apk="$out/$name-$APP_VERSION-$APP_BUILD.apk"; aab="$out/$name-$APP_VERSION-$APP_BUILD.aab"
    cp "$here/app/build/outputs/apk/release/app-release.apk" "$apk"
    cp "$here/app/build/outputs/bundle/release/app-release.aab" "$aab"
    "$ANDROID_HOME/build-tools/$(ls "$ANDROID_HOME/build-tools" | sort -V | tail -1)/apksigner" verify "$apk"
    echo "   $apk ($(du -h "$apk" | cut -f1)), signed"
    echo "   $aab ($(du -h "$aab" | cut -f1)), for Google Play"
else
    (cd "$here" && ./gradlew -q assembleDebug)
    apk="$here/app/build/outputs/apk/debug/app-debug.apk"
    echo "   $apk"
fi

[ -z "$run" ] && exit 0
"$adb" install -r "$apk"
"$adb" shell am start -n "$APP_ID/org.quillolang.app.MainActivity"
