#!/bin/sh
# Renders brand/banner.html to PNGs with headless Chrome:
#   quillo-banner.png         2400×1260  Open Graph, X, LinkedIn, Slack, Discord (1200×630 at 2x)
#   quillo-banner-github.png  1280×640   GitHub's social preview (Settings → Social preview)
#   quillo-youtube.png        2560×1440  YouTube channel banner (text in the 1546×423 middle)
#   quillo-x.png              3000×1000  X header (1500×500 at 2x)
#   quillo-reddit.png         4000×192   Reddit banner, desktop
#   quillo-reddit-mobile.png  1600×480   Reddit banner, mobile
set -eu
here=$(cd "$(dirname "$0")" && pwd)
chrome=${CHROME:-"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"}
shot() { # width height scale out [page]
    "$chrome" --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files \
        --force-device-scale-factor="$3" --window-size="$1,$2" --virtual-time-budget=3000 \
        --screenshot="$here/$4" "file://$here/${5:-banner.html}" >/dev/null 2>&1
}
shot 1200 630 2 quillo-banner.png
shot 1280 640 1 quillo-banner-github.png
shot 2560 1440 1 quillo-youtube.png "banner-wide.html#youtube"
shot 1500 500 2 quillo-x.png "banner-wide.html#x"
shot 4000 192 1 quillo-reddit.png "banner-wide.html#reddit"
shot 1600 480 1 quillo-reddit-mobile.png "banner-wide.html#reddit-mobile"
ls -l "$here"/quillo-*.png
