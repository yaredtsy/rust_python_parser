# Object 6 — docstrings and the `ID:` key

There is no docstring node. There is a convention, and you have to implement it.

---

## What a docstring is

A docstring is **the first statement of a body, when that statement is a string
literal**. That is the whole definition. The AST has no `Docstring` node, no
`get_doc_node()`, no flag.

```rust
use ruff_python_ast::{ExprStringLiteral, Stmt};

fn docstring(body: &[Stmt]) -> Option<&ExprStringLiteral> {
    match body.first()? {
        Stmt::Expr(e) => e.value.as_string_literal_expr(),
        _ => None,
    }
}
```

Three conditions, all necessary:

1. **first** — `body.first()`, not "any statement"
2. **a bare expression** — `Stmt::Expr`, not an assignment or a return
3. **a string literal** — not an f-string, not a number

`python/docstrings.py`'s `not_first_statement` fails condition 1 on purpose: the
string is the *second* statement, so it is a no-op expression and **not** a
docstring. Its ID must not be picked up.

---

## ★ ty already wrote this

```rust
use ty_python_core::definition::docstring_from_body;

pub fn docstring_from_body(body: &[ast::Stmt]) -> Option<&ast::ExprStringLiteral>;
```

**[verified, `ty_python_core/src/definition.rs:229`]** — a `pub fn` in a
`pub mod`, fully reachable from your git dependency.

And on the semantic side:

```rust
definition.docstring(db)    // -> Option<String>
```

**[verified, `definition.rs:157`]** — works for function, class and attribute
definitions.

> ⚠ `plan/02-mapping/01` says `docstring_from_body` "is `pub(crate)` to
> `ty_python_semantic`, but under Option A you can use it directly". **That is
> wrong at `ac201b8`** — it is public, and you are on Option B.
>
> One data point for a general rule: the plan's visibility survey is a snapshot
> and it errs in **both** directions. Check before accepting "you cannot reach
> that" — and check before accepting "you can".

### Which to use where

| you are… | use |
|---|---|
| walking the AST (your node tree) | ★ `docstring_from_body(&def.body)` |
| holding a `Definition` (the call tree) | ★ `definition.docstring(db)` |
| parsing client-supplied unsaved content | your own — there is no `Definition` |

Use ty's for both of the first two. It is the same three conditions you would
write, already tested, and it will track any AST change upstream.

---

## Getting the text out

```rust
let doc: &ExprStringLiteral = docstring_from_body(&def.body)?;
let text: &str = doc.value.to_str();
```

**[verified]** — `to_str()` at `nodes.rs:1470`, and it is exactly what ty's own
`Definition::docstring` calls.

`to_str()` gives you the **decoded** value. That means it has already handled:

| input | `to_str()` gives you |
|---|---|
| `"""ID: abc"""` | `ID: abc` |
| `r"""ID: abc\n"""` | `ID: abc\n` — with a real backslash-n, correctly |
| `'ID: abc'` | `ID: abc` |
| `"""a""" """b"""` | `ab` — implicit concatenation joined |

Compare your Python, which slices `[3:-3]`:

- `r"""…"""` has a **four**-character prefix, so slicing three leaves a stray `"`
- `'…'` has one-character quotes, so slicing three eats real content
- implicit concatenation keeps the internal quotes and the gap

So `to_str()` is not merely more convenient — it is **more correct than the code
you are porting**, on three of the twelve cases in `python/docstrings.py`.

That creates a decision, and it is a real one. Keep reading.

### `is_implicit_concatenated()`

```rust
doc.value.is_implicit_concatenated()      // -> bool   [verified]
```

Worth knowing because implicit concatenation is where hand-rolled unquoting goes
wrong, and where you may want to log a warning during parity testing.

---

## The `ID:` scan

Your Python does:

```python
pairs = re.findall(r"(\S+)\s*:\s*(\S+)", docstring)
metadata = dict(pairs)          # ← later keys OVERWRITE earlier ones
```

Two behaviours in there that you must reproduce:

**1. It finds *every* `key: value` pair**, not just `ID:`. So
`python/docstrings.py`'s `multiple_keys` yields `FileID`, `ID` and `Owner`.

**2. `dict(pairs)` means last-wins.** `python/docstrings.py`'s `id_like_text`
reads:

```
This mentions ID: but as prose, then the real one.

ID: cccccccc-5555-…
```

The regex matches `("ID", "but")` first and `("ID", "cccccccc-…")` second. The
dict keeps the **second**. So the UUID wins.

> **If your Rust returns the first match, you produce `"but"` as the node's ID.**
> A corrupt UUID, silently, on any docstring whose prose happens to contain a
> colon after a word. That is not exotic — `Note: see below`, `Args:`, `Returns:`
> are everywhere in real docstrings.

Reproduce last-wins. It is not obviously right; it is what the current
implementation does, and the contract is the output (`MEMORY.md`).

### Hand-roll it, do not reach for `regex`

`plan/02-mapping/02` says so, and it is right: this runs on **every def in the
project**, and the pattern is trivial to scan by hand.

```rust
/// Extract `key: value` pairs, last-wins, mirroring
/// `re.findall(r"(\S+)\s*:\s*(\S+)")` + `dict(...)`.
fn extract_metadata(doc: &str) -> FxHashMap<&str, &str> {
    let mut out = FxHashMap::default();
    // For each ':', take the non-whitespace run before it as the key
    // and the non-whitespace run after it as the value.
    // …twenty lines, no dependency, no allocation for the keys.
    out
}
```

Write it yourself. The details that matter:

- keys and values are runs of **non-whitespace** (`\S+`)
- whitespace is allowed on either side of the `:`
- later pairs overwrite earlier ones
- a `:` with nothing after it (or only whitespace) matches nothing

⚠ Rust's `regex` crate **does not support lookbehind**, which your Python's
`_build_docstring` pattern uses (`(?<=\s)`). So even if you wanted regex, you
would be hand-rolling that half anyway. Exercise 10 covers it.

---

## Two different rules for two different RPCs

This catches people, and the failure is invisible.

| RPC | a def with no `ID:` |
|---|---|
| `parse_file` | **emit the node** with `id: null` |
| `resolve_calls` | **drop it**, and do not descend into it (quirk 4) |

`parse_file` is the *structural* view: v-noc needs to see the def exists, and an
`id: null` is precisely what triggers injection (exercise 10). Drop those nodes
and you deadlock — the file never reports functions, so injection never runs, so
no IDs ever appear.

The call tree is the *relational* view, and `target_id` is the join key. A node
with no ID cannot be joined, so it goes, along with everything beneath it.

`plan/04-build/00-dev-cli.md` calls this "the silent one":

> `✗ no ID: in docstring   (call_resolver.py:154)   ← the silent one`
>
> The `no-ID:` case is the one that will waste your time — a callee vanishes from
> the tree with no other symptom. Make it loud in trace mode.

`python/tree.py` (exercise 09) has `no_id_callee` for exactly this.

---

## Example 1 — extract every docstring and ID

```rust
use ruff_python_ast::{Stmt, StmtClassDef, StmtFunctionDef};
use ty_python_core::definition::docstring_from_body;

fn report(stmt: &Stmt, depth: usize) {
    let pad = "  ".repeat(depth);
    let (kind, name, body) = match stmt {
        Stmt::FunctionDef(d) => ("fn", d.name.as_str(), &d.body),
        Stmt::ClassDef(d) => ("cls", d.name.as_str(), &d.body),
        _ => return,
    };

    let doc = docstring_from_body(body);
    let id = doc
        .map(|d| d.value.to_str())
        .and_then(|text| extract_metadata(text).get("ID").copied());

    match id {
        Some(id) => println!("{pad}{kind} {name:24} ID: {id}"),
        None if doc.is_some() => println!("{pad}{kind} {name:24} (docstring, no ID)"),
        None => println!("{pad}{kind} {name:24} (no docstring)"),
    }

    for inner in body {
        report(inner, depth + 1);
    }
}
```

**Rust notes:**

- `&d.body` in a match arm producing a tuple — all three arms must have the same
  type, so both give `&Vec<Stmt>`, and the `_ => return` arm exits instead of
  producing one.
- `.map(...).and_then(...)` — `map` transforms the value inside an `Option`;
  `and_then` transforms it into *another* `Option` and flattens. Use `and_then`
  when your closure itself returns `Option`, or you get `Option<Option<T>>`.
- `.copied()` — `get` returns `Option<&&str>`; `copied()` makes it
  `Option<&str>`. `&str` is `Copy`, so this is free.
- The three-way `match` on `Option` with a **guard** (`None if doc.is_some()`)
  distinguishes "no docstring" from "docstring without an ID" — which are
  different situations for injection.

---

## Example 2 — the twelve cases

Run example 1 on `python/docstrings.py` and check every row:

| function | ID? | why |
|---|---|---|
| `plain` | ✓ | ordinary triple-quoted |
| `raw` | ✓ | `r"""…"""` — `to_str()` handles the prefix |
| `single_quotes` | ✓ | `'…'` is still a string literal |
| `implicit_concat` | ✓ | `to_str()` joins the parts |
| `no_docstring` | — | no docstring at all |
| `not_first_statement` | **✗** | the string is the second statement |
| `id_like_text` | ✓ **the UUID** | last-wins; the first match is `"but"` |
| `multiple_keys` | ✓ | three keys; you want `ID` |
| `unicode_doc` | ✓ | non-ASCII before the key |
| `Documented` (class) | ✓ | classes have docstrings too |
| `Documented.method` | ✓ | nested |

`not_first_statement` and `id_like_text` are the two that separate a correct
implementation from a plausible one. If you get `"but"` for `id_like_text`, you
took the first match instead of the last.

---

## Exercise

**A.** Write `extract_metadata` by hand. Test it directly on these strings before
wiring it to the AST:

```
"ID: abc"                          → {ID: abc}
"ID:abc"                           → {ID: abc}
"ID  :  abc"                       → {ID: abc}
"Note: see ID: real-one"           → {Note: see, ID: real-one}
"ID: first\n\nID: second"          → {ID: second}      ← last wins
"ID:"                              → {}
"no colons here"                   → {}
```

Write these as `#[test]` cases in `src/nodes.rs`. They are cheap and they cover
the behaviour that matters.

**B.** Write example 1 as `pylspt-dev ids <file>` and run it on
`python/docstrings.py`. Check all twelve rows against the table. Any mismatch is
a bug in your scan.

**C.** Deliberately switch to first-wins and confirm `id_like_text` produces
`"but"`. Then switch back. Add a test that fails on first-wins.

**D.** Compare your `extract_metadata` output against
`doc.value.to_str()` versus a hand-rolled `[3..len-3]` slice, for `raw`,
`single_quotes` and `implicit_concat`. Show that the slice version breaks on all
three.

**E.** Use `docstring_from_body` and then also try `Definition::docstring(db)`
(you will need a `Definition`, which is exercise 06 — so this one can wait). Note
which is easier from where you are standing.

**F.** Print `doc.value.is_implicit_concatenated()` for every docstring in the
fixture. Which ones are true?

---

## The decision this object forces

`to_str()` is more correct than your Python's `[3:-3]`. So on `raw`,
`single_quotes` and `implicit_concat`, **your Rust will extract an ID where the
Python driver extracted garbage or nothing**.

That is a behaviour change, and the contract is the output.

Think it through before you decide:

- If the Python driver failed to extract an ID from an `r"""…"""` docstring, then
  today those defs have `id: null`, and injection *ran on them* — possibly
  repeatedly, possibly appending a second block each time.
- Which means the corpus you compare against may contain files whose state was
  produced by that bug.

So this is not "shall I preserve a bug"; it is "what does the golden data
actually contain". That question is answered at **M0**
(`plan/04-build/02-milestones.md`) by looking, not by reasoning — which is
exactly why M0 is the first milestone.

Write down what you find. And note that the `[3:-3]` bug is *also* one of the
two exercise-10 bugs you are asked to reproduce in the **injector** — so the read
path and the write path may need different answers, and pretending otherwise is
how you get a driver that injects an ID it cannot then read back.

---

## Exam

**1.** Define "docstring" in terms of the AST. Give the three conditions.

**2.** Why is there no `Docstring` node?

**3.** Which ty function does this for you, what is its visibility, and what does
the plan say about it?

**4.** When would you use `Definition::docstring(db)` instead of
`docstring_from_body`? When would you use neither?

**5.** Give four inputs where `to_str()` is correct and `[3:-3]` is not.

**6.** What does `re.findall(...)` + `dict(...)` do that a first-match scan does
not? Which fixture exposes the difference, and what wrong ID do you get?

**7.** Name three real docstring conventions that would trigger the
`key: value` scan by accident.

**8.** Give the two different no-ID rules, per RPC. What deadlocks if you apply
the call-tree rule in `parse_file`?

**9.** Why does `plan/04-build/00-dev-cli.md` call the no-ID case "the silent
one"?

**10.** Why hand-roll the scan rather than use the `regex` crate? Give two
reasons.

**11.** `to_str()` is more correct than the code you are porting. Why is
"just use the better one" not automatically the right call, and where is that
question actually settled?

---

## Answers

**1.** The **first** statement of a body, when it is a **bare expression**
(`Stmt::Expr`) whose value is a **string literal**. All three conditions
matter: position, statement kind, and expression kind.

**2.** Because a docstring is not a syntactic construct — it is a *convention*
about what the first statement means. Python has no docstring syntax; it has a
string expression that the runtime happens to store in `__doc__`. An AST
represents syntax, so it represents the string.

**3.** `ty_python_core::definition::docstring_from_body(&body) ->
Option<&ExprStringLiteral>`. It is **`pub fn` in a `pub mod`** — fully reachable
**[verified, `definition.rs:229`]**.

`plan/02-mapping/01` claims it is `pub(crate)` and needs Option A. That is wrong
at this revision, and it is one of several plan visibility claims that do not
hold — in both directions.

**4.** Use `Definition::docstring(db)` when you already hold a `Definition` —
i.e. in the call tree, where you resolved a callee. Use `docstring_from_body`
when you are walking the AST and have a `&[Stmt]` in hand — i.e. `parse_file`.

Use **neither** for client-supplied unsaved content: there is no `File`, so no
`Definition`, and you are parsing standalone anyway.

**5.** `r"""…"""` (four-character prefix, so slicing 3 leaves a `"`);
`'…'` (one-character quotes, so slicing 3 eats content); `"""a""" """b"""`
(implicit concatenation keeps internal quotes and the gap); and any string with
escapes, where `to_str()` decodes and a slice does not.

**6.** It collects **every** pair and lets **later ones overwrite earlier ones**.
A first-match scan takes the first.

`id_like_text` exposes it: the prose reads `ID: but as prose`, so the first match
is `("ID", "but")` and the real UUID comes second. First-match gives the node an
ID of **`"but"`** — a silently corrupt join key.

**7.** `Args:`, `Returns:`, `Raises:`, `Note:`, `Example:`, `Yields:` — every
Google-style and NumPy-style docstring section header. Also any prose sentence
with a colon after a single word. This is common, not exotic, which is why
last-wins matters.

**8.** `parse_file` emits the node with `id: null`; `resolve_calls` drops it and
does not descend (quirk 4).

Applying the call-tree rule in `parse_file` **deadlocks injection**: the file
reports no functions, so nothing tells v-noc there are defs needing IDs, so
injection never runs, so no IDs ever appear. The system is stuck in a state that
looks like "this file has no code in it".

**9.** Because a callee simply **vanishes from the tree with no other symptom** —
no error, no warning, no gap in the output that looks like a gap. The subtree is
just absent. Everything else that drops a callee (builtin, non-project) is
predictable; this one depends on whether someone's docstring happens to have been
injected yet.

**10.** (1) It runs on **every def in the project**, so it is hot, and a regex
engine is heavy for `find(':')` plus two whitespace scans. (2) Rust's `regex`
crate has **no lookbehind**, which your Python's `_build_docstring` pattern uses
— so you are hand-rolling part of it regardless.

**11.** Because the contract is the observable output, and "better" changes it.
Concretely: if the old driver could not read an ID out of an `r"""…"""`
docstring, then those defs are `id: null` today and the injector has been running
on them — so the golden corpus may contain state *produced by the bug*. Your
"more correct" reader would then extract an ID that the old system never
produced, and the diff would show a change you cannot explain from the code.

Settled at **M0** (`plan/04-build/02-milestones.md`): run the Python driver over
a real corpus, record the responses, and look. It is a question about data, not
about design — which is the whole reason M0 comes before any Rust.
