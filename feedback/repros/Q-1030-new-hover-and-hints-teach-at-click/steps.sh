# The templates of quillo new, the hover's text, and the hints of quillo check.
dir=$(mktemp -d)
(cd "$dir" && "$QUILLO" new demo > /dev/null 2>&1 && "$QUILLO" new mini --minimal > /dev/null 2>&1)
found=$(grep -rhoE '@(click|submit|change|input)=|bind:value=' "$dir/demo" "$dir/mini" | sort | uniq -c | awk '{ printf "%s %s ", $1, $2 }')
echo "quillo new: ${found:-none}"
rm -rf "$dir"
echo "hover: $(strings "$QUILLO" | grep -o 'Handlers: `[^`]*`' | head -1)"
echo "hints:"
for f in main.qwe server.qwe typo.qwe; do "$QUILLO" check $f 2>&1; done
