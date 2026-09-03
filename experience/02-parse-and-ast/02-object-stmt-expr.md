# Object 2 — `Stmt` and `Expr`

The two enums the whole AST is made of. If you understand these, you can read
any ty code.

---

## What they are

Python has two kinds of thing: **statements** (do something) and **expressions**
(produce a value). Ruff has one enum for each.

```rust
pub enum Stmt {
    FunctionDef(StmtFunctionDef),
    ClassDef(StmtClassDef),
    Return(StmtReturn),
    Assign(StmtAssign),
    If(StmtIf),
    For(StmtFor),
    While(StmtWhile),
    With(StmtWith),
    Try(StmtTry),
    Import(StmtImport),
    ImportFrom(StmtImportFrom),
    Expr(StmtExpr),          // ← a bare expression used as a statement
    Pass(StmtPass),
    …                        // 25 variants total
}

pub enum Expr {
    Call(ExprCall),          // ★ the one you care about
    Name(ExprName),          // ★ `foo`
    Attribute(ExprAttribute),// ★ `obj.foo`
    Subscript(ExprSubscript),// ★ `obj[k]`
    Lambda(ExprLambda),      // ★ quirk 8
    StringLiteral(ExprStringLiteral),  // ★ docstrings
    FString(ExprFString),
    BinOp(ExprBinOp),
    Compare(ExprCompare),
    ListComp(ExprListComp),
    …                        // 33 variants total
}
```

**[verified]** from `ruff_python_ast/src/generated.rs:126` and `:1299`.

Two things to notice immediately:

- Each variant holds a **separate struct** — `Stmt::FunctionDef(StmtFunctionDef)`.
  The struct is where the fields live.
- `Stmt::Expr(StmtExpr)` is the "expression used as a statement" case. A
  docstring is a `Stmt::Expr` whose value is an `Expr::StringLiteral`. You will
  match that exact pattern in object 6.

---

## The parso comparison

| parso | ruff |
|---|---|
| `node.type == "funcdef"` — a string | `Stmt::FunctionDef(def)` — a variant |
| `node.children` — a generic list | typed fields: `def.body`, `def.name`, … |
| whitespace and comments are nodes | not represented at all |
| `isinstance(node, Function)` | `matches!(stmt, Stmt::FunctionDef(_))` |

The parso way is flexible and unchecked: a typo in `"funcdef"` is a bug you find
at runtime, and `node.children[2]` means whatever it means today.

The ruff way is checked: `def.body` is a `Vec<Stmt>` and the compiler knows it.
A typo does not compile, and adding a variant upstream breaks every `match` that
does not handle it — which is exactly what you want when the language grows.

---

## Rust: how to read an enum

This is the section to read slowly if `match` is not yet automatic for you.

### `match` — handle every case

```rust
fn describe(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::FunctionDef(_) => "a function",
        Stmt::ClassDef(_) => "a class",
        Stmt::Expr(_) => "a bare expression",
        _ => "something else",           // ← the catch-all
    }
}
```

- The `_` inside a variant means "I do not care about the payload".
- The bare `_ =>` arm at the end means "every other variant".
- **Without a catch-all, the compiler requires every variant.** With 25 of them,
  you will normally want `_ =>`.

**When to omit the catch-all:** when you *want* to be forced to revisit the code
if a variant is added. For a 25-variant enum in a fast-moving upstream crate,
that is usually not what you want.

### Binding the payload

```rust
match stmt {
    Stmt::FunctionDef(def) => {
        println!("function {} at {:?}", def.name, def.range);
        //                  ^^^^^^^^ `def` is a &StmtFunctionDef
    }
    _ => {}
}
```

Because `stmt` is a `&Stmt`, `def` is automatically a `&StmtFunctionDef` — Rust
matches "through" the reference for you (this is *match ergonomics*; older Rust
needed `ref def`).

### `if let` — one case only

```rust
if let Stmt::FunctionDef(def) = stmt {
    println!("function {}", def.name);
}
```

Exactly `match` with one arm and an empty catch-all. Use it when you care about
one variant.

### `let ... else` — the early-return version

```rust
let Stmt::FunctionDef(def) = stmt else {
    return;      // not a function; nothing to do
};
// from here on, `def` is available and we know it is a function
println!("function {}", def.name);
```

This is the idiom ruff and ty use constantly, and it is the one that keeps code
flat instead of nested five levels deep. Learn to recognise it — you will see it
in every file of ty's source.

### `matches!` — just a boolean

```rust
if matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
    // it is a definition of some kind
}
```

The `|` means "or" in a pattern. This is your `isinstance(node, (Function, Class))`.

### The `as_*` helpers

Ruff generates conversion methods for every variant **[verified,
`generated.rs`]**:

```rust
stmt.as_function_def_stmt()      // -> Option<&StmtFunctionDef>
expr.as_call_expr()              // -> Option<&ExprCall>
expr.as_name_expr()              // -> Option<&ExprName>
expr.as_string_literal_expr()    // -> Option<&ExprStringLiteral>
expr.is_call_expr()              // -> bool
```

These pair beautifully with `?` in a function returning `Option`:

```rust
fn callee_name(call: &ExprCall) -> Option<&str> {
    Some(&call.func.as_name_expr()?.id)
}
```

Two lines instead of a nested match. Use whichever reads better — they compile
to the same thing.

---

## Rust: `Box` in the AST

You will meet this constantly:

```rust
pub struct ExprCall {
    pub func: Box<Expr>,          // ← Box
    pub arguments: Arguments,
    …
}
```

**Why:** `Expr` contains variants that contain `Expr`s. Without indirection the
type would be infinitely large — Rust must know a struct's size at compile time.
`Box<Expr>` is a pointer, so the size is fixed.

**How to get through it:** you usually do not have to.

```rust
call.func.range()                    // ✓ auto-deref
call.func.as_name_expr()             // ✓ auto-deref
match call.func.as_ref() { … }       // ✓ explicit &Expr when a match needs it
match &*call.func { … }              // ✓ same thing, older style
```

Rust auto-dereferences for method calls, so `Box` is mostly invisible. It only
becomes visible when you `match`, and then `.as_ref()` is the readable fix.

**Rust note.** In ty's source you will see `&**expr`, `expr.as_ref()`, and
`&*expr` used interchangeably. They all mean "get me the `&Expr` inside". Do not
read significance into which one an author picked.

---

## Example 1 — classify the top level

```rust
use ruff_python_ast::Stmt;

let ast = parsed.syntax();

for stmt in &ast.body {
    let (kind, name) = match stmt {
        Stmt::FunctionDef(def) => ("function", def.name.to_string()),
        Stmt::ClassDef(def) => ("class", def.name.to_string()),
        Stmt::Import(_) => ("import", String::new()),
        Stmt::ImportFrom(imp) => (
            "from-import",
            imp.module.as_ref().map(ToString::to_string).unwrap_or_default(),
        ),
        Stmt::Expr(e) if e.value.is_string_literal_expr() => {
            ("docstring", String::new())
        }
        Stmt::Assign(_) => ("assignment", String::new()),
        other => ("other", format!("{:?}", other.range())),
    };
    println!("{:12} {:20} {:?}", kind, name, stmt.range());
}
```

Run it on `python/nested.py`:

```
docstring                         0..104
from-import  helpers              106..139
function     outer                141..319
class        Container            322..730
function     with_blocks          733..1043
```

**Rust notes:**

- `Stmt::Expr(e) if e.value.is_string_literal_expr()` — a **match guard**. The arm
  only fires when the condition holds. This is how you distinguish a docstring
  from any other bare expression.
- `def.name.to_string()` — `name` is an `Identifier`, not a `String`. It
  implements `Display`, so `to_string()` works, and so does `{}` in a format
  string.
- `other => … other.range()` — binding the catch-all to a name lets you still use
  it. `Stmt` implements `Ranged`, so `.range()` works on the enum itself, not
  just the variant structs. (Needs `use ruff_text_size::Ranged;`.)

---

## Example 2 — find every call in a statement, by hand

```rust
use ruff_python_ast::{Expr, Stmt};

/// Collect calls in this statement's own expressions.
/// NOT recursive into nested statements — that is object 5's job.
fn calls_in<'a>(stmt: &'a Stmt, out: &mut Vec<&'a ExprCall>) {
    match stmt {
        Stmt::Expr(e) => collect_expr(&e.value, out),
        Stmt::Return(r) => {
            if let Some(value) = &r.value {
                collect_expr(value, out);
            }
        }
        Stmt::Assign(a) => collect_expr(&a.value, out),
        _ => {}
    }
}

fn collect_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a ExprCall>) {
    if let Expr::Call(call) = expr {
        out.push(call);
    }
    // …and you would have to recurse into every other variant that can
    // contain an expression. BinOp has left and right. Compare has a list.
    // FString has elements. Subscript has value and slice. And so on,
    // for 33 variants.
}
```

**Stop when you get here.** The comment is the lesson: doing this by hand means
enumerating every expression variant that can contain another expression, and
you will miss some. `python/calls.py` has calls inside f-strings, inside
comprehensions, inside default arguments, inside subscripts.

That is what object 5 (`SourceOrderVisitor`) is for — it already knows how to
walk every variant. Write this by hand once, feel the problem, then use the
visitor.

**Rust note — `&'a Stmt` and `Vec<&'a ExprCall>`.** The lifetime says the
collected references live as long as the statement they came from. You are
storing borrowed nodes, not copies — which is fine and cheap, as long as the
`ParsedModuleRef` outlives your vector.

---

## Exercise

**A.** Write example 1 as a `pylspt-dev toplevel <file>` command. Run it on all
five fixtures. For `python/edges.py`, how many top-level statements are there,
and how many are functions?

**B.** Extend it to report, for each `Stmt::FunctionDef`, whether
`def.is_async` and how many decorators it has. Check against `python/edges.py`,
which has `plain_async`, `decorated`, and `decorated_async` with two decorators.

**C.** Write `is_definition(stmt) -> bool` using `matches!`, returning true for
functions and classes. Then write the same thing three more ways: with `match`,
with `if let`, and with the `as_*` helpers. Pick the one you find clearest and
note why.

**D.** Write example 2 and run it on `python/calls.py`. Count how many calls it
finds. Then count by hand (or with `grep -c '('`) how many are actually there.
Write down the gap and which variants you would have to add to close it. Do not
close it — object 5 does.

**E.** Take `python/docstrings.py` and print, for each top-level statement,
whether it is a docstring — using the match guard from example 1. Confirm that
`not_first_statement`'s string is **not** reported as a docstring at the module
level (it is inside a function, so you should not see it at all).

---

## Exam

**1.** What is the difference between `Stmt` and `Expr`? Give a Python line that
is both.

**2.** `Stmt::FunctionDef(StmtFunctionDef)` — why is there a separate struct
rather than fields on the variant?

**3.** How do you check "is this a function definition" in parso, and in ruff?
Which one catches a typo, and when?

**4.** Write the `let ... else` idiom for "if this is not a call, return early".
Why does ty's source use it so much?

**5.** What does `matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_))` mean,
and what is the Python equivalent?

**6.** Why is `ExprCall.func` a `Box<Expr>` and not an `Expr`?

**7.** Name three ways to get the `&Expr` out of a `Box<Expr>`. Is there a
meaningful difference?

**8.** What is a match guard, and what do you need one for when finding
docstrings?

**9.** `def.name` is not a `String`. What is it, and how do you print it?

**10.** Why does hand-writing "find every call" not work? What is the specific
thing you would keep missing?

**11.** When should a `match` on `Stmt` *not* have a `_ =>` arm?

---

## Answers

**1.** A statement performs an action; an expression produces a value. A bare
expression used as a statement is both — `foo()` on its own line is a
`Stmt::Expr` whose `value` is an `Expr::Call`. That is the wrapper you will match
through constantly.

**2.** Because the payload has many fields (`range`, `is_async`,
`decorator_list`, `name`, `parameters`, `body`, …), and a named struct lets them
be documented, constructed and matched independently. It also means functions can
take `&StmtFunctionDef` directly rather than an enum they have to re-match.

**3.** parso: `node.type == "funcdef"` or `isinstance(node, Function)`. ruff:
`matches!(stmt, Stmt::FunctionDef(_))`.

Ruff catches a typo **at compile time** — `Stmt::FuncDef` does not exist and will
not build. parso catches it at runtime, or never: `node.type == "funcdefx"` is
simply always false, so the code silently does nothing.

**4.**

```rust
let Expr::Call(call) = expr else { return };
```

Because it keeps the happy path **flat**. The alternative nests the rest of the
function inside an `if let`, and with three or four such checks you are five
levels deep. Ruff and ty are full of early-return guards for this reason.

**5.** "Is this statement a function definition or a class definition?" — `|` is
"or" in a pattern. Python equivalent:
`isinstance(node, (Function, Class))`.

**6.** Because `Expr` variants contain `Expr`s — `ExprCall` holds a `func` which
may itself be a call. Without indirection the type would have no finite size, and
Rust needs to know a struct's size at compile time. `Box` is a pointer, so the
size is fixed.

**7.** `call.func.as_ref()`, `&*call.func`, `&**boxed_in_a_ref` — plus automatic
deref for method calls, so `call.func.range()` needs nothing at all. **No
meaningful difference**; they compile identically. Use `.as_ref()` for
readability in a `match`.

**8.** A condition attached to a match arm: `Stmt::Expr(e) if
e.value.is_string_literal_expr()`. You need one for docstrings because the AST has
no "docstring" node — a docstring is *any* `Stmt::Expr` holding a string literal,
and only its position (first in the body) makes it a docstring. The guard checks
the string part; you check the position separately.

**9.** An `Identifier` — a struct holding `id: Name` and `range: TextRange`
**[verified, `nodes.rs:3841`]**. It provides, all verified:

```rust
def.name.as_str()          // -> &str
def.name.id()              // -> &Name
def.name == "outer"        // PartialEq<str> — compare directly, no conversion
def.name.range()           // ★ the range of just the NAME, not the whole def
{}  /  .to_string()        // Display
```

`Deref<Target = str>` and `AsRef<str>` are both implemented, so most string
methods work directly.

The useful one to notice is `range()`: it gives you the span of the identifier
alone, which is what you want for a "selection range" — and it is why you rarely
need the `Identifier` trait's `identifier()` helper from object 3.

**10.** Because you would have to enumerate **every expression variant that can
contain another expression** — `BinOp` (left, right), `Compare` (a list),
`FString` (elements), `Subscript` (value, slice), `ListComp`, default arguments,
and about twenty more. You will always miss some, and the ones you miss are
silent: the call is simply absent from your tree.

`python/calls.py` is built to expose this: calls inside f-strings, inside
comprehensions, and inside default arguments each need a different variant
handled.

**11.** When you *want* the compiler to force you to revisit the code if the
enum grows — for example, an exhaustive conversion where a new statement kind
must be handled deliberately.

For a 25-variant enum in a fast-moving upstream crate, that is usually a
liability rather than a feature: every ruff upgrade would break your build for
variants you do not care about. Use `_ =>` and be deliberate about the ones you
list.
