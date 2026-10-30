# The app around a Quillo program: read from app.toml in the program's folder by
# ios/build.sh and android/build.sh (sourced, with $prog set). Flat TOML, one key a line:
#
#   name = "Fieldbook"                  # under the icon
#   id = "org.example.fieldbook"        # the bundle id / application id, never changed once published
#   version = "1.0.0"                   # what people see in the store
#   build = 1                           # a whole number, higher for each upload to a store
#   icon = "icon.png"                   # 1024 x 1024 PNG, no transparency (default: Quillo's)
#
# Sets APP_NAME, APP_ID, APP_VERSION, APP_BUILD, APP_ICON (a path) and APP_CONFIG
# (the file, or empty when there is none and the defaults are used).

app_value() {
    # `key = "value"` or `key = value`, a comment after it left out.
    sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*//p" "$APP_CONFIG" | head -1 |
        sed -e 's/[[:space:]]*#.*$//' -e 's/^"\(.*\)"$/\1/' -e "s/^'\(.*\)'\$/\1/"
}

APP_CONFIG=""
[ -f "$prog/app.toml" ] && APP_CONFIG="$prog/app.toml"
if [ -n "$APP_CONFIG" ]; then
    APP_NAME=$(app_value name); APP_ID=$(app_value id); APP_VERSION=$(app_value version)
    APP_BUILD=$(app_value build); APP_ICON=$(app_value icon)
fi
APP_NAME=${APP_NAME:-Quillo}
APP_ID=${APP_ID:-org.quillolang.app}
APP_VERSION=${APP_VERSION:-0.1.0}
APP_BUILD=${APP_BUILD:-1}
if [ -n "$APP_ICON" ]; then
    case "$APP_ICON" in /*) ;; *) APP_ICON="$prog/$APP_ICON";; esac
else
    APP_ICON="$(cd "$(dirname "$0")/.." && pwd)/assets/icon.png"
fi

app_fail() { echo "app.toml: $1" >&2; exit 1; }
echo "$APP_ID" | grep -Eq '^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)+$' ||
    app_fail "id \"$APP_ID\" is not reverse-DNS, like org.example.notes (letters, digits and _, dot-separated, each part starting with a letter)"
echo "$APP_VERSION" | grep -Eq '^[0-9]+(\.[0-9]+){0,2}$' || app_fail "version \"$APP_VERSION\" is not like 1.0 or 1.2.3"
echo "$APP_BUILD" | grep -Eq '^[1-9][0-9]{0,8}$' || app_fail "build \"$APP_BUILD\" is not a whole number from 1"
[ -n "$APP_NAME" ] && [ "${#APP_NAME}" -le 30 ] || app_fail "name must be 1 to 30 characters"
[ -f "$APP_ICON" ] || app_fail "icon $APP_ICON: no such file"
export APP_NAME APP_ID APP_VERSION APP_BUILD APP_ICON APP_CONFIG
