# Object 4 — `ExprCall` and `Arguments`

Calls. The node your whole tool is about, and the one place where ruff's tree
has a genuinely different **shape** from parso's.

---

## What they are

```rust
pub struct ExprCall {
    pub range_start: TextSize,      // ⚠ NOT a TextRange. see below.
    pub func: Box<Expr>,            // ★ what is being called
    pub arguments: Arguments,       // ★ the (…) part
    pub node_index: AtomicNodeIndex,
}

pub struct Arguments {
    pub range: TextRange,           // ★★ spans `(` to `)` INCLUSIVE
    pub args: Box<[Expr]>,          // positional
    pub keywords: ThinVec<Keyword>, // keyword
    pub node_index: AtomicNodeIndex,
}
```

**[verified]** from `generated.rs:9789` and `nodes.rs:3499`.

### ⚠ `ExprCall` has no `range` field

Look again: it has `range_start`, a single offset. The full range is **computed**:

```rust
// ruff_python_ast/src/nodes.rs:1331   [verified]
impl Ranged for ExprCall {
    fn range(&self) -> TextRange {
        TextRange::new(self.range_start, self.arguments.end())
    }
}
```

So `call.range()` works, `call.range` does **not** compile. For a call, the
`Ranged` trait is not a convenience — it is the only way to get a range at all.

```rust
use ruff_text_size::Ranged;      // ★ mandatory for calls
```

This also tells you something true about the AST: a call *ends* where its
arguments end. There is nothing after the `)`.

---

## ★★ `call_col_pos` is one field access

`plan/00-orientation/01` (quirk 12) says `call_col_pos` is **the column of the
`(`**. In parso you find it by scanning trailers for the opening parenthesis.

In ruff, `Arguments` is documented as spanning

> "from the left to right parentheses (inclusive)"

**[verified, `ruff_python_ast/src/nodes.rs:3483`]**. So its start **is** the `(`:

```rust
let open_paren: TextSize = call.arguments.range.start();
```

One field access. No token scanning, no `SimpleTokenizer`, no skipping comments.
And it stays correct through:

```python
foo (  # a comment
    1)
```

because the `Arguments` range starts at the real `(` regardless of what is
between it and the callee.

The same is true for classes: `class Foo(Bar):` has an `Arguments` node covering
`(Bar)`, from `class_def.arguments` (object 3).

---

## The shape difference: call chains

This is the one real structural change from parso, and it is where your
`call_index` comes from.

**parso** gives a flat `atom_expr` whose children are trailers:

```
a.b().c()

atom_expr
├── Name "a"
├── trailer ".b"
├── trailer "()"      ← call_index 0
├── trailer ".c"
└── trailer "()"      ← call_index 1
```

**ruff** gives a nested expression, **outermost first**:

```
ExprCall                              ← the OUTER call. parso called this index 1
├── func: ExprAttribute (.c)
│         └── value: ExprCall         ← the INNER call. parso called this index 0
│                    ├── func: ExprAttribute (.b)
│                    │         └── value: ExprName "a"
│                    └── arguments: ()
└── arguments: ()
```

So parso's "position in the trailer list" becomes **"depth from the innermost
call in this chain"**.

### Walking a chain inside-out

To reproduce parso's numbering you descend through `call.func`, collecting every
`ExprCall`, then reverse:

```rust
/// Walk a call chain inside-out, mirroring parso's trailer order.
/// Returns calls with call_index 0,1,2… from innermost to outermost.
fn flatten_call_chain<'a>(outer: &'a ExprCall) -> Vec<&'a ExprCall> {
    let mut chain = Vec::new();
    let mut cur = Some(outer);
    while let Some(call) = cur {
        chain.push(call);
        cur = match call.func.as_ref() {
            Expr::Call(inner) => Some(inner),                     // f()()
            Expr::Attribute(attr) => attr.value.as_call_expr(),    // f().g()
            Expr::Subscript(sub) => sub.value.as_call_expr(),      // f()[k].g()
            _ => None,
        };
    }
    chain.reverse();          // innermost first == call_index 0
    chain
}
```

**Three descent kinds**, and `python/calls.py` has a fixture for each:

| descent | fixture | Python |
|---|---|---|
| `Expr::Call` → itself | `call_of_call` | `build()()` |
| `Expr::Attribute` → `.value` | `chained`, `deep_chain` | `build().render()` |
| `Expr::Subscript` → `.value` | `subscripted` | `build()["k"].render()` |

**Rust notes:**

- `while let Some(call) = cur` — loop while the option is `Some`, binding the
  inner value. The idiomatic Rust "follow a linked structure" loop.
- `call.func.as_ref()` — `func` is a `Box<Expr>`; `as_ref()` gives `&Expr` so
  `match` can see the variants (object 2).
- `attr.value.as_call_expr()` — returns `Option<&ExprCall>` directly, which is
  exactly what `cur` wants. Neater than a nested match.

---

## The three call fields, and where each comes from

From `plan/00-orientation/01`'s wire shape:

| field | parso | ruff |
|---|---|---|
| `position` | `atom_expr.start_pos` .. `trailer.end_pos` | **`chain[0].range().start()`** .. `this_call.range().end()` |
| `call_col_pos` | column of `(` | **`this_call.arguments.range.start()`** |
| `name` | text of the prefix children | **source slice of `this_call.func.range()`**, trimmed |

Note the `position` row carefully: the **start** comes from the *innermost* call
in the chain (which is where the whole expression begins — the `a` in
`a.b().c()`), while the **end** comes from *this* call. So every call in one
chain shares `line` and `column` but has a different `end_line`/`end_column`.

That is parso's behaviour too, and reproducing it is quirk 12.

### `name` and the multiline problem

`name` is a source slice of the callee expression. For `obj.render(x)` that is
`"obj.render"`. But `python/calls.py`'s `multiline_callee` is:

```python
    return obj \
        . render (value=1)
```

The raw slice is `"obj \\\n        . render"` — backslash, newline, spaces and
all. Your Python built the name from cleaned prefix children, so it produced
`"obj.render"`.

`plan/02-mapping/01` argues for the **raw slice** (fast, matches the source) and
flagging it as a known difference. Whichever you choose, **write the decision
down** — during parity testing someone will ask why a name has a newline in it,
and you want the answer to be "deliberately" rather than "huh".

---

## What you can do with `Arguments`

**[verified]** from `nodes.rs:3568-3603`. This is more useful than it looks —
you will need it in `plan/03-call-tree/05` for argument binding.

| method | returns |
|---|---|
| `.len()` | total count, positional + keyword |
| `.is_empty()` | `bool` |
| `.range` | ★ `(` to `)` inclusive |
| `.args` | `&[Expr]` — positional |
| `.keywords` | `&[Keyword]` — `k=v` |
| `.find_keyword("key")` | `Option<&Keyword>` |
| `.find_positional(0)` | `Option<&Expr>` |
| `.find_argument_value("key", 0)` | ★ `Option<&Expr>` — "by name **or** position" |
| `.find_argument("key", 0)` | `Option<ArgOrKeyword>` |

`find_argument_value(name, position)` is the one to remember: it answers "what
was passed for this parameter", whether the caller wrote it positionally or by
keyword. That is exactly the question argument binding asks, and ruff has
already written it.

---

## Example 1 — every call in a file, with all four fields

```rust
use ruff_python_ast::{Expr, ExprCall};
use ruff_source_file::SourceCode;
use ruff_text_size::Ranged;

fn report_chain(code: &SourceCode<'_, '_>, outer: &ExprCall) {
    let chain = flatten_call_chain(outer);
    let chain_start = chain[0].range().start();

    for (call_index, call) in chain.iter().enumerate() {
        let name = code.slice(call.func.as_ref()).trim();
        let position = to_position(code, TextRange::new(chain_start, call.range().end()));
        let col = code.line_column(call.arguments.range.start());

        println!(
            "call_index={call_index}  name={name:?}  \
             pos={}:{}..{}:{}  call_col_pos={}  args={}",
            position.line, position.column,
            position.end_line, position.end_column,
            col.column.to_zero_indexed(),
            call.arguments.len(),
        );
    }
}
```

Run it on `python/calls.py`'s `chained` (`build().render()`):

```
call_index=0  name="build"          pos=20:11..20:18  call_col_pos=16  args=0
call_index=1  name="build().render" pos=20:11..20:27  call_col_pos=25  args=0
```

Two things to check against your own output:

- both share the **start** position (`20:11`) and differ in the **end**
- `name` for index 1 includes the inner call — because the callee expression
  really is `build().render`. parso produced the same thing from its prefix
  children.

---

## Example 2 — the fixture checklist

```
python/calls.py
├── simple           1 call,  index 0
├── chained          2 calls, indices 0,1 — shared start
├── deep_chain       3 calls, indices 0,1,2
├── nested_args      3 calls: `wrap(...)` plus TWO `build()` — each index 0
├── call_of_call     2 calls — descent through Expr::Call
├── subscripted      2 calls — descent through Expr::Subscript
├── in_fstring       2 calls inside an f-string
└── multiline_callee 1 call — the `name` question
```

`nested_args` is the one that catches the most common bug. `wrap(build(), key=build())`
is **three** call nodes, each with `call_index: 0`, because the two `build()`
calls are separate chains that happen to appear inside another call's arguments.

If you get one, you handled the outer call and forgot to keep walking into
`arguments`. That loses calls **silently** — no error, just a smaller tree.

---

## Exercise

**A.** Write `flatten_call_chain` and `report_chain`, as
`pylspt-dev calls <file>`. Run it on `python/calls.py`.

**B.** Fill in the expected counts for all eight functions in `calls.py` before
running, then compare. Any mismatch is a bug in your walk, not in the fixture.

**C.** For `chained`, verify by hand from the file bytes that:
- both calls' `position.column` is the column of `build`
- `call_col_pos` for index 0 is the column of the first `(`
- `call_col_pos` for index 1 is the column of the second `(`

Use `grep -bo` to get byte offsets and check your columns.

**D.** Print `name` for `multiline_callee`. Decide raw-slice or normalised.
Write the decision and the reason in a comment in your source.

**E.** Try to make `call.range` compile (without the parentheses). Read the
error. Then explain in one sentence why `ExprCall` is stored this way.

**F.** Use `find_argument_value` to answer, for `nested_args`
(`wrap(build(), key=build())`): what expression was passed for the parameter
named `key`? And for position 0? You now have argument binding's core primitive
— which is `plan/03-call-tree/05`'s subject, arriving three exercises early.

---

## Exam

**1.** Why does `ExprCall` have `range_start` instead of `range`? What must you
import to get a call's range at all?

**2.** What does an `ExprCall`'s range end at, and what does that tell you about
the syntax?

**3.** Give the one expression for `call_col_pos`, and quote the documentation
sentence that guarantees it.

**4.** Why does `foo (  # comment\n  1)` not break `call_col_pos`?

**5.** Draw the ruff AST for `a.b().c()`. Which node is `call_index` 0?

**6.** Name the three expression kinds `flatten_call_chain` must descend
through, with a fixture for each.

**7.** For `build().render().strip()`: which parts of the three calls' positions
are shared, and which differ?

**8.** `wrap(build(), key=build())` — how many call nodes, and what is each
one's `call_index`? What bug produces the answer "one"?

**9.** What is `name` for `multiline_callee`, raw? What did parso produce? Which
does the plan recommend and why?

**10.** Which `Arguments` method answers "what was passed for parameter `key`,
whether positionally or by keyword"? Why does that matter later?

---

## Answers

**1.** Because a call's end is always its arguments' end, so storing a full range
would duplicate information. `Ranged::range()` computes
`TextRange::new(self.range_start, self.arguments.end())` **[verified,
`nodes.rs:1331`]**.

You must import `ruff_text_size::Ranged`. For calls this is not optional
convenience — there is no `range` field to fall back on.

**2.** At the closing `)` of its arguments. That tells you nothing in the syntax
follows a call's arguments — the call expression *is* callee-plus-arguments, and
anything after it (`.foo`, `[k]`) belongs to an enclosing node.

**3.**

```rust
call.arguments.range.start()
```

> "The `Arguments` node would span from the left to right parentheses
> (inclusive), and contain …" — **[verified, `nodes.rs:3483`]**

**4.** Because the `Arguments` range starts at the actual `(` token, wherever it
is. The parser found it; you are reading a recorded fact, not scanning text. Any
whitespace, comments or line continuations between the callee and the `(` are
irrelevant.

**5.**

```
ExprCall (outer)                     ← call_index 1
└── func: ExprAttribute .c
          └── value: ExprCall        ← call_index 0
                     └── func: ExprAttribute .b
                               └── value: ExprName "a"
```

`call_index` 0 is the **innermost** — `a.b()` — because parso numbered trailers
left to right, and the leftmost trailer corresponds to the deepest ruff node.

**6.** `Expr::Call` → itself (`call_of_call`, `build()()`); `Expr::Attribute` →
`.value` (`chained`, `deep_chain`); `Expr::Subscript` → `.value`
(`subscripted`).

**7.** All three share `line` and `column` — the start of the whole chain, the
`b` of `build`. They differ in `end_line`/`end_column` (each ends at its own
`)`) and in `call_col_pos` (each has its own `(`).

**8.** **Three** nodes, each with `call_index: 0`. `wrap(...)` is a chain of
length one; each `build()` is its own independent chain of length one.

The answer "one" comes from handling `Expr::Call` and **not continuing to walk
into `arguments`**. It is the most common bug in this exercise and it fails
silently — you just get a smaller tree.

**9.** Raw, it is `"obj \\\n        . render"` — including the backslash,
newline and indentation. parso produced `"obj.render"`, because
`_get_clean_code` stripped prefixes.

`plan/02-mapping/01` recommends the **raw slice**: it is fast, it matches the
source exactly, and normalising means reimplementing parso's prefix cleaning.
The cost is a documented parity difference on a rare construct — so flag it and
check whether v-noc downstream cares.

**10.** `find_argument_value(name, position)` **[verified]**. It matters because
that is precisely the question **argument binding** asks in
`plan/03-call-tree/05`: to build an environment you must know which expression
was supplied for each parameter, and callers may write it either way. Ruff has
already implemented the lookup, so the interpreter does not have to.
