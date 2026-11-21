# Three expressions piped into the REPL, each answer on one line; then the guide's REPL section.
out=$(printf 'let p = { name: "Rex" }\np.age\np.name\np.age == null\n' | "$QUILLO" repl 2>&1 | tr '\n' '|')
echo "repl answers: $out"
book=../../../book/chapters/64-concurrency-and-reloading.md
said=$(awk '/^## The REPL/ { on = 1; next } /^## / { on = 0 } on' "$book" | grep -ci 'null')
echo "the guide's REPL section names null: $said time(s)"
case "$out" in
  null\|*) echo "verdict: fixed (the REPL prints null)" ;;
  *) if [ "$said" -gt 0 ]; then echo "verdict: fixed (the guide says it)"; else echo "verdict: open"; fi ;;
esac
