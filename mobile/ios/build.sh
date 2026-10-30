#!/bin/sh
# Builds the iOS app around a Quillo program: its name, id, version, build number and
# icon from app.toml in the program's folder (see ../app-config.sh).
#
#   mobile/ios/build.sh app                  for the simulator
#   mobile/ios/build.sh app --run            ... and open it on a booted (or the first) iPhone simulator
#   mobile/ios/build.sh app --device         for iPhones and iPads (arm64)
#   mobile/ios/build.sh app --archive        ... and an .ipa for App Store Connect (build/<Name>-<version>-<build>.ipa)
#   mobile/ios/build.sh app --device --run   ... and install it on a connected iPhone (signed only)
#
# A device build is signed when these are set, with what an Apple Developer account
# gives (Xcode > Settings > Accounts, or developer.apple.com):
#   QUILLO_IOS_IDENTITY   the certificate, as `security find-identity -v -p codesigning` names it
#                         ("Apple Distribution: ..." for the App Store, "Apple Development: ..." to run on a phone)
#   QUILLO_IOS_PROFILE    the provisioning profile (.mobileprovision) for the app's id
# Without them it is built unsigned, ready to sign, and says so.
#
# The program's folder is bundled whole, but for its databases and any subfolder
# that is a program of its own (has a main.qwe). No Xcode project: swiftc links
# the runtime (mobile/quillo-mobile, a static library) into the app.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
prog=$(cd "${1:-$root/app}" && pwd)
[ $# -gt 0 ] && shift
run=""; device=""; archive=""
for a in "$@"; do
    case "$a" in
        --run) run=1 ;;
        --device) device=1 ;;
        --archive) device=1; archive=1 ;;
        *) echo "build.sh: unknown option $a" >&2; exit 2 ;;
    esac
done
. "$root/mobile/app-config.sh"

# The oldest iOS the app runs on, for Swift, Rust and the C the runtime builds (SQLite,
# ring): without it the C is built for the SDK's own version and may not run on older ones.
export IPHONEOS_DEPLOYMENT_TARGET=17.0
if [ -n "$device" ]; then
    target=aarch64-apple-ios; sdkname=iphoneos; swift_target=arm64-apple-ios17.0; platform=iPhoneOS
else
    target=aarch64-apple-ios-sim; sdkname=iphonesimulator; swift_target=arm64-apple-ios17.0-simulator; platform=iPhoneSimulator
fi
echo "== $APP_NAME ($APP_ID) $APP_VERSION ($APP_BUILD)${APP_CONFIG:+, from app.toml}"
[ -z "$APP_CONFIG" ] && echo "   no app.toml in $prog: Quillo's name, id and icon are used"

echo "== the runtime, for $target"
(cd "$root/mobile/quillo-mobile" && cargo build --release --target $target)
lib="$root/mobile/quillo-mobile/target/$target/release/libquillo_mobile.a"

build="$here/build/$sdkname"
out="$build/Quillo.app"
rm -rf "$build"; mkdir -p "$out/program"
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
sdk=$(xcrun --sdk $sdkname --show-sdk-path)
xcrun -sdk $sdkname swiftc -O -parse-as-library -target $swift_target -sdk "$sdk" \
    -import-objc-header "$here/Quillo/quillo.h" "$here"/Quillo/*.swift "$lib" \
    -framework WebKit -framework UIKit -framework CoreLocation -framework UserNotifications -framework Security -framework SystemConfiguration -framework CoreFoundation -lc++ \
    -o "$out/Quillo"
[ -n "$device" ] && xcrun strip -x "$out/Quillo"

# Info.plist: the template, with the app's own name, id and version.
plist="$out/Info.plist"
cp "$here/Quillo/Info.plist" "$plist"
plutil -replace CFBundleIdentifier -string "$APP_ID" "$plist"
plutil -replace CFBundleName -string "$APP_NAME" "$plist"
plutil -replace CFBundleDisplayName -string "$APP_NAME" "$plist"
plutil -replace CFBundleShortVersionString -string "$APP_VERSION" "$plist"
plutil -replace CFBundleVersion -string "$APP_BUILD" "$plist"
plutil -replace CFBundleSupportedPlatforms -json "[\"$platform\"]" "$plist"
cp "$here/Quillo/PrivacyInfo.xcprivacy" "$out/"

# The icon: an asset catalog of one 1024-point image, compiled by actool, which also
# says in the plist where the icon is.
xc="$build/Assets.xcassets"
mkdir -p "$xc/AppIcon.appiconset"
cp "$APP_ICON" "$xc/AppIcon.appiconset/icon.png"
printf '{"images":[{"filename":"icon.png","idiom":"universal","platform":"ios","size":"1024x1024"}],"info":{"author":"xcode","version":1}}' > "$xc/AppIcon.appiconset/Contents.json"
printf '{"info":{"author":"xcode","version":1}}' > "$xc/Contents.json"
xcrun actool "$xc" --compile "$out" --platform $sdkname --minimum-deployment-target 17.0 \
    --app-icon AppIcon --target-device iphone --target-device ipad \
    --output-partial-info-plist "$build/icon.plist" --output-format human-readable-text --errors >/dev/null
/usr/libexec/PlistBuddy -c "Merge $build/icon.plist" "$plist" >/dev/null

if [ -n "$device" ]; then
    # What Xcode writes into every app it builds; App Store Connect checks for them.
    sdkver=$(xcrun --sdk iphoneos --show-sdk-version); sdkbuild=$(xcrun --sdk iphoneos --show-sdk-build-version)
    xv=$(xcodebuild -version | sed -n 's/^Xcode //p'); xb=$(xcodebuild -version | sed -n 's/^Build version //p')
    dtx=$(echo "$xv" | awk -F. '{ printf "%d%d%d", $1, ($2 == "" ? 0 : $2), ($3 == "" ? 0 : $3) }')
    for kv in "DTPlatformName iphoneos" "DTPlatformVersion $sdkver" "DTSDKName iphoneos$sdkver" "DTSDKBuild $sdkbuild" \
              "DTPlatformBuild $sdkbuild" "DTXcode $dtx" "DTXcodeBuild $xb" "DTCompiler com.apple.compilers.llvm.clang.1_0" \
              "BuildMachineOSBuild $(sw_vers -buildVersion)" "CFBundleInfoDictionaryVersion 6.0" "CFBundleDevelopmentRegion en"; do
        plutil -replace "${kv%% *}" -string "${kv#* }" "$plist"
    done
    plutil -replace LSRequiresIPhoneOS -bool YES "$plist"
    plutil -replace UIRequiredDeviceCapabilities -json '["arm64"]' "$plist"
fi
plutil -lint "$plist" >/dev/null

# Signing: ad hoc for the simulator; for a device, the certificate and profile given.
signed=""
if [ -n "$device" ] && [ -n "$QUILLO_IOS_IDENTITY" ] && [ -n "$QUILLO_IOS_PROFILE" ]; then
    [ -f "$QUILLO_IOS_PROFILE" ] || { echo "QUILLO_IOS_PROFILE: no such file $QUILLO_IOS_PROFILE" >&2; exit 1; }
    security cms -D -i "$QUILLO_IOS_PROFILE" > "$build/profile.plist"
    appid=$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:application-identifier' "$build/profile.plist")
    case "${appid#*.}" in
        "$APP_ID"|"*") ;;
        *) echo "the profile is for ${appid#*.}, not $APP_ID: make one for $APP_ID at developer.apple.com" >&2; exit 1 ;;
    esac
    /usr/libexec/PlistBuddy -x -c 'Print :Entitlements' "$build/profile.plist" > "$build/entitlements.plist"
    cp "$QUILLO_IOS_PROFILE" "$out/embedded.mobileprovision"
    codesign --force --timestamp --sign "$QUILLO_IOS_IDENTITY" --entitlements "$build/entitlements.plist" "$out"
    codesign --verify --strict "$out"
    signed=1
    echo "   signed by $QUILLO_IOS_IDENTITY, team ${appid%%.*}"
else
    codesign --force --sign - "$out" >/dev/null
    [ -n "$device" ] && echo "   not signed for devices: set QUILLO_IOS_IDENTITY and QUILLO_IOS_PROFILE (see the top of this script)"
fi
echo "   $out ($(du -sh "$out" | cut -f1))"

if [ -n "$archive" ]; then
    name=$(echo "$APP_NAME" | tr -c 'A-Za-z0-9._-' '-' | sed 's/-*$//')
    suffix=""; [ -n "$signed" ] || suffix="-unsigned"
    ipa="$here/build/$name-$APP_VERSION-$APP_BUILD$suffix.ipa"
    rm -rf "$build/Payload" "$ipa"; mkdir -p "$build/Payload"
    cp -R "$out" "$build/Payload/"
    (cd "$build" && zip -qry "$ipa" Payload)
    rm -rf "$build/Payload"
    echo "== $ipa ($(du -h "$ipa" | cut -f1))"
    if [ -n "$signed" ]; then
        echo "   upload: xcrun altool --upload-app -f \"$ipa\" -t ios --apiKey <key id> --apiIssuer <issuer id> (or Apple's Transporter app)"
    else
        echo "   unsigned: sign it once the account is active (run this again with QUILLO_IOS_IDENTITY and QUILLO_IOS_PROFILE set)"
    fi
fi

[ -z "$run" ] && exit 0
if [ -n "$device" ]; then
    [ -n "$signed" ] || { echo "a phone runs only a signed app: set QUILLO_IOS_IDENTITY and QUILLO_IOS_PROFILE" >&2; exit 1; }
    phone=$(xcrun devicectl list devices 2>/dev/null | awk '/connected/ && /iPhone|iPad/ { for (i = 1; i <= NF; i++) if ($i ~ /^[0-9A-F-]{36}$/) { print $i; exit } }')
    [ -n "$phone" ] || { echo "no iPhone connected (xcrun devicectl list devices)" >&2; exit 1; }
    echo "== on the phone $phone"
    xcrun devicectl device install app --device "$phone" "$out"
    xcrun devicectl device process launch --device "$phone" "$APP_ID"
    exit 0
fi
echo "== on the simulator"
dev=$(xcrun simctl list devices booted | grep -o '[0-9A-F-]\{36\}' | head -1)
if [ -z "$dev" ]; then
    dev=$(xcrun simctl list devices available | grep 'iPhone' | grep -o '[0-9A-F-]\{36\}' | head -1)
    xcrun simctl boot "$dev"
fi
xcrun simctl bootstatus "$dev" -b >/dev/null
xcrun simctl install "$dev" "$out"
xcrun simctl launch "$dev" "$APP_ID"
echo "   running on $dev"
