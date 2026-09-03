# Object 3 — `StmtFunctionDef` and `StmtClassDef`

The two nodes your node tree is built from. And the one trap in this exercise
that will silently break your output.

---

## What they are

```rust
pub struct StmtFunctionDef {
    pub range: TextRange,                       // ⚠ see the trap below
    pub is_async: bool,                         // ★ `async def`
    pub decorator_list: ThinVec<Decorator>,     // ⚠ see the trap below
    pub name: Identifier,                       // ★
    pub type_params: Option<Box<TypeParams>>,   // `def f[T]()` — 3.12+
    pub parameters: Box<Parameters>,
    pub returns: Option<Box<Expr>>,             // `-> int`
    pub body: Vec<Stmt>,                        // ★ the statements inside
    pub node_index: AtomicNodeIndex,            // ignore this
}

pub struct StmtClassDef {
    pub range: TextRange,
    pub decorator_list: ThinVec<Decorator>,
    pub name: Identifier,
    pub type_params: Option<Box<TypeParams>>,
    pub arguments: Option<Box<Arguments>>,      // ★ the base classes: `class C(A, B)`
    pub body: Vec<Stmt>,
    pub node_index: AtomicNodeIndex,
}
```

**[verified]** from `ruff_python_ast/src/generated.rs:9301`.

### Three things to note straight away

**1. `StmtFunctionDef` covers `async def` too.** There is no separate node — just
`is_async: bool`. parso wraps async functions in an `async_stmt` node; ruff does
not.

**2. `StmtClassDef.arguments` is where base classes live.** `class Foo(Bar):` puts
`Bar` in `arguments`, the same `Arguments` type a *call* uses (object 4). It is
`Option` because `class Foo:` has none at all — not an empty list, but `None`.

**3. `Parameters` is a `Box`, `body` is a plain `Vec`.** No reason you need to
care, but it explains why `def.body` needs no dereferencing and
`def.parameters` sometimes does.

---

## ⚠ The trap: decorators are inside the range

```python
@functools.cache
def decorated():
    ...
```

**In ruff, `StmtFunctionDef.range` starts at the `@`, not at the `def`.**

**[verified]** — `parse_decorators` binds `start = self.node_start()` *before*
consuming the `@`, then passes that same offset into
`parse_function_definition(decorators, start)`
(`ruff_python_parser/src/parser/statement.rs:2894, 3021`).

In parso, `Function.start_pos` is at `def`; decorators live in a wrapping
`decorated` node **outside** the function.

### Why this matters more than it looks

On any real codebase — `@property`, `@staticmethod`, `@pytest.fixture`,
`@app.route`, `@dataclass` — a large fraction of all methods are decorated. So
this is not an edge case; it changes the `position` of most of your nodes, and
the M2 gate in `plan/04-build/02-milestones.md` (byte-identical `nodes` JSON)
would fail on it immediately.

### And there is a second, worse consequence

`decorator_list` is a **field of** `StmtFunctionDef`. So a visitor that walks the
def also walks its decorators — and a call inside a decorator becomes a **child
of the function it decorates**.

```python
@functools.wraps(build)      # ← this call
async def decorated_async():
    log()
```

parso puts `functools.wraps(build)` in the enclosing scope (it is in the
`decorated` wrapper, outside the funcdef). Ruff's tree puts it inside. Unless you
handle it, your `decorated_async` node gains a phantom child.

`python/edges.py` has exactly this case, on purpose.

### What to do

Two options, both defensible:

```rust
// Option 1: range from the `def`/`async` keyword onwards.
fn def_range_without_decorators(def: &StmtFunctionDef) -> TextRange {
    match def.decorator_list.last() {
        None => def.range,
        Some(last) => TextRange::new(
            // the first non-trivia after the last decorator — you need the
            // tokens or a trivia scan to find the exact `def`/`async` offset
            last.range().end(),
            def.range.end(),
        ),
    }
}

// Option 2: accept the difference, document it, move on.
```

Option 1 as written gives you a range starting just after the decorator, which
includes the newline and indentation — not quite `def`. Getting to the exact
keyword needs `ruff_python_trivia`'s `SimpleTokenizer` or a scan of
`parsed.tokens()`.

**And in both cases you must skip `decorator_list` when walking the body**, or
you get the phantom child regardless of what the range says.

> **Decide deliberately and write it down.** This is the class of silent parity
> difference the M2 gate exists to catch, and "we never decided" is the worst of
> the three possible states.

---

## ✅ Not a trap: `async def` is already correct

`parser.py:126` has a special case using the parent `async_stmt` position so the
node starts at the `async` keyword.

In ruff, **the range already includes `async`** **[verified,
`statement.rs:2853`]** — the parser threads `async_start` into
`parse_function_definition`.

So the special case simply disappears. `plan/02-mapping/01` marked this
`[check]`; now it is checked.

Note the asymmetry with decorators: same mechanism (the parser choosing a start
offset), opposite outcome. Ruff includes `async` (which you want) **and**
decorators (which you do not). Neither is "better" — the contract is parso's
output.

---

## What you can do with them

### Fields you will use

| field | notes |
|---|---|
| `.name` | ★ an `Identifier`: `.as_str()`, `== "foo"`, `.range()` |
| `.body` | ★ `&[Stmt]` — recurse into this |
| `.range` | ⚠ includes decorators |
| `.is_async` | ★ functions only |
| `.decorator_list` | ⚠ skip when walking; use to find where the header starts |
| `.arguments` | ★ classes only — the base classes |
| `.type_params` | `Some` for `def f[T]()` / `class C[T]` (3.12+) |

### The `Identifier` trait — a second way to get the name range

```rust
use ruff_python_ast::identifier::Identifier;   // the TRAIT, not the struct

def.identifier()        // -> TextRange, the range of just the name
```

**[verified]** — implemented for `StmtFunctionDef`, `StmtClassDef`, `Stmt`,
`Parameter`, `Alias` (`ruff_python_ast/src/identifier.rs:18`).

⚠ Confusing naming: there is a **struct** `Identifier` (the `name` field's type,
object 2) and a **trait** `Identifier` (this). They are different things with
the same name in different modules.

In practice `def.name.range()` gives you the same answer and needs no import, so
you will rarely reach for the trait. Know it exists so the name does not confuse
you when you read ty's source.

---

## Example 1 — list every definition, with the trap visible

```rust
use ruff_python_ast::{Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::Ranged;

fn report(stmt: &Stmt, source: &str, depth: usize) {
    let pad = "  ".repeat(depth);
    match stmt {
        Stmt::FunctionDef(def) => {
            println!(
                "{pad}fn  {:20} range={:?}  name_range={:?}  async={}  decorators={}",
                def.name.as_str(),
                def.range,
                def.name.range(),
                def.is_async,
                def.decorator_list.len(),
            );
            // ⚠ note: walking `def.body` only. NOT `decorator_list`.
            for inner in &def.body {
                report(inner, source, depth + 1);
            }
        }
        Stmt::ClassDef(def) => {
            let bases = def.arguments.as_ref().map_or(0, |a| a.args.len());
            println!(
                "{pad}cls {:20} range={:?}  bases={}",
                def.name.as_str(), def.range, bases,
            );
            for inner in &def.body {
                report(inner, source, depth + 1);
            }
        }
        _ => {}
    }
}
```

Run it on `python/edges.py` and look at the `decorated` row:

```
fn  decorated            range=163..295   name_range=175..184   async=false  decorators=1
    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    range starts at the `@`, name_range starts at the identifier.
    The gap between range.start and name_range.start is the decorator + `def `.
```

Compare `plain_async`:

```
fn  plain_async          range=...        async=true   decorators=0
```

and check that its range starts at `async`, not at `def` — compute the byte
offset of both from the file and see which one matches.

**Rust notes:**

- `def.arguments.as_ref().map_or(0, |a| a.args.len())` — `arguments` is
  `Option<Box<Arguments>>`. `.as_ref()` turns `&Option<Box<T>>` into
  `Option<&Box<T>>`; `map_or(default, f)` gives the default for `None`. This
  four-token dance is extremely common with optional AST fields.
- `"  ".repeat(depth)` — the cheap way to indent recursive output. Keep it; you
  will want it in every tree-printing function from here on.
- The recursion walks `def.body` and nothing else. That is deliberate, and it is
  the fix for the decorator problem.

---

## Example 2 — the nesting rule from `parser.py`

Your Python's `_scan_children` **stops** at a nested def or class
(`parser.py:78-80`): calls inside a nested function are children of *that*
function, never of the enclosing one.

Example 1's `report` already has this shape — it recurses into a definition's
body, and each definition prints its own children. That structure *is* the rule.

Where it gets interesting is statements that are **not** definitions but can
contain them:

```python
def with_blocks(flag):
    if flag:
        def conditional():      # ← a def inside an `if`
            log()
        conditional()
```

`report` above misses `conditional` entirely, because `Stmt::If` falls into
`_ => {}`.

So the rule is not "recurse into definitions" — it is:

> **Recurse into every statement, but treat a definition as a new scope.**

That is two different traversals, and hand-writing both is what object 5's
visitor removes. `python/nested.py`'s `with_blocks` is the fixture that proves
you got it right.

---

## Exercise

**A.** Write `report` from example 1 as `pylspt-dev defs <file>`. Run it on all
five fixtures.

**B.** For `python/edges.py`, compute by hand the byte offsets of:
- the `@` of `@functools.cache` before `decorated`
- the `def` of `decorated`
- the `async` of `plain_async`
- the `def` of `plain_async`

(`grep -bo` gives you byte offsets: `grep -bo 'def decorated' edges.py`.)
Then compare with the `range` your program printed. Write down which keyword each
range starts at. **This is the trap, confirmed by measurement.**

**C.** Add a column showing what fraction of the range is decorator: 
`(name_range.start - range.start)`. Which fixture has the biggest gap?

**D.** Run it on `python/nested.py` and confirm `with_blocks`'s nested
`conditional` is **missing** from your output. Then add `Stmt::If` handling and
watch it appear. Now add `Stmt::Try`, `Stmt::With`, `Stmt::For`, `Stmt::While`…
and stop when you see where this is going.

**E.** For `python/edges.py`'s `decorated_async`, print the calls you find if you
*do* walk `decorator_list`. Confirm that `functools.wraps(build)` appears as a
child. That is the phantom child — see it once so you never ship it.

---

## Exam

**1.** How does ruff represent `async def`? How did parso? What does that do to
`parser.py:126`?

**2.** Where does `StmtFunctionDef.range` start for a decorated function? Cite
the mechanism.

**3.** Where does parso start? What does the difference do to your output, and
how common is it in real code?

**4.** Name the *second* problem decorators cause, beyond the range. Give the
fixture and the phantom output.

**5.** Why is `StmtClassDef.arguments` an `Option` rather than an empty list?
What is the type inside, and where else have you seen it?

**6.** Give the four ways to work with `def.name`.

**7.** There is a struct called `Identifier` and a trait called `Identifier`.
What is each for, and which do you actually need?

**8.** Write the expression for "how many base classes does this class have",
given `def.arguments: Option<Box<Arguments>>`.

**9.** State the nesting rule from `parser.py` in one sentence. Why is "recurse
into definitions" not a correct statement of it?

**10.** `python/nested.py`'s `with_blocks` contains a def inside an `if`. What
must your walk do, and what happens if you only handle definitions?

---

## Answers

**1.** One node with `is_async: bool` **[verified]**. parso wrapped async
functions in an `async_stmt` node, and `parser.py:126` reached up to that wrapper
to get a range starting at `async`.

In ruff the range **already starts at `async`** **[verified,
`statement.rs:2853`]**, so the special case disappears entirely. One of the rare
cases where the port is strictly simpler.

**2.** At the **`@`**. `parse_decorators` takes `start = self.node_start()`
before consuming the decorator and passes that offset to
`parse_function_definition(decorators, start)` **[verified, `statement.rs:2894,
3021`]**.

**3.** parso starts at `def`, with decorators in a separate wrapping node. Every
decorated function therefore gets a different `position` — pointing at the
decorator instead of the definition.

Very common: `@property`, `@staticmethod`, `@classmethod`, `@dataclass`,
`@pytest.fixture`, `@app.route`. On a typical codebase this is most methods, so
the M2 byte-identical gate fails at once.

**4.** `decorator_list` is a **field of the def**, so a visitor that walks the
def walks its decorators too — and any call inside a decorator becomes a child of
the decorated function.

Fixture: `python/edges.py`'s `decorated_async`, decorated with
`@functools.wraps(build)`. The phantom output is a `build` call node nested
inside `decorated_async`. parso puts it in the enclosing scope.

**5.** Because `class Foo:` and `class Foo():` are genuinely different in the
source, and `None` versus `Some(empty)` records that difference. The inner type
is `Arguments` — **the same type a function call uses** (object 4), which is why
`class Foo(Bar)` and `foo(Bar)` are parsed into the same shape.

**6.**

```rust
def.name.as_str()      // -> &str
def.name.range()       // -> TextRange, just the name
def.name == "outer"    // PartialEq<str>
format!("{}", def.name)  // Display
```

**7.** The **struct** `Identifier` is the type of the `name` field — it holds the
name text and its range. The **trait** `Identifier` (in
`ruff_python_ast::identifier`) provides `.identifier() -> TextRange` on
statements.

You need the struct, always. You rarely need the trait, because
`def.name.range()` gives the same answer with no extra import. Know the trait
exists so the duplicate name does not confuse you in ty's source.

**8.**

```rust
def.arguments.as_ref().map_or(0, |a| a.args.len())
```

`as_ref()` because you cannot move out of a borrow; `map_or` to supply 0 for
`None`. Note `args` is the positional list — keywords (like `metaclass=`) are
separate, which matters in exercise 08.

**9.** **Recurse into every statement, but treat a definition as the start of a
new scope.**

"Recurse into definitions" is wrong because definitions are not the only
statements that *contain* statements: `if`, `try`, `with`, `for`, `while` and
`match` all have bodies, and a `def` can hide in any of them.

**10.** Your walk must descend into `Stmt::If`'s body (and `Try`, `With`, `For`,
`While`, `Match`) looking for definitions, while *not* treating those statements
as new scopes.

If you only handle definitions, `conditional` is missing from your output —
along with every function defined under a feature flag, inside a `try/except
ImportError`, or in a `with` block. Those are common patterns, so the omission
is not exotic; it just looks like "some functions are missing" with no pattern
you would notice.
