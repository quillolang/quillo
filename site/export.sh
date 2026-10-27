#!/bin/sh
# Renders the site to static files in site/dist: every page in /sitemap.txt
# (as <path>/index.html), the component bundles they load, fonts, favicon and link preview. A static host serves them as they are; the
# components run in the browser and call nothing on the server.
#
#   cargo build --release && site/export.sh
set -eu

here=$(cd "$(dirname "$0")" && pwd)
quillo="$here/../target/release/quillo"
port=${PORT:-8790}
out="$here/dist"

rm -rf "$out"
mkdir -p "$out/_quillo/b"

"$quillo" run "$here/main.qwe" --port "$port" --log off &
pid=$!
trap 'kill $pid 2>/dev/null' EXIT

i=0
until curl -sf -o /dev/null "http://127.0.0.1:$port/"; do
    i=$((i + 1))
    [ $i -gt 50 ] && { echo "export: the site did not start" >&2; exit 1; }
    sleep 0.2
done

curl -sf "http://127.0.0.1:$port/sitemap.txt" -o "$out/sitemap.txt"
for path in $(sed 's|^https://[^/]*||' "$out/sitemap.txt"); do
    mkdir -p "$out$path"
    curl -sf "http://127.0.0.1:$port$path" -o "$out${path}index.html"
done
curl -sf "http://127.0.0.1:$port/favicon.svg" -o "$out/favicon.svg"
curl -sf "http://127.0.0.1:$port/banner.png" -o "$out/banner.png"
mkdir -p "$out/fonts"
for f in "$here"/public/fonts/*.woff2; do
    curl -sf "http://127.0.0.1:$port/fonts/${f##*/}" -o "$out/fonts/${f##*/}"
done
for js in $(find "$out" -name index.html -exec grep -oh '/_quillo/b/[0-9a-f]*\.js' {} + | sort -u); do
    curl -sf "http://127.0.0.1:$port$js" -o "$out$js"
done

# Bundles are named by their content, so browsers may keep them for good.
# Pages may be kept 30 seconds, as quillo run lets a prefetched page be kept:
# a browser without speculation rules fetches the page a link points to, and
# the click is answered from its cache. (LiteSpeed keeps a static file's
# headers whatever the request, so this cannot be for prefetches alone.)
cat > "$out/.htaccess" <<'EOF'
<IfModule mod_headers.c>
  <FilesMatch "\.(js|woff2)$">
    Header set Cache-Control "public, max-age=31536000, immutable"
  </FilesMatch>
  <FilesMatch "\.html$">
    Header set Cache-Control "private, max-age=30"
  </FilesMatch>
</IfModule>
<IfModule mod_deflate.c>
  AddOutputFilterByType DEFLATE text/html text/css application/javascript image/svg+xml
</IfModule>
EOF

find "$out" -type f | sed "s|$out/||" | sort
