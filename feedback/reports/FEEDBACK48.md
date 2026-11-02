# Feedback from probing markup written the JSX way: React's event names, values in attributes, entities, spreads, and the parser's messages

| | |
|---|---|
| App | None: about 60 probe programs of 1 to 15 lines, each written the way a React or JSX developer would write it, plus six pages run in headless Chrome. Covered: event names and the controlled input, `true`/`false`/numbers in attributes and children, `style` and `class` given a map or a list, `{...spread}` on elements and on views, entities, attribute strings, `<pre>` and whitespace, fragments, member tags (`<Card.Header/>`), dynamic tags, props and `children`, render props, `.map` with an index, comments inside tags, void tags closed XHTML-style |
| Quillo | 0.6.0, release build of the runtime-coverage branch's working tree (it reports `54dd1bd`; the binary was built at 07:20 on 2026-11-03, after the last source change). The branch's uncommitted work touches `parser.rs`, `check.rs`, `interp.rs`, `client.rs` and `runtime.js`; every finding below was seen on this build, and none was checked on main |
| Model | Claude (Opus 5.5) |
| Tested with | `quillo check` and `quillo run` on each probe (`print(<markup>)` shows the server's HTML); for components, `quillo run --port <free port>`, `curl` and headless Chrome driven by [tests/browser/run.js](../../tests/browser/run.js), typing by `document.execCommand("insertText")`. Read: [src/parser.rs](../../src/parser.rs) (`parse_html_el`, `parse_html_children`, `jsx_text`, `decode_entities`, `read_interp_name`), [src/check.rs](../../src/check.rs) (`attr_typo`), [src/interp.rs](../../src/interp.rs) (`style_text`, `write_attr`), [src/client/runtime.js](../../src/client/runtime.js) (the event listener, around line 5455), USERGUIDE.md (HTML literals, From HTML, Components, Coming from JavaScript, Coming from other frameworks) |

No Quillo source was changed, and `feedback/INDEX.md` and `feedback/repros/` were not touched. Each repro is in its issue's section, and the browser probes are at the end. FEEDBACK47.md (routes and groups) was being written at the same time, from Q-534, and had reached Q-546 when this report was finished: numbers here start at Q-560, to leave it room.

The guide says markup "is written like JSX". Most of it is: text, `{expr}`, ternaries and `&&`, `(...)` around markup, self-closing tags, `onClick={f}` as `@click`, and `className`/`htmlFor` are warned about. Where it is not like JSX, it is mostly silent. `quillo check` says ok, and the page does something else. The worst cases are React's controlled input, which only updates on blur (Q-560), React event names that never fire (Q-561), and `&amp;` in a link, which breaks the link (Q-562). Fourteen of the nineteen issues are silent.

## Summary

| ID | Problem | Kind |
|---|---|---|
| Q-560 | `<input value={x} onChange={...}>`, React's controlled input, updates only when the input loses focus | silent |
| Q-561 | `onDoubleClick`, `onClickCapture`, `@mouseEnter`, and `onFocus` on a wrapper never fire | silent |
| Q-562 | Only ten named entities are decoded in text, and none in attribute strings: `href="?a=1&amp;b=2"` breaks the link | silent |
| Q-563 | Attribute strings take backslash escapes: `title="C:\new"` gets a line break, and `pattern="\d{3}"` does not load | silent / loud |
| Q-564 | `{true}` shows "true", `aria-hidden={true}` does not hide, and `aria-expanded={false}`, `spellcheck={false}` and `data-x={false}` are left out | silent |
| Q-565 | A number in a `style` map gets no unit: `fontSize: 12` is `font-size: 12`, which the browser drops | silent |
| Q-566 | `class={{ active: on }}` and `class={[...]}` are written as Quillo text, and get scrambled after an update | silent |
| Q-567 | A spread map's `onClick` is dropped, and its `className` and `false` values get none of the written-out rules | silent |
| Q-568 | `.class` and `#id` on a view or component tag are dropped; `<Card.Header/>` renders `<Card>` | silent |
| Q-569 | Children given to a view or component without a `children` parameter are dropped | silent |
| Q-570 | React-only attributes reach the page and do nothing: `defaultValue`, `ref`, `dangerouslySetInnerHTML`, SVG's `strokeWidth` | silent |
| Q-571 | A function as a child shows `<fn <fn>>` | silent |
| Q-572 | `<pre>` and `<textarea>` text loses its line breaks and indentation, and a `//` line in it is dropped | silent |
| Q-573 | Text and an element on separate lines are joined with no space: "Hello,<b>Ann</b>welcome back" | silent |
| Q-574 | `xs.map((x, i) => ...)` passes `check` and fails when it runs | loud |
| Q-575 | JSX's comments in a tag and `{// ...}` are refused with "expected an expression, found `}`" | loud |
| Q-576 | `<img></img>`: "`</img>` does not close `<div>`", and a quick fix that breaks the markup | loud |
| Q-577 | React's forms get messages that do not say how Quillo writes them: `<React.Fragment>`, `<Tag>`, `props`, `({ title })`, `{...xs}` | loud |
| Q-578 | The guide has a "From HTML" table but no "From JSX" one | docs |

## New issues

### Q-560. `<input value={x} onChange={...}>`, React's controlled input, updates only when the input loses focus

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

`onChange={f}` on an element becomes `@change` (`parser.rs:3057`), which is the DOM's `change` event. For a text field, `change` fires when the field loses focus. React's `onChange` fires on every keystroke (the DOM's `input`). This is how most React code reads an input:

```quillo
component C() {
    state name = ""
    <div>
        <input #a value={name} onChange={(e) => { name = e.target.value }}/>
        <p #pa>{name}</p>
    </div>
}
route GET / = <html><body><C/></body></html>
```

`check`: ok. In Chrome, after typing "ann": the input shows `ann` and the `<p>` is empty. When focus moves to another input, both show `ann`. A transform shows the same lag: with `upper = upper(e.target.value)`, the field shows `bob` while typing and `BOB` after a blur.

It is worse than leaving the handler off. Without the handler, `value={name}` binds both ways by itself and updates on every keystroke. The handler counts as "a handler of its own on an event that says it changed", so the input is now one-way.

**Suggestion:** On `<input>` (not a checkbox, radio or file input) and `<textarea>`, either make `onChange={...}` mean `@input`, as React does, or warn: `` `onChange` here is the DOM's `change`, which fires when the field loses focus: for each keystroke write `@input` (or leave the handler off: `value={name}` follows typing by itself) ``. Keep `@change` exactly as written, since that is the DOM's name used on purpose. Warning is the smaller change. Making it `@input` matches what the code expects.

### Q-561. `onDoubleClick`, `onClickCapture`, `@mouseEnter`, and `onFocus` on a wrapper never fire

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 1 |

The `on…` rename lowercases the rest of the name, and the runtime listens for that event name as given (`runtime.js:5463`). Several React names are not DOM names:

```quillo
component C() {
    state log = []
    <div>
        <div #me @mouseEnter={() => { log = push(log, "mouseEnter") }}
                 onDoubleClick={() => { log = push(log, "dbl") }}
                 onClickCapture={() => { log = push(log, "cap") }}
                 @mouseenter={() => { log = push(log, "mouseenter") }}>hover</div>
        <div #w onFocus={() => { log = push(log, "focus") }}><input #i/></div>
        <p #log>{join(log, ",")}</p>
    </div>
}
```

`check`: ok, with no warnings. The script sends `mouseenter`, a bubbling `dblclick` and a click, then focuses `#i` and moves focus away. The log shows `mouseenter` only:

- `onDoubleClick` listens for `doubleclick`. The DOM's name is `dblclick`.
- `onClickCapture`, and any `on…Capture`, listens for `clickcapture`. React reads the suffix as "capture phase".
- `@mouseEnter`, `@keyDown`: DOM event names are case-sensitive. `attr_typo` compares them to its list ignoring case (`check.rs:4249`), so they pass.
- `onFocus`/`onBlur` on a wrapper: React's bubble (they are `focusin`/`focusout`). Here they are the DOM's `focus`/`blur`, which do not bubble, so a wrapper around inputs never hears them.

**Suggestion:** Map the React names that differ: `onDoubleClick` to `dblclick`, and `on<X>Capture` to `<x>` with `{ capture: true }` (or refuse it and say so). For `@` names, compare against the event list case-sensitively and warn about a capital (`` `@mouseEnter` never fires: DOM event names are lower case, `@mouseenter` ``). For `onFocus`/`onBlur` on an element that cannot take focus itself (not an input, button, link or `tabindex` element), warn and suggest `@focusin`/`@focusout`.

### Q-562. Only ten named entities are decoded in text, and none in attribute strings: `href="?a=1&amp;b=2"` breaks the link

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, browser | 1 |

`decode_entities` (`parser.rs:3528`) knows `amp lt gt quot apos nbsp copy mdash ndash hellip` and numeric references. Any other name stays as text, and is escaped, so it shows on the page as written. Attribute strings are not decoded at all. JSX decodes all of HTML's named entities in both places. The guide's "From HTML" section says that entities stay as they are.

```quillo
print(<a href="/s?a=1&amp;b=2" title="Tom &amp; Jerry">x</a>)
print(<p>&rarr; &times; &euro; &lsquo;q&rsquo; &bull; &deg; &trade; &frac12;</p>)
```

`check`: ok. Output:

```html
<a href="/s?a=1&amp;amp;b=2" title="Tom &amp;amp; Jerry">x</a>
<p>&amp;rarr; &amp;times; &amp;euro; &amp;lsquo;q&amp;rsquo; &amp;bull; &amp;deg; &amp;trade; &amp;frac12;</p>
```

The link goes to `/s?a=1&amp;b=2`, so the server gets a parameter named `amp;b`. The tooltip says `Tom &amp; Jerry`. In the browser `#ent`'s text is `&rarr; … &times;` and its title `&copy; &hellip;`, the same as the server's, so nothing is remade and nothing is reported.

HTML pasted from an editor or a CMS is full of these (`&amp;` in every link with two parameters, `&rsquo;` in text, `&times;` on close buttons).

**Suggestion:** Decode HTML's full named-entity table (the WHATWG list) in text and in quoted and unquoted attribute strings. This happens at parse time on the server, so the runtime does not grow. If the table is too large, `quillo check` could at least report a name it does not know: `` `&rarr;` is not an entity Quillo knows: write the character, →, or `&#8594;` ``.

### Q-563. Attribute strings take backslash escapes: `title="C:\new"` gets a line break, and `pattern="\d{3}"` does not load

| Kind | Detected by | Attempts |
|---|---|---|
| silent / loud | `quillo run` | 2 |

An attribute's `"..."` is read by the lexer's `lex_string` (`parser.rs:2986`), the same as a Quillo string, so `\n`, `\t` and `\"` are escapes. In JSX, and in HTML, an attribute string is plain text, and a backslash is a backslash.

```quillo
print(<p title="C:\new\table">x</p>)
print(<input pattern="\d{3}"/>)
```

The first passes `check` and prints `title="C:` followed by a line break, `ew`, a tab and `able`. The second does not load: `error: unknown escape \d in string`, followed by the note `if the code was written for a newer Quillo, that would explain it`, which does not apply here. `pattern` holds a regular expression, so a backslash in it is common (`\d`, `\w`, `\.`).

The guide's "HTML literals" says attributes take `"text"` "(plain, as in JSX)".

**Suggestion:** Read an attribute's `"..."` as plain text, as JSX does, ending at the next `"`, and decode entities in it (Q-562). `&quot;` then writes a quote. A `\"` written today would then end the string early, so `quillo check` should warn about a `\` in an attribute string for one release. As a smaller step, make the error say what to do: `` `\d` in an attribute: attribute text takes `\\` for a backslash, or write the value as {"\\d{3}"} ``. Leave out the "newer Quillo" note for this error.

### Q-564. `{true}` shows "true", `aria-hidden={true}` does not hide, and `aria-expanded={false}`, `spellcheck={false}` and `data-x={false}` are left out

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, browser | 1 |

As a child, `true` is shown and `false` is not. React shows neither. In an attribute, `true` writes the bare name and `false` leaves the attribute out (`write_attr`, `interp.rs:4345`). That is right for HTML's boolean attributes (`disabled`, `hidden`). It is wrong for the attributes whose value is the text `"true"`/`"false"`, which React writes out as text:

```quillo
let admin = true
print(<p>{admin} {!admin}</p>)
print(<div aria-hidden={true} aria-expanded={false} data-on={false} draggable={true} spellcheck={false}>x</div>)
```

`check`: ok. Output: `<p>true </p>` and `<div aria-hidden draggable>x</div>`. In Chrome, the same comes out of the browser's own rendering after a click:

- `aria-hidden=""`: ARIA reads an empty value as not set, so the element is not hidden from screen readers.
- `aria-expanded` is left out, so a collapsed menu is announced as having no state, not as collapsed.
- `draggable=""` is not one of the attribute's values, so it is `auto`: `el.draggable` is `false`.
- `spellcheck` is left out, so `el.spellcheck` stays `true`.
- `data-on` is left out, so `el.dataset.on` is `undefined`, not `"false"`.

`{flag}` showing `true` and nothing for `false` is not symmetric. A JSX habit such as `{isAdmin && " (admin)"}` written as `{isAdmin}` shows "true" on the page.

**Suggestion:** For `aria-*`, `data-*` and the enumerated attributes (`draggable`, `spellcheck`, `contenteditable`, `translate` as yes/no, `autocapitalize`), write `true`/`false` as the text `"true"`/`"false"`, as React does. Keep the bare/absent form for HTML's boolean attributes. For a `bool` child, either show nothing, as React does, or have `quillo check` warn: `` a bool shows as "true" and false as nothing: write {admin ? "yes" : "no"} ``. Showing nothing changes what existing programs print, so this one is yours to decide.

### Q-565. A number in a `style` map gets no unit: `fontSize: 12` is `font-size: 12`, which the browser drops

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, browser | 1 |

`style_text` (`interp.rs:714`) writes a value with `display()`. React adds `px` to a number for every property not on its list of unitless ones (`opacity`, `zIndex`, `lineHeight`, `flex`, `fontWeight`, ...).

```quillo
print(<div style={{ fontSize: 12, width: 100, opacity: 0.5, zIndex: 3, msFlex: "1" }}>x</div>)
```

`check`: ok. Output: `style="font-size: 12; width: 100; opacity: 0.5; z-index: 3; ms-flex: 1"`. In Chrome, `getComputedStyle(el).fontSize` is `16px`, so the declaration was dropped. `width` was dropped the same way. Q-429's fix brought the map form in "React's way", so React's numbers come with it.

`msFlex` becomes `ms-flex`. React gives `-ms-flex`, as it does for `Webkit…` and `Moz…`, which Quillo already gets right.

**Suggestion:** Add `px` to a non-zero number for properties not on React's unitless list, so `0` stays `0`. Otherwise, have `quillo check` warn about a number on a property that needs a length: `` `fontSize: 12` is `font-size: 12`, which the browser drops: write "12px" ``. Give `ms` a leading `-` as well.

### Q-566. `class={{ active: on }}` and `class={[...]}` are written as Quillo text, and get scrambled after an update

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, browser | 1 |

The object and array forms that `clsx`, `classnames`, Vue and Svelte all take are not read as classes:

```quillo
component C() {
    state on = true
    <div>
        <p #m class={{ active: on, off: !on }}>a</p>
        <p #l class={["a", on && "b"]}>b</p>
        <button #go @click={() => { on = !on }}>t</button>
    </div>
}
```

`check`: ok. The server writes `class="{ active: true, off: false }"` and `class="[&quot;a&quot;, &quot;b&quot;]"`, so the element has the classes `{`, `active:`, `true,` and so on. After the click, the browser's class update reads the old and new text as lists of words and changes only the words that differ. `#m`'s class becomes `{ active: off: } false, true` and `#l`'s becomes `["a", false]`.

Quillo's own form is `class:active={on}`, and `class={on && "active"}` works too. Neither is reached for by a developer used to `clsx`.

**Suggestion:** Take a map as "the keys whose values are truthy" and a list as "the items that are strings, joined", on the server and in the browser. Or make it an error in `quillo check` (`` `class` given a map: write class:active={on}, or a string ``). The first is what all four libraries do.

### Q-567. A spread map's `onClick` is dropped, and its `className` and `false` values get none of the written-out rules

| Kind | Detected by | Attempts |
|---|---|---|
| silent | browser | 2 |

`<button {...props}>` is how React passes props through to an element. A spread follows different rules from the same keys written out:

```quillo
component C() {
    state n = 0
    let props = { onClick: () => { n += 1 }, title: "t", className: "big", "aria-pressed": false, style: { fontSize: 20 } }
    let at = { "@click": () => { n += 1 } }
    <div><button #b {...props}>{n}</button><button #c {...at}>{n}</button></div>
}
```

`check`: ok. The server writes `<button id="b" title="t" className="big" style="font-size: 20">0</button>`. In Chrome, clicking `#b` leaves it at `0`, and its attributes are `id`, `title`, `classname` and `style`. The same keys written out behave differently:

| Key | Written out | In a spread |
|---|---|---|
| `onClick: f` | a handler, as `@click` | dropped (`onclick: f` too) |
| `"@click": f` | a handler | a handler (`#c` counts) |
| `className` | warned about by `check` | written as `classname="big"` |
| `"aria-pressed": false` | left out (Q-564) | left out |
| `style: { fontSize: 20 }` | Q-565 | Q-565 |

The guide says spread "may hold a handler", without saying that it must be written `"@click"`.

**Suggestion:** When spreading onto an HTML element, apply the `on…` rename to a key whose value is a function, as the parser does for a written-out attribute. Treat `className` as `class` (React's code is the reason anyone spreads it), or warn about it at run time in development.

### Q-568. `.class` and `#id` on a view or component tag are dropped; `<Card.Header/>` renders `<Card>`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | 1 |

The shorthand is read after any tag name, including a capitalised one, and then ignored for views and components. `class="x"` on the same tag is an error (`<Card> has no parameter class`). That makes the shorthand an unchecked way to write the same thing. It also turns JSX's member tags into something else without a word:

```quillo
view Card() = <div.card>card</div>
component Tab(label) { <button.tab>{label}</button> }
print(<Card.big #main/>)
print(<Card.Header/>)
view V() = <div><Tab.active label="a"/><Tab #first label="b"/></div>
```

`check`: ok. Both `print`s write `<div class="card">card</div>`, and `V` writes two `<button class="tab">`, without `active` and without the id. `<Card.Header/>`, which in JSX is the `Header` that `Card` exports, renders `Card` itself.

**Suggestion:** Make `.x` and `#x` on a capitalised tag an error that says what to do. When the name is capitalised (`.Header`), say `` there are no member tags: <Card.Header> is <Card> with the class `Header`; import the view by its own name ``. For lower case, say `` `.active` on <Tab> goes nowhere: a view's attributes are its parameters; give it a `class` parameter ``. Or pass the shorthand to a view's declared `class`/`id` parameter.

### Q-569. Children given to a view or component without a `children` parameter are dropped

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | 1 |

```quillo
view Card(title) = <div>{title}</div>
print(<Card title="t">body text</Card>)
```

`check`: ok. Output: `<div>t</div>`. The body is lost. An attribute the view lacks is an error (`<Card> has no parameter extra`), but a body the view lacks is not. React does the same, but React has no checker that claims to check a view's parameters.

**Suggestion:** Give the same error as for an attribute: `` <Card> takes no children: add a `children` parameter, or write <Card title="t"/> ``. Count text that is only whitespace as no children.

### Q-570. React-only attributes reach the page and do nothing: `defaultValue`, `ref`, `dangerouslySetInnerHTML`, SVG's `strokeWidth`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run`, browser | 1 |

`attr_typo` (`check.rs:4193`) warns about `className` and `htmlFor`, and about names near its list. React's own names that are not HTML's pass:

```quillo
let s = "<b>x</b>"
print(<input defaultValue="hello"/>)
print(<input type="checkbox" defaultChecked/>)
print(<div dangerouslySetInnerHTML={{ __html: s }}></div>)
print(<div ref={s}>x</div>)
print(<svg viewBox="0 0 10 10"><path strokeWidth={2} fillRule="evenodd" strokeLinecap="round" d="M0 0"/></svg>)
```

`check`: ok. The page gets `defaultValue="hello"`, `defaultChecked`, `dangerouslySetInnerHTML="{&quot;__html&quot;:...}"`, `ref="..."` and `strokeWidth="2"`. In Chrome, the input's value is empty and the box is not ticked. A path with `strokeWidth="5"` has a stroke width of `1px`, while one with `stroke-width="5"` has `5px`. The HTML parser lowercases the name to `strokewidth`, which SVG does not know.

The check gives up on any SVG tag (`check.rs:4218`), because SVG has too many attributes to list. React's camelCase versions of SVG's hyphenated attributes (`strokeWidth`, `strokeLinecap`, `strokeDasharray`, `fillRule`, `clipRule`, `fillOpacity`, `stopColor`, `fontSize`, `textAnchor`) are a short, known list.

**Suggestion:** Warn about each by name: `defaultValue` (`` write `value`: an input with no state keeps what is typed ``), `defaultChecked`, `dangerouslySetInnerHTML` (`` write {raw(s)} as the child ``), `ref` (`` write @mount={(el) => ...} ``), and on SVG tags React's camelCase names (`` `strokeWidth` is React's: SVG's attribute is `stroke-width` ``), the same way as `className`.

### Q-571. A function as a child shows `<fn <fn>>`

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | 1 |

```quillo
print(<div>{(x) => <p>{x}</p>}</div>)
```

`check`: ok. Output: `<div>&lt;fn &lt;fn&gt;&gt;</div>`. A render prop passed as an attribute works (`render={(i) => <li>{i}</li>}`, called as `render(i)`). Only a function placed directly where markup goes is shown as text. That is the form React's "function as children" pattern takes, and also the form of a mistake (`{onClick}` where `{label}` was meant).

**Suggestion:** Have `quillo check` warn about a lambda, or a name known to hold a function, as an element's child: `` a function here shows as <fn>: call it, {render(x)}, or pass it as an attribute ``. At run time, show nothing in its place, as with `null`.

### Q-572. `<pre>` and `<textarea>` text loses its line breaks and indentation, and a `//` line in it is dropped

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | 1 |

`jsx_text` (`parser.rs:3508`) trims and joins lines in every element. The `//` comment rule (`parse_html_children`) also applies everywhere outside `<script>` and `<style>`:

```quillo
print(<pre>
fn main() =
    print("hi")
// done
</pre>)
print(<textarea>
line one
  line two
</textarea>)
```

`check`: ok. Output: `<pre>fn main() = print(&quot;hi&quot;)</pre>` and `<textarea>line one line two</textarea>`. The layout is gone, and the `// done` line is dropped as a comment. JSX does the same, which is why React code writes `` <pre>{`...`}</pre> ``. But the guide's "From HTML" section invites pasting HTML. Its table lists the braces in a `<pre>` sample as something to change, but not the whitespace, and not a line starting with `//`, which most code samples have.

**Suggestion:** Inside `<pre>`, `<textarea>` and `<listing>`, keep the text as written: no trimming, no joining, and no `//` comments. Drop only the one line break right after the start tag, as HTML does. Otherwise, add a row to "From HTML": `` a <pre> block | its text as a template string, <pre>{`...`}</pre>: markup text is joined into one line ``, and have `quillo check` warn about a `<pre>` whose text spans lines.

### Q-573. Text and an element on separate lines are joined with no space: "Hello,<b>Ann</b>welcome back"

| Kind | Detected by | Attempts |
|---|---|---|
| silent | `quillo run` | 1 |

```quillo
let name = "Ann"
print(<p>
    Hello,
    <b>{name}</b>
    welcome back
</p>)
```

Output: `<p>Hello,<b>Ann</b>welcome back</p>`, which reads "Hello,Annwelcome back". This is JSX's rule too, and it catches React developers as well: they add `{" "}`. Quillo follows JSX on purpose, so this is not a bug. But `quillo check` could catch it, since the case is clear: text that ends a line, followed by an inline element on the next line, or an inline element at the end of a line followed by text.

**Suggestion:** Have `quillo check` warn: `` "Hello," and <b> are on separate lines, so no space comes between them: write {" "}, or put them on one line ``. Limit it to inline elements (`b i em strong a span code`, a component), where a missing space shows.

### Q-574. `xs.map((x, i) => ...)` passes `check` and fails when it runs

| Kind | Detected by | Attempts |
|---|---|---|
| loud | run-time error | 1 |

React lists get their keys from the second argument of `.map`. The method form works with one parameter, but Q-515's arity check covers only the function form:

```quillo
let xs = ["a", "b"]
print(<ul>{xs.map((x, i) => <li key={i}>{i}: {x}</li>)}</ul>)
```

`check`: ok. Run: `p/mapindex.qwe:2: missing argument i in call to <fn>`. The function form, `map(xs, (x, i) => i)`, is caught by `check`: `` `map` passes its function one item, but this one takes 2 ``. Neither message says how to get the index. The `for` form a developer then tries, `for (x, i) in xs`, gets `expected a pattern, found (`.

**Suggestion:** Run Q-515's check on `xs.map(f)`, `xs.filter(f)` and the other method forms. Add the way to get an index to both messages: `` …takes 2: for the index, map(enumerate(xs), ([i, x]) => ...) `` (or whatever form the guide gives). If two parameters are common enough, `map` could pass the index to a function that declares a second parameter, as JavaScript does.

### Q-575. JSX's comments in a tag and `{// ...}` are refused with "expected an expression, found `}`"

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

The guide says `{/* ... */}` is a comment, "as in JSX". It is one between children. Two other places where JSX allows a comment are refused, with a message that does not mention comments:

```quillo
print(<div {/* note */} class="a">x</div>)
```

```quillo
print(<div>
    {// a note
    }
    <p>a</p>
</div>)
```

Both give `error: expected an expression, found }`. A `//` line between attributes is already accepted (`<div\n    // note\n    class="a">`).

**Suggestion:** Accept `{/* ... */}` between attributes, and `{` followed by only `//` comments and whitespace before `}`, in both places. Otherwise, say `` a comment in a tag: write it on a line of its own starting with //  ``.

### Q-576. `<img></img>`: "`</img>` does not close `<div>`", and a quick fix that breaks the markup

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 |

A void tag ends at its `>`, so the `</img>` after it is taken as the end tag of the element around it:

```quillo
print(<div><img src="a.png"></img><br></br></div>)
```

`error: </img> does not close <div> from line 1: expected </div>`. The quick fix that comes with it (`parser.rs:3170`, "Change to </div>") turns `</img>` into `</div>`. That closes the `<div>` early and leaves `<br></br></div>` after it. XHTML, React (`<img></img>` is valid JSX), and HTML copied from templates all write void elements this way.

**Suggestion:** When the end tag names a void element (`VOID_TAGS`), say `` <img> has no end tag: remove </img> `` with a fix that deletes it. Or accept `</img>` right after `<img ...>` and pass over it, as JSX does.

### Q-577. React's forms get messages that do not say how Quillo writes them: `<React.Fragment>`, `<Tag>`, `props`, `({ title })`, `{...xs}`

| Kind | Detected by | Attempts |
|---|---|---|
| loud | `quillo check` | 1 each |

`<>` gets a good message (`there are no <> fragments: wrap the elements in one...`). Its other spellings, and some nearby React forms, do not:

| Written | Message | What it could say |
|---|---|---|
| `<React.Fragment>...</React.Fragment>` | `expected > to close </React` | the `<>` message |
| `<Fragment>...</Fragment>` | `unknown name Fragment` | the `<>` message |
| `let Tag = "h1"` then `<Tag>hi</Tag>` | `unknown name Tag (did you mean t?)` | `` a tag's name is written out: there are no dynamic tags; use a match, or a view per tag `` (and `t` is a poor suggestion) |
| `view Card(props) = <div>{props.title}</div>`, `<Card title="x"/>` | `<Card> has no parameter title` and `<Card> needs props` | `` a view's parameters are its props, by name: view Card(title) `` |
| `view Card({ title, body }) = ...` | `expected a parameter name, found {` | the same: `` view Card(title, body) `` |
| `<ul>{...xs}</ul>` | `expected an expression, found ..` | `` a list as a child shows each item: write {xs} `` |

**Suggestion:** Add the messages in the right-hand column. The first two can share the code that writes the `<>` message. The `props` one needs the case "a single parameter named `props`, and the call passes names it does not have".

### Q-578. The guide has a "From HTML" table but no "From JSX" one

| Kind | Detected by | Attempts |
|---|---|---|
| docs | reading USERGUIDE.md | 0 |

"HTML literals" opens with "written like JSX". "Coming from JavaScript" ends with one line on `class` and `@click`. "Coming from other frameworks" maps React's hooks. Nowhere lists, for markup, what a React developer writes and what Quillo needs instead. Until the issues above are fixed, that list is Q-560 to Q-577. Some differences stay by design anyway: `class` not `className`, no fragments, no member or dynamic tags, `children` as a parameter, `for` and `key=`, `@mount` for `ref`, `raw()` for `dangerouslySetInnerHTML`, `value={x}` binding by itself, and `0` and `""` being truthy in `&&`.

**Suggestion:** Add a "From JSX" table beside "From HTML" (`#### From JSX`), with the same two columns. Have `book/` follow, as it follows every change.

## What behaves well

| Probe | Result |
|---|---|
| `<>...</>` | `there are no <> fragments: wrap the elements in one, like <div>, or give a list` |
| `className="x"`, `htmlFor="n"` | warned: `className is React's name: write class` |
| `onClick={save()}` | error: `@click needs a function, got null: a handler is a function, @click={() => ...}` |
| `onClick={f}`, `onMouseEnter={f}`, `onKeyDown={f}` | the same as `@click`, `@mouseenter`, `@keydown`: the `mouseenter` handler fired in Chrome |
| `{xs.length > 0 && ...}` | `list has no field length: len(x) is how many it holds` |
| `xs.length === 0`, `undefined` | `there is no ===`; `unknown name undefined (did you mean null?)` |
| `xs.map(x => <li>{x}</li>)`, `xs.filter(...).map(...)` | the list, as in React |
| `return (` markup `)`, ternaries with `(` markup `)` on each side | as in React |
| `{' '}`, `title={'t'}` | single-quoted strings in code work |
| `<div/>`, `<p/>`, `<textarea/>`, `<script src="/a.js"/>`, `<iframe/>` | written with an end tag, so the browser does not swallow what follows |
| `<Badge big/>` | `big` is `true` |
| `<Badge count="3"/>` to `count: int` | `` `count` of <Badge> must be int, got str `` |
| `render={(i) => <li>{i}</li>}` on a view | called as `render(i)`, as a render prop |
| `style={{ WebkitLineClamp: 2, MozAppearance: "none", backgroundColor: "red" }}` | `-webkit-line-clamp`, `-moz-appearance`, `background-color` |
| a `//` line between a tag's attributes | a comment |
| `<a href="x"class="y">` (no space) | taken |
| `{true}`, the entities, `style` numbers, `aria-*` booleans | the browser's own rendering after an update matched the server's in each case: no component was remade, and `__quilloErrors` was empty (the wrong output is at least consistent) |

## Where the AI struggled

| What I wrote first | Why | What happened | Cost |
|---|---|---|---|
| Probes with a `route`, run with `quillo run` and no `--port` | to see the page | port 8080 was taken by another session's server, so `run` stopped at once; replaced with a script that picks a free port and stops only the PID it started | 3 probes |
| One probe for all the event names, with `onClick={save()}` in it | to save files | that line's error stopped the program loading, so the page with the other handlers never ran; split | 1 run |
| `{xs.length}` and `xs.map(...)` in one probe | the same | `.length`'s error hid whether `.map` worked; split | 1 run |
| `onChange` probe with `e.value` only | it is Quillo's form | it showed the blur lag, but not that React's own form (`value={x}` with `e.target.value`) fails the same way; a second page tested that | 1 page |
| Expected `class={{...}}` to be written as JSON, as `write_attr` writes a map | from reading `interp.rs:4220` | `class` is written as Quillo text (`{ active: true, off: false }`), not JSON; reported as seen | 0 |

## Not covered

- The editors' grammars and the language server in markup: highlighting, completion of attribute and event names, hover.
- `quillo fix` on any of the forms above.
- Markup the server and the browser render differently. None of these probes found any, but they were not built to look for it.
- `<select>`/`<option>` with React's `value` on the select, and radio groups with `onChange`.

## Order of work, and questions

1. Q-560 and Q-561 first. They break the most React code with no message, and the warnings are small changes in `attr_typo` and the `on…` rename.
2. Q-562 and Q-563 together, as both change how attribute strings are read.
3. The warnings: Q-568, Q-569, Q-570, Q-571, Q-573, Q-574 (in `check.rs`), then the messages in Q-575 to Q-577, then Q-578.

Questions only you can answer:

- Q-560: should `onChange` on a text field mean `@input`, as in React (code written for React works), or stay the DOM's `change` with a warning (Quillo keeps one meaning per name)?
- Q-564: should `{true}` show nothing, as in React? That changes what existing programs print.
- Q-565: add `px` as React does, or only warn?
- Q-563: attribute strings as plain text means a `\"` written today ends the string. Is one release of warnings enough?
- Q-566: should a map and a list in `class` become classes, or be an error that points to `class:name`?

## Probe scripts

**Free port and browser** (the `browse.sh` used for each page; it stops only the server it started):

```sh
Q=/Users/restiguay/dev/quillo-app1/target/release/quillo
PORT=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1])')
$Q run $1 --port $PORT > q-$PORT.log 2>&1 &
PID=$!
for i in $(seq 1 50); do curl -s -o /dev/null http://127.0.0.1:$PORT/ && break; sleep 0.1; done
curl -s http://127.0.0.1:$PORT/
node tests/browser/run.js "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  http://127.0.0.1:$PORT/ prof-$PORT 20000 $2
kill $PID
```

**Events, `{true}`, style, booleans and entities** (Q-560, Q-561, Q-562, Q-564, Q-565). The page is Q-561's component, with these added:

```quillo
        <button #go @click={() => { n += 1 }}>go</button>
        {n > 0 && <div #made>
            <p #t>[{true}]</p>
            <div #s style={{ fontSize: 12, width: 100 }}>x</div>
            <div #a aria-hidden={true} aria-expanded={false} draggable={true} spellcheck={false}>x</div>
            <p #ent title="&copy; &hellip;">&rarr; &hellip; &times;</p>
        </div>}
        <input #in onChange={(e) => { log = push(log, "change:" + e.value) }}/>
```

```js
(async () => {
  const out = {};
  const tick = (ms = 80) => new Promise((r) => setTimeout(r, ms));
  const $ = (s) => document.querySelector(s);
  try {
    for (let i = 0; i < 200 && !$("#go"); i++) await tick(30);
    await tick(400);
    $("#go").click(); await tick(200);
    out.browser = { t: $("#t").textContent, style: $("#s").getAttribute("style"), fontSize: getComputedStyle($("#s")).fontSize,
      aria: [...$("#a").attributes].map(a => a.name + "=" + JSON.stringify(a.value)), draggable: $("#a").draggable,
      spell: $("#a").spellcheck, ent: $("#ent").textContent, entTitle: $("#ent").title };
    const me = $("#me");
    me.dispatchEvent(new MouseEvent("mouseenter")); await tick();
    me.dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await tick();
    me.click(); await tick();
    out.logAfterMouse = $("#log").textContent;
    const inp = $("#in"); inp.focus(); document.execCommand("insertText", false, "abc"); await tick(200);
    out.logAfterTyping = $("#log").textContent;
    inp.blur(); await tick(200);
    out.logAfterBlur = $("#log").textContent;
    out.remade = window.__quilloRemade || null;
    out.errors = window.__quilloErrors || [];
  } catch (e) { out.exception = String(e && e.stack || e); }
  document.body.setAttribute("data-result", JSON.stringify(out));
})();
```

Result:

```json
{"browser":{"t":"[true]","style":"font-size: 12; width: 100","fontSize":"16px",
  "aria":["id=\"a\"","aria-hidden=\"\"","draggable=\"\""],"draggable":false,"spell":true,
  "ent":"&rarr; … &times;","entTitle":"&copy; &hellip;"},
 "logAfterMouse":"mouseenter","logAfterTyping":"mouseenter","logAfterBlur":"mouseenter,change:abc",
 "remade":null,"errors":[]}
```

The other pages follow the same pattern, with the results quoted in their sections. Controlled input (Q-560): `{"aTyping":["ann",""],"aBlurred":["ann","ann"],"bTyping":["bob",""],"bBlurred":["BOB","BOB"]}`. Focus on a wrapper (Q-561): `{"log":""}`. Spread (Q-567): `{"text":"1","c":["id"],"attrs":["id=\"b\""]}` for `"@click"` and `onclick`, and `{"text":"0","attrs":["id=\"b\"","title=\"t\"","classname=\"big\"","style=\"font-size: 20\""]}` for `onClick`. Classes (Q-566): `{"before":["{ active: true, off: false }","[\"a\", \"b\"]"],"after":["{ active: off: } false, true","[\"a\", false]"]}`. React attributes (Q-570): `{"defaultValue":"","defaultChecked":false,"strokeWidth":["1px","5px"]}`.
