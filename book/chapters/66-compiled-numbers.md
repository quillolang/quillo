# Compiled numbers

A top-level function that only does arithmetic on ints, floats and bools is compiled to
machine code (with [Cranelift](https://cranelift.dev)) the second time it is called, or the
first if it has a loop of its own, for the types of the arguments it gets: `let`,
assignment, `if`, `&&`, `||`, `return`, `for` over `range(...)` or `a..b`, `while`, and
calls of other such functions, itself included. So is a lambda of that kind given a
number, as `map` and `filter` give one: `xs | filter((x) => x % 3 == 0)`.
It then runs as it would have, only faster: `fib(32)` takes 11 ms (Node 11, Bun 21),
and a loop of ten million steps 29 ms (Node and Bun take about 280 ms for the same
loop, on doubles).

- The answers are the interpreter's: ints are 64-bit and overflow is an error, `%` of
  ints by zero is an error, ordering NaN (`<`, `>`) is an error while `NaN == NaN` is
  false, and the 10,000-call limit holds. Where the interpreter would fail, the
  machine code gives up and the call is run again by the interpreter, which fails as
  always (the function changed nothing, so running it twice is running it once).
- What it does not take runs in the interpreter as before: strings, lists, maps,
  decimals, builtins, globals that are not functions, functions inside functions, a variable that
  changes type, and `/` of two ints, whose answer is an int or a float depending on the
  numbers (`6 / 3` is 2, `7 / 2` is 3.5).
- A function it calls that is assigned something else is not called by the old code.
- Not with a fuel limit (scripts given a budget of steps), and not in builds without
  the `jit` feature (`cargo build --no-default-features --features mysql`).
  `QUILLO_JIT=0` runs everything in the interpreter, to compare the two.

A loop goes to machine code by itself, wherever it is: in a function that does other
things too, or at a script's top. Its first 256 rounds run in the interpreter; a loop that
has gone that far is compiled for the types its variables have then, and goes on from the
round it is at. Its body may use ints, floats and bools, lists of them and lists of such
lists held in variables: `xs[i]`, `g[y][x]`, `len(xs)`, and `xs = xs | set(i, v)`, `g =
set(g, y, set(g[y], x, v))` and `xs = xs | push(v)` in any of their forms; texts: `s[i]`
(a character, as the interpreter gives it), `let c = s[i]`, `len(s)`, a character compared
with a quoted text, another character or `null`, and `s += x` or `s = s + x`; counting into a
map by an int key, `m = m | set(k, get(m, k, d) + e)` (also `-`, `*` and `set(m, ...)`, with a
number for `d`); `let`, assignment, `if`, `for` over a range and `while`. A list read in a
loop costs about 4 ns a round, written about 5, where the interpreter takes 45 (FEEDBACK29.md);
a text read by index about 1. A loop that appends to a text and reads it by index in the
same loop is not compiled.

- What it does is what the interpreter does: an error is the interpreter's own, made from
  the same values (an overflow, a `%` by zero, a `set` past the end, a step of 0).
- An item of another type than the code was made for, or a read past the end, gives the
  round back to the interpreter, with its numbers as the round began. A list written
  cannot be put back, so a loop that reads after writing in the same round is not
  compiled, and runs as before.
- A list it writes is taken out of its variable while it runs, and put back after; one
  only read is held. What else holds the list keeps what it had.
- Not in a server for a global the loop assigns (what follows it must be told), not under
  the debugger, with [`--max-compute`](#options-of-quillo-run), in a script with a budget, or in a cached view.
