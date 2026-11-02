# Rust, from the basics to advanced

For people who already write Quillo. Most examples show the same program twice, Quillo on the left and Rust on the right, so each Rust idea starts from something you know. Quillo is itself written in Rust, so chapters link to the place in its source where the idea is used.

**How to read the examples**

- Where Quillo has nothing to compare, as with lifetimes and traits, the two columns show the Rust that does not compile and the Rust that does, or two ways to write the same thing. The column headings say which.
- Rust snippets go inside `fn main() { ... }` unless they show their own `main`. Paste one into `src/main.rs`, wrap it, and `cargo run`.
- `// → text` is what the program prints at that point.
- `error[E0382]` and the like are the compiler's own error codes. `rustc --explain E0382` explains any of them.
- The examples were compiled and run with Rust 1.99 (2024 edition) and Quillo 0.6. New Rust projects use the 2024 edition. Quillo's own code is on 2021, which differs in nothing shown here except `unsafe extern` in chapter 26.

## Contents

- [Part 1: The basics](#part-1-the-basics)
  - [1. Setup and a first program](#1-setup-and-a-first-program)
  - [2. Variables and mutability](#2-variables-and-mutability)
  - [3. Numbers](#3-numbers)
  - [4. Strings](#4-strings)
  - [5. Functions](#5-functions)
  - [6. Control flow](#6-control-flow)
  - [7. Collections](#7-collections)
- [Part 2: Ownership](#part-2-ownership)
  - [8. Ownership and moves](#8-ownership-and-moves)
  - [9. Borrowing](#9-borrowing)
  - [10. Slices](#10-slices)
- [Part 3: Modelling data](#part-3-modelling-data)
  - [11. Structs and methods](#11-structs-and-methods)
  - [12. Enums and `match`](#12-enums-and-match)
  - [13. `Option` instead of `null`](#13-option-instead-of-null)
  - [14. `Result` and error handling](#14-result-and-error-handling)
- [Part 4: Abstraction](#part-4-abstraction)
  - [15. Generics](#15-generics)
  - [16. Traits](#16-traits)
  - [17. Iterators and closures](#17-iterators-and-closures)
  - [18. Modules and crates](#18-modules-and-crates)
  - [19. Tests](#19-tests)
- [Part 5: Advanced](#part-5-advanced)
  - [20. Lifetimes](#20-lifetimes)
  - [21. Smart pointers](#21-smart-pointers)
  - [22. Threads and shared state](#22-threads-and-shared-state)
  - [23. Async](#23-async)
  - [24. Advanced traits](#24-advanced-traits)
  - [25. Macros](#25-macros)
  - [26. Unsafe and calling C](#26-unsafe-and-calling-c)
- [Part 6: Putting it together](#part-6-putting-it-together)
  - [27. A tiny Quillo](#27-a-tiny-quillo)
  - [28. Writing Rust for a Quillo program](#28-writing-rust-for-a-quillo-program)
- [Cheat sheet](#cheat-sheet)
- [Where to go next](#where-to-go-next)

## Part 1: The basics

### 1. Setup and a first program

Install Rust with `rustup`, then make a project:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
cargo new hello
cd hello
cargo run
```

`cargo new` writes `Cargo.toml`, which names the project and its dependencies, and `src/main.rs`, which holds the program.

| Job | Quillo | Rust |
|---|---|---|
| Run | `quillo run main.qwe` | `cargo run` |
| Check without running | `quillo check main.qwe` | `cargo check` |
| Build for deployment | `quillo build main.qwe` | `cargo build --release` |
| Add a dependency | `quillo install csv` | `cargo add csv` |
| Find likely mistakes | `quillo check main.qwe` | `cargo clippy` |
| Format | no command | `cargo fmt` |
| Test | no command | `cargo test` |

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
print("Hello, world")  // → Hello, world
```

</td>
<td valign="top">

```rust
fn main() {
    println!("Hello, world"); // → Hello, world
}
```

</td>
</tr>
</table>

Three things differ already:

- A Rust program starts at `main`. Nothing runs at the top level of a file.
- `println!` ends in `!` because it is a macro, not a function. Chapter 25 covers macros; for now, the `!` is just part of the name.
- Statements end with `;`.

The largest difference is one you cannot see here. `cargo` compiles the program to a native binary before anything runs. `quillo run` checks a program first too, but it can only see what its annotations and values tell it, so a function with untyped parameters waits until the call to find out. Rust requires the types of every function, so a type mistake anywhere stops the build.

### 2. Variables and mutability

Quillo's `let` makes a variable you can reassign. In Rust you have to ask for that with `mut`.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let count = 0
count = count + 1
count += 1
print(count)  // → 2
```

</td>
<td valign="top">

```rust
let mut count = 0;
count = count + 1;
count += 1;
println!("{count}"); // → 2
```

</td>
</tr>
</table>

Without `mut`, assigning is an error:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0384
let count = 0;
count += 1;
// error[E0384]: cannot assign twice
// to immutable variable `count`
```

</td>
<td valign="top">

```rust
let mut count = 0;
count += 1;
println!("{count}"); // → 1
```

</td>
</tr>
</table>

A second `let` makes a new variable with the same name, and it may have a different type. This is called shadowing, and it works like `let x` inside a Quillo block, except that it also works in the same block.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let x = 1
if true {
    let x = "inner"
    print(x)  // → inner
}
print(x)      // → 1
```

</td>
<td valign="top">

```rust
let x = "5";
let x: i32 = x.parse().unwrap();
let x = x + 1;
println!("{x}"); // → 6
```

</td>
</tr>
</table>

Constants use `const`, need a written type, and are worked out at compile time:

```rust
const MAX_USERS: u32 = 10_000;
println!("{MAX_USERS}"); // → 10000
```

`println!("{count}")` fills in a variable by name, like `${count}` in a template string, but the braces take only a name. For an expression, pass it as an argument: `println!("{}", a + b)`. `{:?}` prints the debug form, which is what you use for lists and `Option`s.

### 3. Numbers

Quillo has `int` and `float`. Rust has one type per size:

| Quillo | Rust | Notes |
|---|---|---|
| `int` | `i64` | also `i8`, `i16`, `i32`, `i128`, and unsigned `u8` to `u128` |
| list index | `usize` | the machine's size; lengths and indexes use it |
| `float` | `f64` | also `f32` |
| `dec` | none in `std` | Quillo has its own in [src/decimal.rs](src/decimal.rs) |
| `bool` | `bool` | |

A whole number with no other hint is `i32`, a decimal number is `f64`.

**Division** is the first trap. Quillo's `/` is exact. In Rust, `/` on integers throws the remainder away:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
print(7 / 2)         // → 3.5
print(floor(7 / 2))  // → 3
print(7 % 2)         // → 1
```

</td>
<td valign="top">

```rust
println!("{}", 7.0 / 2.0); // → 3.5
println!("{}", 7 / 2);     // → 3
println!("{}", 7 % 2);     // → 1
```

</td>
</tr>
</table>

**Nothing converts by itself.** Quillo mixes an int and a float freely. Rust refuses to mix even two sizes of integer:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0308
let a: i32 = 5;
let b: i64 = 10;
let c = a + b;
// error[E0308]: mismatched types
```

</td>
<td valign="top">

```rust
let a: i32 = 5;
let b: i64 = 10;
let c = i64::from(a) + b;
let d = a as f64 / 2.0;
println!("{c} {d}"); // → 15 2.5
```

</td>
</tr>
</table>

`i64::from` is for conversions that always work. `as` always compiles too, but it can lose information: `300_i32 as u8` silently gives 44. Prefer `from`, and `try_from` when a value may not fit.

**Overflow.** In Quillo, an int past 64 bits is the error `integer overflow`. Plain `+` in Rust panics in a debug build and **wraps around silently in a release build**. When it matters, say what you want:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
// The largest int, then one more.
// Quillo stops with: integer overflow
let big = 9223372036854775807
print(big)  // → 9223372036854775807
```

</td>
<td valign="top">

```rust
let big = i64::MAX;
let a = big.checked_add(1);
println!("{a:?}");  // → None
let b = big.wrapping_add(1);
println!("{b}");    // → -9223372036854775808
let c = big.saturating_add(1);
println!("{c}");    // → 9223372036854775807
```

</td>
</tr>
</table>

This is how Quillo does it. The line that handles `1 + 1` is in `binop` in [src/interp.rs](src/interp.rs): `x.checked_add(y).map(Int).ok_or_else(overflow)`. By chapter 14 you will be able to read all of it.

### 4. Strings

Quillo has one string type. Rust has two that matter:

- `String` owns its text and can grow. Think of a buffer on the heap.
- `&str`, said "string slice", is a read-only view of text that lives somewhere else. Literals like `"hello"` are `&str`.

So `"a" + "b"`, which is fine in Quillo, does not compile, because adding two views would need a new buffer and Rust does not allocate behind your back.

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0369
let s = "a" + "b" + "c";
// error[E0369]: cannot add `&str`
// to `&str`
```

</td>
<td valign="top">

```rust
let s = format!("{}{}{}", "a", "b", "c");
println!("{s}"); // → abc

let t = "a".to_string() + "b" + "c";
println!("{t}"); // → abc
```

</td>
</tr>
</table>

`format!` is the one to reach for. It is what Quillo itself uses for `"a" + "b"`, in `binop` in [src/interp.rs](src/interp.rs). Chapter 24 explains why `String + &str` is allowed and `&str + &str` is not.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let name = "Ada"
let age = 36
print(`${name} is ${age}`)
// → Ada is 36
print("n=" + 1)         // → n=1
print(upper("abc"))
// → ABC
print(trim("  hi  "))  // → hi
print(replace("a-b", "-", "+"))
// → a+b
print(contains("abc", "b"))
// → true

print(split("a,b", ","))
// → ["a", "b"]
```

</td>
<td valign="top">

```rust
let name = "Ada";
let age = 36;
println!("{name} is {age}");
// → Ada is 36
println!("n={}", 1);    // → n=1
println!("{}", "abc".to_uppercase());
// → ABC
println!("{}", "  hi  ".trim()); // → hi
println!("{}", "a-b".replace("-", "+"));
// → a+b
println!("{}", "abc".contains("b"));
// → true
let parts: Vec<&str> =
    "a,b".split(',').collect();
println!("{parts:?}");
// → ["a", "b"]
```

</td>
</tr>
</table>

**Length and indexing.** Rust strings are UTF-8 bytes, and the API does not pretend otherwise. `.len()` counts bytes, and indexing by position does not compile, because the Nth byte may be the middle of a character.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let s = "héllo"
print(len(s))  // → 5
print(s[1])    // → é
```

</td>
<td valign="top">

```rust
let s = "héllo";
println!("{}", s.len());           // → 6
println!("{}", s.chars().count()); // → 5
println!("{:?}", s.chars().nth(1));
// → Some('é')
```

</td>
</tr>
</table>

To convert between the two types: `"x".to_string()` or `String::from("x")` gives a `String`, and `&s` or `s.as_str()` gives a `&str`.

### 5. Functions

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn add(a: int, b: int) -> int = a + b

fn area(w, h) {
    let a = w * h
    a
}

print(add(1, 2))   // → 3
print(area(2, 3))  // → 6
```

</td>
<td valign="top">

```rust
fn add(a: i64, b: i64) -> i64 {
    a + b
}

fn area(w: f64, h: f64) -> f64 {
    let a = w * h;
    a
}

println!("{}", add(1, 2));     // → 3
println!("{}", area(2.0, 3.0)); // → 6
```

</td>
</tr>
</table>

- Parameter and return types are required. Inside a function, Rust works types out itself.
- As in Quillo, a block's last expression is its value, and `return` is only for leaving early.
- The semicolon matters. `a + b` gives a value; `a + b;` throws it away and the block gives `()`, the empty value, which is Rust's nearest thing to `null` from a function with nothing to return.
- There are no default parameters and no overloading. The usual substitutes are a second function, or a struct of options with `Default` (chapter 11).

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0308
fn add(a: i64, b: i64) -> i64 {
    a + b;
}
// error[E0308]: mismatched types
// expected `i64`, found `()`
```

</td>
<td valign="top">

```rust
fn add(a: i64, b: i64) -> i64 {
    a + b
}
println!("{}", add(1, 2)); // → 3
```

</td>
</tr>
</table>

**Closures** are Quillo's arrow functions, written with bars:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let double = (x) => x * 2
let base = 10
let plus = (x) => x + base
print(double(4))  // → 8
print(plus(4))    // → 14
```

</td>
<td valign="top">

```rust
let double = |x| x * 2;
let base = 10;
let plus = |x| x + base;
println!("{}", double(4)); // → 8
println!("{}", plus(4));   // → 14
```

</td>
</tr>
</table>

### 6. Control flow

`if` is an expression, as in Quillo. Rust has no `? :` because `if` does the job. The condition must be a `bool`: there is no truthy or falsy, so `if count { }` does not compile. Write `if count != 0`.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let n = 5
let word = n > 0 ? "pos" : "neg"
print(word)  // → pos

let size = if n > 3 {
    "big"
} else {
    "small"
}
print(size)  // → big
```

</td>
<td valign="top">

```rust
let n = 5;
let word =
    if n > 0 { "pos" } else { "neg" };
println!("{word}"); // → pos

let size = if n > 3 {
    "big"
} else {
    "small"
};
println!("{size}"); // → big
```

</td>
</tr>
</table>

`for` looks the same, with one difference: a Quillo `for` gives back a list, and a Rust `for` gives nothing. To make a list in Rust, push in a loop, or use an iterator (chapter 17).

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
for i in 0..3 {
    print(i)
}
// → 0
// → 1
// → 2

let squares = for x in [1, 2, 3] => x * x
print(squares)  // → [1, 4, 9]
```

</td>
<td valign="top">

```rust
for i in 0..3 {
    println!("{i}");
}
// → 0
// → 1
// → 2

let mut squares = Vec::new();
for x in [1, 2, 3] {
    squares.push(x * x);
}
println!("{squares:?}"); // → [1, 4, 9]
```

</td>
</tr>
</table>

Quillo has `while` and leaves a loop only with `return`. Rust adds `loop`, which runs until a `break`, plus `continue`. `break` can give a value, and a label lets it leave an outer loop.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let n = 1
while n < 100 {
    n = n * 2
}
print(n)  // → 128

fn find_pair() {
    for a in 1..5 {
        for b in 1..5 {
            if a * b == 6 {
                return [a, b]
            }
        }
    }
}
print(find_pair())  // → [2, 3]
```

</td>
<td valign="top">

```rust
let mut n = 1;
let result = loop {
    n *= 2;
    if n >= 100 { break n; }
};
println!("{result}"); // → 128

let mut pair = None;
'outer: for a in 1..5 {
    for b in 1..5 {
        if a * b == 6 {
            pair = Some((a, b));
            break 'outer;
        }
    }
}
println!("{pair:?}"); // → Some((2, 3))
```

</td>
</tr>
</table>

### 7. Collections

Quillo has lists and maps, and they never change: `push` returns a new list. Rust's `Vec` is changed in place, so it must be `mut`.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let xs = [1, 2, 3]
let ys = xs | push(4)
print(xs)      // → [1, 2, 3]
print(ys)      // → [1, 2, 3, 4]
print(len(ys)) // → 4
print(xs[0])   // → 1
print(xs[-1])  // → 3
print(xs[99])  // → null
```

</td>
<td valign="top">

```rust
let mut xs = vec![1, 2, 3];
xs.push(4);
println!("{xs:?}");     // → [1, 2, 3, 4]
println!("{}", xs.len()); // → 4
println!("{}", xs[0]);  // → 1
println!("{:?}", xs.last());  // → Some(4)
println!("{:?}", xs.get(99)); // → None
```

</td>
</tr>
</table>

`xs[99]` in Rust panics and stops the program. Use `.get()` when an index might be missing. Every item of a `Vec` has one type, so `[1, "two"]` needs an enum (chapter 12).

`HashMap` has no order, and `BTreeMap` keeps its keys sorted. These examples use `BTreeMap` so the output stays the same on every run; the methods are the same.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let ages = { ada: 36 }
let more = set(ages, "bob", 40)
print(more.bob)      // → 40
print(get(more, "cy"))  // → null
print(keys(more))    // → ["ada", "bob"]
print(remove(more, "ada"))
// → { bob: 40 }
```

</td>
<td valign="top">

```rust
use std::collections::BTreeMap;
let mut ages = BTreeMap::new();
ages.insert("ada", 36);
ages.insert("bob", 40);
println!("{:?}", ages.get("bob"));
// → Some(40)
println!("{:?}", ages.get("cy")); // → None
let names: Vec<_> = ages.keys().collect();
println!("{names:?}"); // → ["ada", "bob"]
ages.remove("ada");
println!("{ages:?}");
// → {"bob": 40}
```

</td>
</tr>
</table>

The `entry` method handles "update or insert", the usual way to count things:

```rust
use std::collections::BTreeMap;
let mut counts = BTreeMap::new();
for word in "a b a".split(' ') {
    *counts.entry(word).or_insert(0) += 1;
}
println!("{counts:?}"); // → {"a": 2, "b": 1}
```

A **tuple** is a fixed group of values of different types, the job Quillo gives to a short list. An **array** has a fixed length and lives on the stack, and a `HashSet` or `BTreeSet` holds unique values.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let pair = ["ada", 36]
let [name, age] = pair
print(name)  // → ada
print(age)   // → 36
```

</td>
<td valign="top">

```rust
let pair = ("ada", 36);
let (name, age) = pair;
println!("{name}");   // → ada
println!("{}", pair.1); // → 36
```

</td>
</tr>
</table>

**Try it.** Write `fn word_lengths(text: &str) -> Vec<usize>`, which returns the length of each word. Then write `fn fizzbuzz(n: u32) -> String`, using `if` as an expression.

## Part 2: Ownership

This part is what makes Rust different from other languages. It takes time to get used to, and the rest of Rust gets easier once it does.

### 8. Ownership and moves

Quillo has a runtime that keeps values alive until nothing uses them. Rust has no garbage collector and no runtime doing that job. It decides at compile time when each value is freed, using three rules:

1. Every value has one owner, a variable.
2. There is only one owner at a time.
3. When the owner goes out of scope, the value is freed.

So assigning a `String` does not copy it. It **moves** it, and the old name can't be used any more.

<table>
<tr><th>Quillo</th><th>Rust that doesn't compile</th></tr>
<tr>
<td valign="top">

```quillo
let a = "hello"
let b = a
print(a)  // → hello
print(b)  // → hello
```

</td>
<td valign="top">

```rust compile_fail=E0382
let a = String::from("hello");
let b = a;        // moved here
println!("{a}");
// error[E0382]: borrow of moved
// value: `a`
```

</td>
</tr>
</table>

Why does Quillo get away with it? Quillo values are reference-counted. `Value::Str` holds an `Arc<str>`, as `Value::str` in [src/value.rs](src/value.rs) shows, so `let b = a` adds a second owner, and text is never changed in place, which makes sharing safe. Rust offers the same thing as a choice (chapter 21), not a default.

There are two ways to fix the example. Copy the text with `clone`, or borrow it (next chapter):

<table>
<tr><th>Explicit copy</th><th>Borrow</th></tr>
<tr>
<td valign="top">

```rust
let a = String::from("hello");
let b = a.clone();
println!("{a} {b}"); // → hello hello
```

</td>
<td valign="top">

```rust
let a = String::from("hello");
let b = &a;
println!("{a} {b}"); // → hello hello
```

</td>
</tr>
</table>

Small values held entirely on the stack, like integers, floats, `bool`, `char` and `&str` itself, are `Copy`: assigning them copies them, and the original stays usable. That is why the earlier chapters could reuse numbers freely.

Passing to a function moves too, and so does returning:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0382
fn shout(s: String) -> String {
    s.to_uppercase()
}
let name = String::from("ada");
let loud = shout(name); // moved
println!("{name}");
// error[E0382]
```

</td>
<td valign="top">

```rust
fn shout(s: &str) -> String {
    s.to_uppercase()
}
let name = String::from("ada");
let loud = shout(&name); // borrowed
println!("{name} {loud}"); // → ada ADA
```

</td>
</tr>
</table>

### 9. Borrowing

A reference lets you use a value without owning it. There are two kinds:

- `&T` is a shared reference. You can read through it, and you can have any number at once.
- `&mut T` is a mutable reference. You can change the value through it, and it must be the only reference at that moment.

<table>
<tr><th>Read through a reference</th><th>Change through a reference</th></tr>
<tr>
<td valign="top">

```rust
fn total(xs: &Vec<i32>) -> i32 {
    let mut sum = 0;
    for x in xs {
        sum += x;
    }
    sum
}
let xs = vec![1, 2, 3];
println!("{}", total(&xs)); // → 6
println!("{xs:?}");  // → [1, 2, 3]
```

</td>
<td valign="top">

```rust
fn add_one(xs: &mut Vec<i32>) {
    for x in xs.iter_mut() {
        *x += 1;
    }
}
let mut xs = vec![1, 2, 3];
add_one(&mut xs);
println!("{xs:?}"); // → [2, 3, 4]
```

</td>
</tr>
</table>

`*x` follows the reference to the value it points at. For most method calls and field reads Rust does this for you; you write the `*` mainly to assign.

The rule of **many readers or one writer** catches real bugs. Pushing to a `Vec` can move its items to a bigger buffer, which would leave `first` pointing at freed memory:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0502
let mut xs = vec![1, 2, 3];
let first = &xs[0];
xs.push(4);
println!("{first}");
// error[E0502]: cannot borrow `xs`
// as mutable because it is also
// borrowed as immutable
```

</td>
<td valign="top">

```rust
let mut xs = vec![1, 2, 3];
let first = xs[0]; // copies the i32
xs.push(4);
println!("{first}"); // → 1
```

</td>
</tr>
</table>

A borrow ends at its last use, not at the end of the block. Moving the `println!` above the `push` in the failing example also fixes it.

Quillo avoids all this differently: values never change, so a list you are reading can't change under you. Rust allows change and proves that nobody is watching when it happens.

### 10. Slices

A slice is a view of part of a collection: `&[T]` for lists and `&str` for strings. A function that takes a slice accepts a `Vec`, an array or a part of either, so it's better than `&Vec<T>` and `&String` for parameters.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let xs = [10, 20, 30, 40]
print(slice(xs, 1, 3))  // → [20, 30]

fn total(xs) = sum(xs)
print(total(xs))        // → 100
print(total([1, 2]))    // → 3
```

</td>
<td valign="top">

```rust
let xs = vec![10, 20, 30, 40];
println!("{:?}", &xs[1..3]); // → [20, 30]

fn total(xs: &[i32]) -> i32 {
    xs.iter().sum()
}
println!("{}", total(&xs));     // → 100
println!("{}", total(&[1, 2])); // → 3
```

</td>
</tr>
</table>

String slices use byte positions, and a position inside a character panics. Find positions with methods such as `find` and `char_indices`, not by counting.

```rust
fn first_word(s: &str) -> &str {
    match s.find(' ') {
        Some(i) => &s[..i],
        None => s,
    }
}
println!("{}", first_word("hello world")); // → hello
```

`first_word` makes no copy. It returns a view into the text it was given, which is the kind of speed Rust is good at, and the compiler keeps the original alive for as long as the view is used.

**Try it.** Write `fn largest(xs: &[i32]) -> Option<i32>`. Then write `fn double_all(xs: &mut [i32])`. Call both on the same `Vec`, and print it after each.

## Part 3: Modelling data

### 11. Structs and methods

A Quillo `type` names a shape, and at run time it is still a map. A Rust `struct` is a real type with fixed fields, and methods go in an `impl` block.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
type User = { name: str, age: int }

fn greet(u: User) = `Hi ${u.name}`
fn birthday(u: User) = { ...u, age: u.age + 1 }

let ada = User({ name: "Ada", age: 36 })
print(ada.greet())         // → Hi Ada
print(ada.birthday().age)  // → 37
print(ada.age)             // → 36
```

</td>
<td valign="top">

```rust
struct User { name: String, age: u32 }

impl User {
    fn new(name: &str, age: u32) -> User {
        User { name: name.to_string(), age }
    }
    fn greet(&self) -> String {
        format!("Hi {}", self.name)
    }
    fn birthday(&mut self) {
        self.age += 1;
    }
}

let mut ada = User::new("Ada", 36);
println!("{}", ada.greet()); // → Hi Ada
ada.birthday();
println!("{}", ada.age);     // → 37
```

</td>
</tr>
</table>

The first parameter of a method says how it uses the value:

| Written | Means | Quillo's nearest |
|---|---|---|
| `&self` | reads it | any function |
| `&mut self` | changes it | none: Quillo returns a new value |
| `self` | takes it, and the caller loses it | none |
| no `self` | belongs to the type: `User::new(...)` | `User({...})` |

Quillo's `x.f(y)` means `f(x, y)`, and a Rust method call works much the same way: `ada.greet()` is `User::greet(&ada)`. `new` is only a naming convention, not a keyword.

Quillo's `{ ...u, age: 37 }` has a direct equivalent. `..` fills the rest from another value:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let a = { name: "Ada", age: 36, on: true }

let b = { ...a, age: 37 }
print(b)
// → { name: "Ada", age: 37, on: true }
```

</td>
<td valign="top">

```rust
#[derive(Debug, Clone)]
struct U { name: String, age: u32, on: bool }

let a =
    U { name: "Ada".into(), age: 36, on: true };
let b = U { age: 37, ..a.clone() };
println!("{b:?}");
// → U { name: "Ada", age: 37, on: true }
```

</td>
</tr>
</table>

`#[derive(...)]` asks the compiler to write code for you. `Debug` makes `{:?}` work, `Clone` adds `.clone()`, `PartialEq` adds `==`, and `Default` gives a value with every field at its default. Default values for options look like this:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
type Opts = {
    port: int = 8080,
    host: str = "::1",
}
let o = Opts({ port: 3000 })
print(o.port)  // → 3000
print(o.host)  // → ::1
```

</td>
<td valign="top">

```rust
#[derive(Debug)]
struct Opts { port: u16, host: String }
impl Default for Opts {
    fn default() -> Opts {
        Opts { port: 8080, host: "::1".into() }
    }
}
let o =
    Opts { port: 3000, ..Default::default() };
println!("{}", o.port); // → 3000
println!("{}", o.host); // → ::1
```

</td>
</tr>
</table>

A **tuple struct** gives a plain value a name of its own, called the newtype pattern. The compiler then stops you passing an order id where a user id belongs:

```rust
struct UserId(u64);
struct OrderId(u64);

fn load_user(id: UserId) -> u64 { id.0 }

println!("{}", load_user(UserId(7))); // → 7
// load_user(OrderId(7)) does not compile
```

### 12. Enums and `match`

A Quillo `"admin" | "member"` type lists strings, checked when the program runs. A Rust `enum` lists the cases of a type, each case can hold its own data, and `match` must handle every case. This is the most useful feature in the language.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn area(s) = match s {
    { kind: "circle", r } => 3 * r * r
    { kind: "rect", w, h } => w * h
}

let box = { kind: "rect", w: 2, h: 3 }
print(area(box))  // → 6
```

</td>
<td valign="top">

```rust
enum Shape {
    Circle { r: f64 },
    Rect { w: f64, h: f64 },
}

fn area(s: &Shape) -> f64 {
    match s {
        Shape::Circle { r } => 3.0 * r * r,
        Shape::Rect { w, h } => w * h,
    }
}

let b = Shape::Rect { w: 2.0, h: 3.0 };
println!("{}", area(&b)); // → 6
```

</td>
</tr>
</table>

In Quillo a `match` no arm takes is a runtime error. In Rust it does not compile, so adding a new case to an enum makes the compiler list every `match` that needs updating:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0004
enum Role { Admin, Member, Guest }

fn level(r: Role) -> u8 {
    match r {
        Role::Admin => 2,
        Role::Member => 1,
    }
}
// error[E0004]: non-exhaustive
// patterns: `Role::Guest` not covered
```

</td>
<td valign="top">

```rust
enum Role { Admin, Member, Guest }

fn level(r: Role) -> u8 {
    match r {
        Role::Admin => 2,
        Role::Member => 1,
        Role::Guest => 0,
    }
}
println!("{}", level(Role::Guest)); // → 0
```

</td>
</tr>
</table>

Quillo itself is built on this. Each kind of expression in a program is one case of `Expr` in [src/ast.rs](src/ast.rs), each kind of value is one case of `Value` in [src/value.rs](src/value.rs), and `eval` in [src/interp.rs](src/interp.rs) is a big `match`.

Patterns work much as they do in Quillo:

| Pattern | Quillo | Rust |
|---|---|---|
| Anything | `_` | `_` |
| A literal | `42`, `"order"` | `42`, `"order"` |
| Either | `"a" \| "b"` | `"a" \| "b"` |
| A range | use a guard | `1..=9` |
| Bind and test | `int as n` | `n @ 1..=9` |
| Guard | `int as n if n < 0` | `n if n < 0` |
| List start | `[a, b, ..rest]` | `[a, b, rest @ ..]` |

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn describe(x) = match x {
    0 => "zero"
    int as n if n < 0 => "negative"
    [] => "empty"
    [first, ..rest] => `first is ${first}`
    _ => "something else"
}
print(describe(-4))      // → negative
print(describe([7, 8]))  // → first is 7
```

</td>
<td valign="top">

```rust
fn sign(x: i32) -> &'static str {
    match x {
        0 => "zero",
        n if n < 0 => "negative",
        _ => "positive",
    }
}
fn head(xs: &[i32]) -> String {
    match xs {
        [] => "empty".to_string(),
        [first, ..] => {
            format!("first is {first}")
        }
    }
}
println!("{}", sign(-4)); // → negative
println!("{}", head(&[7, 8]));
// → first is 7
```

</td>
</tr>
</table>

For a single case, a whole `match` is too much, so Rust has shorter forms:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn first_big(xs) {
    let x = find(xs, (x) => x > 1)
        ?? return null
    x * 2
}
print(first_big([1, 2]))  // → 4
print(first_big([1]))     // → null
```

</td>
<td valign="top">

```rust
fn first_big(xs: &[i32]) -> Option<i32> {
    let hit = xs.iter().find(|x| **x > 1);
    let Some(x) = hit else {
        return None;
    };
    Some(x * 2)
}
println!("{:?}", first_big(&[1, 2]));
// → Some(4)
println!("{:?}", first_big(&[1])); // → None

if let Some(x) = first_big(&[3]) {
    println!("{x}"); // → 6
}
```

</td>
</tr>
</table>

`if let` runs a block when a pattern matches. `let ... else` is for leaving when it doesn't. `while let` loops while it does, and `matches!(x, pattern)` gives a `bool`.

### 13. `Option` instead of `null`

Rust has no `null`. A value that may be missing has the type `Option<T>`, an ordinary enum from the standard library:

```rust
enum Option<T> {
    Some(T),
    None,
}
```

The compiler won't let you use an `Option<i32>` as an `i32`, so "forgot to check for null" can't happen.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let names = ["ada", "bob"]
let found = find(names, (n) => n == "cy")
print(found ?? "nobody")  // → nobody

let hit = find(names, (n) => n == "bob")
print(upper(hit))         // → BOB
```

</td>
<td valign="top">

```rust
let names = ["ada", "bob"];
let mut it = names.iter().copied();
let found = it.find(|n| *n == "cy");
println!("{}", found.unwrap_or("nobody"));
// → nobody

let mut it = names.iter().copied();
let hit = it.find(|n| *n == "bob");
if let Some(n) = hit {
    println!("{}", n.to_uppercase());
    // → BOB
}
```

</td>
</tr>
</table>

The methods you will use most:

| Rust | Quillo's nearest | Does |
|---|---|---|
| `x.unwrap_or(d)` | `x ?? d` | the value, or `d` |
| `x.unwrap_or_else(f)` | `x ?? f()` | `f` runs only if needed |
| `x.map(f)` | `x != null ? f(x) : null` | changes the value if there is one |
| `x.and_then(f)` | | as `map`, for an `f` that gives an `Option` |
| `x?` | `x?` | returns `None` from the function if `None` |
| `x.is_some()` | `x != null` | a `bool` |
| `x.unwrap()` | | the value, or a **panic** |

`unwrap` is fine in examples and tests. In real code, prefer one of the others, or `.expect("why this can't be None")`, which at least says what you assumed.

### 14. `Result` and error handling

Quillo has error values, `err(x)`, and `?` and `??` to deal with them. Rust's are the same idea with a type. `Result<T, E>` is an enum of `Ok(T)` and `Err(E)`:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn parse_age(s: str) -> int ! str =
    match int(s) {
        int as n if n >= 0 => n
        _ => err(`bad age: ${s}`)
    }

fn next_year(s) = parse_age(s)? + 1

print(next_year("41"))
// → 42
print(next_year("x"))
// → err("bad age: x")
print(next_year("x") ?? 18)
// → 18
```

</td>
<td valign="top">

```rust
fn parse_age(s: &str) -> Result<i64, String> {
    match s.parse::<i64>() {
        Ok(n) if n >= 0 => Ok(n),
        _ => Err(format!("bad age: {s}")),
    }
}

fn next_year(s: &str) -> Result<i64, String> {
    Ok(parse_age(s)? + 1)
}

println!("{:?}", next_year("41"));
// → Ok(42)
println!("{:?}", next_year("x"));
// → Err("bad age: x")
println!("{}", next_year("x").unwrap_or(18));
// → 18
```

</td>
</tr>
</table>

`?` does the same thing in both languages: give back the value, or return the error from this function now. The difference is that Rust won't let you ignore the `Result`. You can't use it as a number until you've dealt with the error.

Now the line from chapter 3 reads easily. `x.checked_add(y)` gives an `Option<i64>`, `.map(Int)` wraps the number in the `Value::Int` case, and `.ok_or_else(overflow)` turns `None` into an `Err`, so the result is a `Result<Value, Ctrl>`:

```rust
#[derive(Debug)]
enum Value { Int(i64) }

fn add(x: i64, y: i64) -> Result<Value, String> {
    let overflow = || "integer overflow".to_string();
    x.checked_add(y).map(Value::Int).ok_or_else(overflow)
}
println!("{:?}", add(1, 1));        // → Ok(Int(2))
println!("{:?}", add(i64::MAX, 1)); // → Err("integer overflow")
```

**Your own error type.** Strings are fine at first. A real program uses an enum, so callers can `match` on what went wrong. Implementing `From` lets `?` convert other errors into yours by itself:

```rust
use std::fmt;
use std::num::ParseIntError;

#[derive(Debug)]
enum AgeError {
    NotANumber(ParseIntError),
    Negative(i64),
}

impl fmt::Display for AgeError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            AgeError::NotANumber(e) => write!(f, "not a number: {e}"),
            AgeError::Negative(n) => write!(f, "negative age: {n}"),
        }
    }
}

impl std::error::Error for AgeError {}

impl From<ParseIntError> for AgeError {
    fn from(e: ParseIntError) -> AgeError {
        AgeError::NotANumber(e)
    }
}

fn parse_age(s: &str) -> Result<i64, AgeError> {
    let n: i64 = s.parse()?; // a ParseIntError becomes an AgeError here
    if n < 0 {
        return Err(AgeError::Negative(n));
    }
    Ok(n)
}

fn main() {
    println!("{:?}", parse_age("41").unwrap()); // → 41
    println!("{}", parse_age("-1").unwrap_err()); // → negative age: -1
    println!("{}", parse_age("x").unwrap_err());
    // → not a number: invalid digit found in string
}
```

The `thiserror` crate writes that code from attributes, and `anyhow` gives applications a single catch-all error type. For quick programs, `Box<dyn std::error::Error>` holds any error, and `main` itself can return a `Result`:

```rust
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let n: i32 = "42".parse()?;
    println!("{n}"); // → 42
    Ok(())
}
```

**Result or panic?** Quillo draws this line too. An error value is something that can happen, like bad input or a missing file: use `Result`. A runtime error is a mistake in the code, like calling a non-function: that's `panic!`. A panic unwinds the thread and normally ends the program.

**Try it.** Model a bank account with `enum Tx { Deposit(u64), Withdraw(u64) }`. Write `fn apply(balance: u64, tx: &Tx) -> Result<u64, String>`, which refuses to overdraw. Then fold a list of transactions with `?`.

## Part 4: Abstraction

### 15. Generics

A Quillo function works on any value and finds problems when it runs. A Rust function has to say what it accepts, and a **type parameter** is how it says "any type that can do this". `T: PartialOrd` is a bound, meaning any `T` that can be compared.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn largest(xs) {
    let top = xs[0]
    for x in xs {
        if x > top { top = x }
    }
    top
}
print(largest([1, 5, 2]))     // → 5
print(largest([1.5, 0.5]))    // → 1.5
print(largest(["a", "b"]))    // → b
```

</td>
<td valign="top">

```rust
fn largest<T: PartialOrd + Copy>(xs: &[T]) -> T {
    let mut top = xs[0];
    for &x in xs {
        if x > top { top = x; }
    }
    top
}
println!("{}", largest(&[1, 5, 2]));   // → 5
println!("{}", largest(&[1.5, 0.5]));  // → 1.5
println!("{}", largest(&["a", "b"]));  // → b
```

</td>
</tr>
</table>

The compiler generates a separate copy of the function for each type you use, so generics cost nothing at run time. A long list of bounds can move to a `where` clause. Structs and enums can be generic too, which is how `Vec<T>`, `Option<T>` and `Result<T, E>` work:

```rust
#[derive(Debug)]
struct Pair<T> {
    left: T,
    right: T,
}

impl<T> Pair<T> {
    fn swap(self) -> Pair<T> {
        Pair { left: self.right, right: self.left }
    }
}

let p = Pair { left: "a", right: "b" }.swap();
println!("{p:?}"); // → Pair { left: "b", right: "a" }
```

### 16. Traits

A trait is a set of methods a type promises to have, close to an interface in other languages. Quillo has no traits: it asks each value what it is. In Rust you declare the shared behaviour and the compiler checks every type against it.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn describe(x) = match x {
    { kind: "dog", name } => `dog ${name}`
    { kind: "cat", lives } => `cat, ${lives} lives`
}

print(describe({ kind: "dog", name: "Rex" }))
// → dog Rex
print(describe({ kind: "cat", lives: 9 }))
// → cat, 9 lives
```

</td>
<td valign="top">

```rust
trait Describe {
    fn describe(&self) -> String;
}

struct Dog { name: String }
struct Cat { lives: u8 }

impl Describe for Dog {
    fn describe(&self) -> String {
        format!("dog {}", self.name)
    }
}
impl Describe for Cat {
    fn describe(&self) -> String {
        format!("cat, {} lives", self.lives)
    }
}

let d = Dog { name: "Rex".into() };
println!("{}", d.describe()); // → dog Rex
let c = Cat { lives: 9 };
println!("{}", c.describe());
// → cat, 9 lives
```

</td>
</tr>
</table>

A trait can give a method a default body. You can implement your own trait for someone else's type, or someone else's trait for your own type, but not someone else's trait for someone else's type.

There are two ways to accept "anything with this trait":

<table>
<tr><th>Static: one copy per type</th><th>Dynamic: one copy, looked up at run time</th></tr>
<tr>
<td valign="top">

```rust
trait Area { fn area(&self) -> f64; }
struct Square(f64);
impl Area for Square {
    fn area(&self) -> f64 { self.0 * self.0 }
}

fn show(s: &impl Area) {
    println!("{}", s.area());
}
show(&Square(2.0)); // → 4
```

</td>
<td valign="top">

```rust
trait Area { fn area(&self) -> f64; }
struct Square(f64);
struct Unit;
impl Area for Square {
    fn area(&self) -> f64 { self.0 * self.0 }
}
impl Area for Unit {
    fn area(&self) -> f64 { 1.0 }
}

let shapes: Vec<Box<dyn Area>> =
    vec![Box::new(Square(2.0)), Box::new(Unit)];
let total: f64 =
    shapes.iter().map(|s| s.area()).sum();
println!("{total}"); // → 5
```

</td>
</tr>
</table>

Use `impl Trait` or generics by default; they're faster and can be inlined. Use `dyn Trait` when one list has to hold different types and an enum won't do, for example when other crates add types of their own.

Implementing the standard traits makes your type work with the rest of Rust:

| Trait | Gives you | Usually |
|---|---|---|
| `Debug` | `{:?}` | `#[derive]` |
| `Display` | `{}` and `.to_string()` | by hand |
| `Clone` / `Copy` | `.clone()` / copy on assign | `#[derive]` |
| `PartialEq` / `Eq` | `==` | `#[derive]` |
| `PartialOrd` / `Ord` | `<`, `.sort()` | `#[derive]` |
| `Hash` | a key in a `HashMap` | `#[derive]` |
| `Default` | `T::default()` | `#[derive]` |
| `From<U>` | `T::from(u)` and `u.into()` | by hand |
| `Iterator` | `for` and every iterator method | by hand |

### 17. Iterators and closures

Quillo pipelines become iterator chains. Start with `.iter()`, add steps, and end with something that produces a result, such as `.collect()` or `.sum()`.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let orders = [
    { id: 1, total: 50, paid: true },
    { id: 2, total: 150, paid: true },
    { id: 3, total: 300, paid: false },
]

let ids = orders
    | filter((o) => o.paid)
    | filter((o) => o.total > 100)
    | map((o) => o.id)
print(ids)  // → [2]

let totals = orders | map((o) => o.total)
print(sum(totals))  // → 500
```

</td>
<td valign="top">

```rust
struct Order { id: u32, total: u32, paid: bool }
let orders = [
    Order { id: 1, total: 50, paid: true },
    Order { id: 2, total: 150, paid: true },
    Order { id: 3, total: 300, paid: false },
];

let ids: Vec<u32> = orders.iter()
    .filter(|o| o.paid)
    .filter(|o| o.total > 100)
    .map(|o| o.id)
    .collect();
println!("{ids:?}"); // → [2]

let sum: u32 =
    orders.iter().map(|o| o.total).sum();
println!("{sum}"); // → 500
```

</td>
</tr>
</table>

Iterators are **lazy**: nothing runs until the final step, and no list is built between the steps. Rust compiles a chain down to about the same code as a hand-written loop.

`collect` can build many types, so it needs to know which one: write `let ids: Vec<u32>`, or use `.collect::<Vec<u32>>()`. The `::<>` form is known as the turbofish.

| Quillo | Rust |
|---|---|
| `xs \| map(f)` | `xs.iter().map(f)` |
| `xs \| filter(f)` | `xs.iter().filter(f)` |
| `find(xs, f)` | `xs.iter().find(f)` |
| `reduce(xs, 0, f)` | `xs.iter().fold(0, f)` |
| `sum(xs)` | `xs.iter().sum()` |
| `max(xs)` | `xs.iter().max()` |
| `sort_by(xs, f)` | `xs.sort_by_key(f)`, in place |
| `flatten(xs)` | `xs.iter().flatten()` |
| `first(xs)` | `xs.first()` |
| `for [i, x] in ...` over pairs | `.enumerate()`, `.zip(other)` |
| | `.take(n)`, `.skip(n)`, `.rev()`, `.chain(other)` |
| | `.any(f)`, `.all(f)`, `.count()`, `.flat_map(f)` |

Three methods start a chain, and they differ in ownership:

| Call | Items are | The collection afterwards |
|---|---|---|
| `xs.iter()` | `&T` | unchanged |
| `xs.iter_mut()` | `&mut T` | changed in place |
| `xs.into_iter()` | `T` | gone: its items were moved out |

A `for x in &xs` uses `iter()`, and a `for x in xs` uses `into_iter()`.

**Closures and capture.** A closure borrows what it uses when it can. `move` makes it take ownership, which it needs when it outlives the current scope, as with threads and returned closures.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn counter(step) {
    let n = 0
    () => {
        n += step
        n
    }
}
let next = counter(5)
print(next())  // → 5
print(next())  // → 10
```

</td>
<td valign="top">

```rust
fn counter(step: i32) -> impl FnMut() -> i32 {
    let mut n = 0;
    move || {
        n += step;
        n
    }
}
let mut next = counter(5);
println!("{}", next()); // → 5
println!("{}", next()); // → 10
```

</td>
</tr>
</table>

Each closure has one of three traits, depending on what it does with what it captured. `Fn` only reads and can be called any number of times. `FnMut` changes state, so calling it needs a `mut` binding. `FnOnce` uses something up, so it can be called once.

**Your own iterator.** Implement one method, `next`, and every method above comes with it:

```rust
struct Countdown(u32);

impl Iterator for Countdown {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        self.0 -= 1;
        Some(self.0 + 1)
    }
}

let v: Vec<u32> = Countdown(3).map(|x| x * 10).collect();
println!("{v:?}"); // → [30, 20, 10]
```

### 18. Modules and crates

| Quillo | Rust |
|---|---|
| a `.qwe` file | a module, `mod name;` |
| `use "maths.qwe"` | `mod maths;` to include it, `use` to shorten a path |
| `private` hides a name | everything is private unless `pub` |
| one namespace for the program | each module has its own |
| `.qwer` plugin | a crate from crates.io |

Quillo marks what to hide, and Rust marks what to show:

<table>
<tr><th>Quillo: maths.qwe, then main.qwe</th><th>Rust: one file</th></tr>
<tr>
<td valign="top">

```quillo no_run
// maths.qwe
fn add(a, b) = a + b
private fn secret() = 42

// main.qwe
use "maths.qwe"
print(add(1, 2))
```

</td>
<td valign="top">

```rust
mod maths {
    pub fn add(a: i32, b: i32) -> i32 {
        a + b + secret()
    }
    fn secret() -> i32 { 0 }
}

use maths::add;
println!("{}", add(1, 2)); // → 3
```

</td>
</tr>
</table>

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0603
mod maths {
    fn secret() -> i32 { 42 }
}
println!("{}", maths::secret());
// error[E0603]: function `secret`
// is private
```

</td>
<td valign="top">

```rust
mod maths {
    pub fn open() -> i32 { secret() }
    fn secret() -> i32 { 42 }
}
println!("{}", maths::open()); // → 42
```

</td>
</tr>
</table>

A module usually lives in a file of its own. `mod maths;` in `main.rs` loads `src/maths.rs`:

```text
my_app/
├── Cargo.toml
└── src/
    ├── main.rs      the binary, starts at fn main
    ├── lib.rs       the library, if there is one
    ├── maths.rs     `mod maths;`
    └── db/
        ├── mod.rs   `mod db;`
        └── pool.rs  `mod pool;` inside db/mod.rs
```

Paths start from `crate` (this project), `self` (this module) or `super` (the parent). `pub(crate)` means visible anywhere in this crate but not outside it. Quillo uses this layout, with each `mod` listed in [src/lib.rs](src/lib.rs).

A crate is one library or binary, and `Cargo.toml` lists the ones you depend on. `cargo add` writes that for you, and `Cargo.lock` records the exact versions, so commit it for applications.

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
```

### 19. Tests

Tests sit next to the code in a module that is built only for `cargo test`. Files in a top-level `tests/` folder are compiled separately and see only your public API. Quillo's own tests live there.

<table>
<tr><th>The code</th><th>Its tests</th></tr>
<tr>
<td valign="top">

```rust
fn is_even(n: i32) -> bool {
    n % 2 == 0
}

fn div(a: i32, b: i32) -> i32 {
    if b == 0 {
        panic!("division by zero");
    }
    a / b
}
println!("{}", is_even(4)); // → true
println!("{}", div(6, 2));  // → 3
```

</td>
<td valign="top">

```rust test
fn is_even(n: i32) -> bool { n % 2 == 0 }
fn div(a: i32, b: i32) -> i32 {
    if b == 0 { panic!("division by zero"); }
    a / b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_is_even() {
        assert!(is_even(4));
        assert_eq!(div(6, 2), 3);
    }

    #[test]
    #[should_panic(expected = "by zero")]
    fn zero_panics() {
        div(1, 0);
    }
}
```

</td>
</tr>
</table>

`cargo test` runs them all, and `cargo test four` runs those whose names contain `four`. Add `-- --nocapture` to see what tests print. Code examples in `///` doc comments are compiled and run as tests too.

**Try it.** Write a `Matrix` struct with a `Display` impl. Give it `fn rows(&self) -> impl Iterator<Item = &[f64]>`, a test, and a module of its own.

## Part 5: Advanced

### 20. Lifetimes

A lifetime is the span of code in which a reference is valid. The compiler tracks them for every reference, and most of the time it works them out for you. Without this tracking, this would be a pointer to freed memory:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0597
let r;
{
    let s = String::from("hi");
    r = &s;
}
println!("{r}");
// error[E0597]: `s` does not live
// long enough
```

</td>
<td valign="top">

```rust
let s = String::from("hi");
let r;
{
    r = &s;
}
println!("{r}"); // → hi
```

</td>
</tr>
</table>

You have to write lifetimes when a function returns a reference and the compiler can't tell which input it came from:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0106
fn longest(a: &str, b: &str) -> &str {
    if a.len() > b.len() { a } else { b }
}
// error[E0106]: missing lifetime
// specifier
```

</td>
<td valign="top">

```rust
fn longest<'a>(
    a: &'a str,
    b: &'a str,
) -> &'a str {
    if a.len() > b.len() { a } else { b }
}
println!("{}", longest("ab", "abc"));
// → abc
```

</td>
</tr>
</table>

`'a` changes nothing at run time. It only states a relationship: the result is valid for as long as both inputs are. The caller can then be checked against that.

**Elision.** You rarely write them, because the compiler fills in the common cases. Each reference parameter gets its own lifetime; if there is exactly one input lifetime, the output takes it; and in a method, the output takes the lifetime of `&self`. That is why `first_word(s: &str) -> &str` in chapter 10 needs no annotation.

A struct that holds a reference needs a lifetime, and can't outlive what it borrows. A lexer that borrows its source can return tokens without copying any text:

```rust
struct Words<'a> {
    rest: &'a str,
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a str;
    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(' ').unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        self.rest = tail;
        Some(word)
    }
}

let text = String::from("no copies here");
let n = Words { rest: &text }.map(|w| w.len()).sum::<usize>();
println!("{n}"); // → 12
```

`'static` means valid for the whole program. String literals are `&'static str`, because they're stored in the binary.

When lifetimes get in the way, it's usually a sign that a value should be owned. Returning a `String` instead of a `&str`, or calling `.clone()`, is a fine answer, especially while learning.

### 21. Smart pointers

| Type | What it does | Use it for |
|---|---|---|
| `Box<T>` | owns a value on the heap | recursive types, `dyn Trait` |
| `Rc<T>` | shared ownership, counted, one thread | graphs, shared read-only data |
| `Arc<T>` | as `Rc`, safe across threads | shared data between threads |
| `RefCell<T>` | borrow rules checked at run time | changing what an `Rc` holds |
| `Mutex<T>` | as `RefCell`, safe across threads | changing what an `Arc` holds |
| `Cow<'a, T>` | borrowed, cloned only if changed | often no copy is needed |

**`Box`.** A type can't contain itself directly, because its size would be endless. A `Box` is one pointer, so its size is known. This is how a syntax tree works, and `Expr::Binary` in [src/ast.rs](src/ast.rs) uses the same `Box<Expr>`.

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0072
enum Expr {
    Num(i64),
    Add(Expr, Expr),
}
// error[E0072]: recursive type `Expr`
// has infinite size
```

</td>
<td valign="top">

```rust
enum Expr {
    Num(i64),
    Add(Box<Expr>, Box<Expr>),
}

fn eval(e: &Expr) -> i64 {
    match e {
        Expr::Num(n) => *n,
        Expr::Add(a, b) => eval(a) + eval(b),
    }
}

let one = || Box::new(Expr::Num(1));
let two = Expr::Add(one(), one());
println!("{}", eval(&two)); // → 2
```

</td>
</tr>
</table>

**`Rc`, `Arc` and interior mutability.** This is the model Quillo gives you for free: several owners of one value. `Rc::clone` adds an owner without copying the data. `RefCell` moves the borrow rules to run time, so a second writer panics instead of failing to compile.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let a = [1, 2, 3]
let b = a    // shares the same list
print(b)     // → [1, 2, 3]
```

</td>
<td valign="top">

```rust
use std::cell::RefCell;
use std::rc::Rc;

let a = Rc::new(RefCell::new(vec![1, 2, 3]));
let b = Rc::clone(&a);
b.borrow_mut().push(4);
println!("{:?}", a.borrow());
// → [1, 2, 3, 4]
println!("{}", Rc::strong_count(&a)); // → 2
```

</td>
</tr>
</table>

Quillo's list join uses this to save work. `Arc::try_unwrap` in `binop` gets the `Vec` out without copying if nothing else owns it, and otherwise makes a copy:

```rust
use std::sync::Arc;

fn join(x: Arc<Vec<i32>>, y: &[i32]) -> Vec<i32> {
    let mut v = match Arc::try_unwrap(x) {
        Ok(v) => v,                 // the only owner: reuse it
        Err(x) => x.as_ref().clone(), // shared: copy it
    };
    v.extend_from_slice(y);
    v
}
println!("{:?}", join(Arc::new(vec![1]), &[2])); // → [1, 2]
```

**`Cow`** ("clone on write") returns a borrow when nothing changed and an owned value when something did:

```rust
use std::borrow::Cow;

fn escape(s: &str) -> Cow<'_, str> {
    if s.contains('<') {
        Cow::Owned(s.replace('<', "&lt;"))
    } else {
        Cow::Borrowed(s)
    }
}
println!("{}", escape("plain"));   // → plain
println!("{}", escape("a<b"));     // → a&lt;b
```

**`Drop`.** Freeing is not just for memory. A type can run code when its owner goes out of scope, and this is how Rust closes files and releases locks. Quillo has no equivalent.

```rust
struct Guard(&'static str);

impl Drop for Guard {
    fn drop(&mut self) {
        println!("drop {}", self.0);
    }
}

let _a = Guard("a");
let _b = Guard("b");
println!("end");
// → end
// → drop b
// → drop a
```

Values are dropped in reverse order of creation. `drop(x)` drops one early.

### 22. Threads and shared state

In Quillo, code runs one piece at a time, so `parallel` shortens waits but not computing. Rust threads run at the same moment on different cores.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let [a, b] = parallel(() => 1 + 1, () => 2 + 2)
print(a + b)  // → 6
```

</td>
<td valign="top">

```rust
use std::thread;

let x = thread::spawn(|| 1 + 1);
let y = thread::spawn(|| 2 + 2);
let a = x.join().unwrap();
let b = y.join().unwrap();
println!("{}", a + b); // → 6
```

</td>
</tr>
</table>

A spawned thread may outlive the function that made it, so it can't borrow local variables. `move` gives it ownership:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0373
use std::thread;
let name = String::from("ada");
let t = thread::spawn(|| {
    println!("{name}");
});
t.join().unwrap();
// error[E0373]: closure may outlive
// the current function
```

</td>
<td valign="top">

```rust
use std::thread;
let name = String::from("ada");
let t = thread::spawn(move || {
    println!("{name}"); // → ada
});
t.join().unwrap();
```

</td>
</tr>
</table>

`thread::scope` promises that the threads end before the block does, so they can borrow:

```rust
use std::thread;

let data = vec![1, 2, 3, 4];
let (left, right) = data.split_at(2);
let total = thread::scope(|s| {
    let a = s.spawn(|| left.iter().sum::<i32>());
    let b = s.spawn(|| right.iter().sum::<i32>());
    a.join().unwrap() + b.join().unwrap()
});
println!("{total}"); // → 10
```

There are two ways for threads to cooperate:

<table>
<tr><th>Shared state: Arc and Mutex</th><th>Message passing: a channel</th></tr>
<tr>
<td valign="top">

```rust
use std::sync::{Arc, Mutex};
use std::thread;

let count = Arc::new(Mutex::new(0));
let mut handles = Vec::new();
for _ in 0..4 {
    let count = Arc::clone(&count);
    handles.push(thread::spawn(move || {
        *count.lock().unwrap() += 1;
    }));
}
for h in handles {
    h.join().unwrap();
}
println!("{}", count.lock().unwrap());
// → 4
```

</td>
<td valign="top">

```rust
use std::sync::mpsc;
use std::thread;

let (tx, rx) = mpsc::channel();
for id in 0..4 {
    let tx = tx.clone();
    thread::spawn(move || {
        tx.send(id * 10).unwrap();
    });
}
drop(tx); // ends the loop when all are done
let mut got: Vec<i32> = rx.iter().collect();
got.sort();
println!("{got:?}"); // → [0, 10, 20, 30]
```

</td>
</tr>
</table>

The lock is released when the guard from `lock()` is dropped, so you can't forget. `.lock()` returns a `Result` because a thread that panics while holding a lock "poisons" it. Quillo's core ignores that with `unwrap_or_else(std::sync::PoisonError::into_inner)`, which you'll find throughout [src/interp.rs](src/interp.rs).

Two marker traits make this safe. `Send` means a value can move to another thread, and `Sync` means it can be shared between threads. The compiler works them out for each type, and data races become compile errors:

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0277
use std::rc::Rc;
use std::thread;
let n = Rc::new(1);
thread::spawn(move || {
    println!("{n}");
});
// error[E0277]: `Rc<i32>` cannot be
// sent between threads safely
```

</td>
<td valign="top">

```rust
use std::sync::Arc;
use std::thread;
let n = Arc::new(1);
let t = thread::spawn(move || {
    println!("{n}"); // → 1
});
t.join().unwrap();
```

</td>
</tr>
</table>

### 23. Async

Threads are expensive to keep waiting. Async lets one thread juggle thousands of tasks that spend their time on I/O. In Quillo you never see this: a call just waits, and other requests run meanwhile. In Rust the waiting is written out with `async` and `.await`.

`std` includes the language feature but no runtime to drive it, so programs add one, usually [Tokio](https://tokio.rs): `cargo add tokio --features full`.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
fn load(name, ms) {
    sleep(ms)
    `${name} loaded`
}

let [a, b] = parallel(
    () => load("user", 20),
    () => load("orders", 10),
)
print(`${a}, ${b}`)
// → user loaded, orders loaded
```

</td>
<td valign="top">

```rust tokio
use std::time::Duration;
use tokio::time::sleep;

async fn load(name: &str, ms: u64) -> String {
    sleep(Duration::from_millis(ms)).await;
    format!("{name} loaded")
}

#[tokio::main]
async fn main() {
    let (a, b) = tokio::join!(
        load("user", 20),
        load("orders", 10),
    );
    println!("{a}, {b}");
    // → user loaded, orders loaded
}
```

</td>
</tr>
</table>

- An `async fn` returns a `Future`, and **a future does nothing until awaited**. Calling `load(...)` alone runs none of it.
- `.await` pauses this task and lets others run until the result is ready.
- `tokio::join!` waits for several at once. `tokio::spawn` starts a task that runs by itself, like `thread::spawn`.
- **Don't block.** A slow computation or `std::thread::sleep` in async code stops every task on that thread. Use `tokio::task::spawn_blocking` for that work.

Use threads for computing and async for many slow connections. Quillo uses Tokio in a few places, such as its PostgreSQL client in [src/pg.rs](src/pg.rs), while its HTTP server in [src/httpd.rs](src/httpd.rs) runs requests as coroutines.

### 24. Advanced traits

**Operators are traits.** `a + b` calls `Add::add(a, b)`, so your own types can use `+`. Quillo's `+` is fixed in `binop`, and you can't extend it.

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let a = { x: 1 }
let b = { y: 2 }
print(a + b)  // → { x: 1, y: 2 }
```

</td>
<td valign="top">

```rust
use std::ops::Add;

#[derive(Debug, Clone, Copy)]
struct Point { x: i32, y: i32 }

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point { x: self.x + o.x, y: self.y + o.y }
    }
}

let a = Point { x: 1, y: 2 };
let p = a + Point { x: 3, y: 4 };
println!("{p:?}");
// → Point { x: 4, y: 6 }
```

</td>
</tr>
</table>

This answers the question from chapter 4. The standard library has `impl Add<&str> for String`, which takes the left `String`, appends to it and returns it, reusing its buffer. There's no `impl Add<&str> for &str`, because that would have to allocate.

**Associated types.** `Add` has `type Output`, and `Iterator` has `type Item`. An associated type is a type chosen by each implementation, used when one implementation per type makes sense. Generic parameters are for when there could be several, as `From<A>` and `From<B>` can both exist for one type.

**Blanket implementations** give a trait to every type that has another trait. This is how every `Display` type gets `.to_string()`:

```rust
use std::fmt::Display;

trait Shout {
    fn shout(&self) -> String;
}

impl<T: Display> Shout for T {
    fn shout(&self) -> String {
        format!("{}!", self.to_string().to_uppercase())
    }
}

println!("{}", "hi".shout()); // → HI!
println!("{}", 42.shout());   // → 42!
```

**Const generics** make a value part of a type:

```rust
fn sum_all<const N: usize>(xs: [i32; N]) -> i32 {
    xs.iter().sum::<i32>() * N as i32
}
println!("{}", sum_all([1, 2, 3])); // → 18
```

**Typestate.** Encode a state in a type parameter, and using something in the wrong state stops being possible. Here `read` exists only on a published post. `PhantomData` marks a type parameter that no field holds, and it takes no memory.

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0599
use std::marker::PhantomData;
struct Draft;
struct Live;
struct Post<S> {
    text: String,
    state: PhantomData<S>,
}
impl Post<Draft> {
    fn new(text: &str) -> Self {
        Post {
            text: text.into(),
            state: PhantomData,
        }
    }
}
impl Post<Live> {
    fn read(&self) -> &str { &self.text }
}
let p = Post::new("hi");
println!("{}", p.read());
// error[E0599]: no method named
// `read` found for `Post<Draft>`
```

</td>
<td valign="top">

```rust
use std::marker::PhantomData;
struct Draft;
struct Live;
struct Post<S> {
    text: String,
    state: PhantomData<S>,
}
impl Post<Draft> {
    fn new(text: &str) -> Self {
        Post {
            text: text.into(),
            state: PhantomData,
        }
    }
    fn publish(self) -> Post<Live> {
        Post { text: self.text, state: PhantomData }
    }
}
impl Post<Live> {
    fn read(&self) -> &str { &self.text }
}
let p = Post::new("hi").publish();
println!("{}", p.read()); // → hi
```

</td>
</tr>
</table>

### 25. Macros

A macro writes code at compile time. Quillo has no macros; its map literal is built into the language. In Rust you can add syntax like that yourself with `macro_rules!`, which matches on the shape of what you write:

<table>
<tr><th>Quillo</th><th>Rust</th></tr>
<tr>
<td valign="top">

```quillo
let m = { a: 1, b: 2 }
print(m)  // → { a: 1, b: 2 }
```

</td>
<td valign="top">

```rust
use std::collections::BTreeMap;

macro_rules! map {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = BTreeMap::new();
        $( m.insert($k, $v); )*
        m
    }};
}

let m = map! { "a" => 1, "b" => 2 };
println!("{m:?}"); // → {"a": 1, "b": 2}
```

</td>
</tr>
</table>

- `$k:expr` captures one expression. Other kinds include `ident`, `ty` and `tt`.
- `$( ... ),*` repeats, once for each comma-separated item.
- `$(,)?` allows a trailing comma.
- Macros are hygienic: the `m` inside doesn't clash with an `m` at the place of use.

Why have `println!` and `vec!` as macros at all? A function can't take a variable number of arguments, and `println!` checks its format string at compile time.

**Procedural macros** are Rust programs that transform code. `#[derive(Debug)]` is one, and so are `#[tokio::main]` and serde's `#[derive(Serialize)]`. Use libraries of them freely, and write your own only when `macro_rules!` and generics can't do the job, since they're harder to write and slower to compile.

### 26. Unsafe and calling C

The compiler can't prove everything. `unsafe` lets you do five extra things, of which the common two are following a raw pointer and calling an `unsafe` function. It doesn't turn off the borrow checker. It marks the place where **you** promise what the compiler can't check.

The classic case: two slices of one array that don't overlap. It's correct, but the borrow checker sees only two `&mut` borrows of `xs`.

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0499
fn split_mut(
    xs: &mut [i32],
    mid: usize,
) -> (&mut [i32], &mut [i32]) {
    (&mut xs[..mid], &mut xs[mid..])
}
// error[E0499]: cannot borrow `*xs`
// as mutable more than once
```

</td>
<td valign="top">

```rust
use std::slice::from_raw_parts_mut as parts;

fn split_mut(
    xs: &mut [i32],
    mid: usize,
) -> (&mut [i32], &mut [i32]) {
    let len = xs.len();
    assert!(mid <= len);
    let p = xs.as_mut_ptr();
    // SAFETY: mid is in range, and the two
    // parts do not overlap.
    unsafe {
        (parts(p, mid), parts(p.add(mid), len - mid))
    }
}

let mut v = [1, 2, 3, 4];
let (a, b) = split_mut(&mut v, 1);
a[0] = 9;
b[0] = 8;
println!("{v:?}"); // → [9, 8, 3, 4]
```

</td>
</tr>
</table>

The important part is the shape: a small `unsafe` block, a comment saying why it's sound, checks that come first, and a safe function around it. Callers can't misuse it. The standard library's `split_at_mut` does exactly this, and `Vec` is built the same way.

**Calling C.** Declare the function in an `extern` block. The compiler can't see inside it, so each call is `unsafe`. Quillo calls the C maths library like this, in `mod c` in [src/numeric.rs](src/numeric.rs).

<table>
<tr><th>Doesn't compile</th><th>Compiles</th></tr>
<tr>
<td valign="top">

```rust compile_fail=E0133
unsafe extern "C" {
    fn abs(x: i32) -> i32;
}
println!("{}", abs(-3));
// error[E0133]: call to unsafe
// function is unsafe
```

</td>
<td valign="top">

```rust
unsafe extern "C" {
    fn abs(x: i32) -> i32;
}
// SAFETY: abs accepts any int but i32::MIN.
println!("{}", unsafe { abs(-3) }); // → 3
```

</td>
</tr>
</table>

Edition 2021, which Quillo uses, writes `extern "C"` without the leading `unsafe`.

**Try it.** Take `struct Stack<T>` with `push` and `pop`, and wrap it in `Arc<Mutex<...>>`. Have four threads each push 100 items, and check the total. Then write a `macro_rules!` called `stack!`, which builds one from a list of values.

## Part 6: Putting it together

### 27. A tiny Quillo

This program puts the tutorial together: enums, `match`, `Box`, `Result`, `?`, traits, and iteration. It handles ints, strings and `+`, and follows the same stages as the real interpreter.

```rust
use std::fmt;

// ---- Lexer: text to tokens ----

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Int(i64),
    Str(String),
    Plus,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            ' ' => i += 1,
            '+' => {
                toks.push(Tok::Plus);
                i += 1;
            }
            '0'..='9' => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let n = text.parse().map_err(|_| format!("too large: {text}"))?;
                toks.push(Tok::Int(n));
            }
            '"' => {
                let start = i + 1;
                i = start;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
                if i == chars.len() {
                    return Err("a string is not closed".into());
                }
                toks.push(Tok::Str(chars[start..i].iter().collect()));
                i += 1;
            }
            c => return Err(format!("unexpected `{c}`")),
        }
    }
    Ok(toks)
}

// ---- Parser: tokens to a tree ----

#[derive(Debug)]
enum Expr {
    Int(i64),
    Str(String),
    Add(Box<Expr>, Box<Expr>),
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn atom(&mut self) -> Result<Expr, String> {
        let tok = self.toks.get(self.pos).cloned();
        self.pos += 1;
        match tok {
            Some(Tok::Int(n)) => Ok(Expr::Int(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            other => Err(format!("expected a value, got {other:?}")),
        }
    }

    // "a" + "b" + "c" leans left: (("a" + "b") + "c")
    fn add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.atom()?;
        while self.toks.get(self.pos) == Some(&Tok::Plus) {
            self.pos += 1;
            let rhs = self.atom()?;
            lhs = Expr::Add(Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }
}

// ---- Evaluator: a tree to a value ----

#[derive(Debug)]
enum Value {
    Int(i64),
    Str(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Str(s) => write!(f, "{s}"),
        }
    }
}

fn binop_add(a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x
            .checked_add(y)
            .map(Int)
            .ok_or_else(|| "integer overflow".to_string()),
        (Str(x), y) => Ok(Str(format!("{x}{y}"))),
        (x, Str(y)) => Ok(Str(format!("{x}{y}"))),
    }
}

fn eval(e: &Expr) -> Result<Value, String> {
    match e {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Add(a, b) => binop_add(eval(a)?, eval(b)?),
    }
}

// ---- All three ----

fn run(src: &str) -> Result<Value, String> {
    let mut p = Parser { toks: lex(src)?, pos: 0 };
    let tree = p.add()?;
    if p.pos < p.toks.len() {
        return Err("more after the end".into());
    }
    eval(&tree)
}

fn main() {
    let programs = [
        "1 + 1",
        r#""a" + "b" + "c""#,
        r#""n=" + 1"#,
        "9223372036854775807 + 1",
        "1 +",
    ];
    for src in programs {
        match run(src) {
            Ok(v) => println!("{v}"),
            Err(e) => println!("error: {e}"),
        }
    }
    // → 2
    // → abc
    // → n=1
    // → error: integer overflow
    // → error: expected a value, got None
}
```

`r#"..."#` is a raw string, in which `"` needs no escaping. Each part corresponds to one in the real thing:

| Here | In Quillo | What differs |
|---|---|---|
| `Tok`, `lex` | [src/lexer.rs](src/lexer.rs) | line numbers, templates, every token |
| `Expr` | `Expr` in [src/ast.rs](src/ast.rs) | dozens of cases |
| `Parser::add` | `parse_add` in [src/parser.rs](src/parser.rs) | also `-`, and a nesting limit |
| `binop_add` | `binop` in [src/interp.rs](src/interp.rs) | lists, maps, HTML, decimals, floats |
| `eval` | `eval` in [src/interp.rs](src/interp.rs) | variables, calls, and `Ctrl` for `return` |

To extend it, add `-` and `*` with the right precedence, then parentheses, then `let` with a `HashMap` of variables.

### 28. Writing Rust for a Quillo program

Now you can use both languages at once. A `.qwer` file is Rust, and a program `use`s it like a `.qwe` file. Its `pub fn`s become Quillo functions, and a `Result` becomes a value or an error value, so `?` and `??` work on it.

<table>
<tr><th>stats.qwer (Rust)</th><th>main.qwe (Quillo)</th></tr>
<tr>
<td valign="top">

```rust file=stats.qwer
/// The mean of some numbers.
pub fn mean(xs: Vec<f64>) -> Result<f64, String> {
    if xs.is_empty() {
        return Err("no numbers".to_string());
    }
    Ok(xs.iter().sum::<f64>() / xs.len() as f64)
}
```

</td>
<td valign="top">

```quillo
use "stats.qwer"

print(mean([1.0, 2.0, 6.0]))  // → 3.0
print(mean([]) ?? "none")     // → none
```

</td>
</tr>
</table>

The first run builds the file with `cargo`, and later runs load the result. Rust's types cross as you'd expect:

| Rust | Quillo |
|---|---|
| `i64` and smaller integers | `int` |
| `f64`, `f32` | `float` |
| `bool` | `bool` |
| `String`, `&str`, `char` | `str` |
| `Vec<T>`, `&[T]` | list |
| `HashMap<String, T>`, `BTreeMap<String, T>` | map |
| `Option<T>` | the value, or `null` |
| `Result<T, E>` | the value, or an error value |
| `()` | `null` |

Structs and tuples don't cross; take a `serde_json::Value` when you need one. A crate goes in a comment at the top of the file, as in `//! [dependencies]`. See [USERGUIDE.md](USERGUIDE.md#rust) for the rest.

## Cheat sheet

| Quillo | Rust |
|---|---|
| `let x = 1` then `x = 2` | `let mut x = 1; x = 2;` |
| `int`, `float`, `bool` | `i64`, `f64`, `bool` |
| `str` | `String` (owned), `&str` (borrowed) |
| `list<int>` | `Vec<i64>` |
| `{ a: 1 }` as data | `HashMap`, `BTreeMap` |
| `type User = {...}` | `struct User {...}` |
| `"a" \| "b"` | `enum` |
| `null` | `None`, of `Option<T>` |
| `err(x)` | `Err(x)`, of `Result<T, E>` |
| `x?` | `x?` |
| `x ?? d` | `x.unwrap_or(d)` |
| `` `hi ${name}` `` | `format!("hi {name}")` |
| `"a" + "b"` | `format!("{}{}", "a", "b")` |
| `(x) => x * 2` | `\|x\| x * 2` |
| `xs \| map(f)` | `xs.iter().map(f).collect()` |
| `match x { ... }` | `match x { ... }`, every case required |
| `len(s)` | `s.chars().count()` |
| `len(xs)` | `xs.len()` |
| `xs[0]` | `xs.get(0)`, or `xs[0]`, which panics |
| `use "file.qwe"` | `mod file;` |
| `private` | leave off `pub` |
| `parallel(f, g)` | `tokio::join!`, or threads |
| runtime error | `panic!` |

| When the compiler says | It usually means | Try |
|---|---|---|
| E0382 use of moved value | a value has one owner | borrow with `&`, or `.clone()` |
| E0502, E0499 borrow conflict | a reference is still in use | use it first, or copy the value out |
| E0308 mismatched types | no automatic conversion | `.into()`, `as`, `&`, or drop the `;` |
| E0106 missing lifetime | a returned reference has no clear source | add `'a`, or return an owned value |
| E0277 trait not satisfied | the type lacks a capability | add a `#[derive]` or a bound |
| E0373 closure may outlive | a thread is borrowing | add `move` |

## Where to go next

- [The Rust Programming Language](https://doc.rust-lang.org/book/) is the official book, and the fullest treatment of everything here.
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/) is shorter, with runnable examples.
- [Rustlings](https://github.com/rust-lang/rustlings) has small exercises to fix.
- [The standard library docs](https://doc.rust-lang.org/std/) are worth reading, especially `Option`, `Result`, `Vec` and `Iterator`.
- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) covers `unsafe` in depth.
- [Asynchronous Programming in Rust](https://rust-lang.github.io/async-book/) covers async.
- Quillo's own [src/interp.rs](src/interp.rs) is a good next read: you now know every feature its `binop` uses.
