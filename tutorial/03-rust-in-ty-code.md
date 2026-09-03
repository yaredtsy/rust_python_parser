# 3. Rust patterns you will meet in ty

Chapter 2 was general Rust. This chapter is the specific patterns that fill
ty's source code. If these look strange, reading ty is painful. Once they look
normal, reading ty is easy.

---

## 3.1 The `'db` lifetime — what it means

You will see `'db` on almost every type in ty:

```rust
Type<'db>
Definition<'db>
SemanticModel<'db>
ProgramFile<'db>
```

A **lifetime** is a label. It says: *"this value is only valid while something
else is alive."*

`Type<'db>` means: *"this type is only valid while the database is alive."*

Why? Because a `Type` is not really the type. It is a **small number** that
points into the database. If the database goes away, the number means nothing.
Rust uses the lifetime to make sure that never happens.

```rust
fn bad<'db>(db: &'db ProjectDatabase) -> Type<'db> {
    // ...
}

let ty = {
    let db = make_database();
    bad(&db)                  // ERROR: db dies at the end of this block,
};                            // but `ty` would outlive it
```

The compiler catches this. In Python, this bug would be a crash at runtime, or
worse, silent wrong data.

### What this means for your design

**You cannot store `Type<'db>` in a long-lived struct.** So the shape of a
request handler is always:

```
1. borrow the database
2. do all the analysis  →  produces Type<'db>, Definition<'db>, etc.
3. convert to plain owned data  →  String, Vec, your own structs
4. release the borrow
5. send the JSON
```

Step 3 is important. Your `BaseNode` and `CallFrameStack` must hold `String`
and `u32`, not `Type<'db>`. The plan says this too — this is why.

### A rule of thumb

If you write a struct that holds anything from ty, that struct needs `'db` too:

```rust
struct Frame<'db> {                 // ← the lifetime spreads
    target: Definition<'db>,
    children: Vec<Frame<'db>>,
}
```

This is called "the lifetime is viral". It spreads through your code. That is
normal and fine. Just add `<'db>` and move on.

## 3.2 `&dyn Db` — the database handle

Nearly every ty function starts like this:

```rust
fn something(db: &dyn Db, ...) -> ...
```

`db` is your handle to everything: files, source text, parsed trees, types.
Think of it as Jedi's `inference_state`, but for the whole program instead of
one script.

There are several `Db` traits, stacked:

```rust
ruff_db::Db                      // files, source text, parsed modules
  └─ ty_python_core::Db          // + scopes, definitions
       └─ ty_python_semantic::Db // + types
            └─ ty_ide::Db        // + IDE features
```

Each one includes the one above it. So if you have a `&dyn ty_python_semantic::Db`,
you can also do everything `ruff_db::Db` can do.

**Good habit:** ask for the smallest one your function needs.

```rust
// good — works with any database that has types
fn my_analysis(db: &dyn ty_python_semantic::Db) { ... }

// worse — locks you to one concrete database
fn my_analysis(db: &ProjectDatabase) { ... }
```

## 3.3 `#[salsa::tracked]` — the caching macro

You will see this on top of functions:

```rust
#[salsa::tracked]
fn semantic_index<'db>(db: &'db dyn Db, file: PythonFile<'db>) -> SemanticIndex<'db> {
    // expensive work
}
```

`#[...]` is an **attribute**. It is a macro — it rewrites the function while
compiling.

What this one does: **the function is called once per set of arguments.** Call
it again with the same `db` and `file`, and you get the stored answer with no
work. Chapter 6 explains how.

You do not need to understand the macro's internals. You just need to know:

- A function with `#[salsa::tracked]` is **cheap to call again**.
- A function without it runs every time.

So calling `parsed_module(db, file)` a thousand times is fine. That surprises
people coming from Jedi, where re-parsing is expensive.

There are related attributes on types:

| Attribute | Meaning |
|---|---|
| `#[salsa::input]` | raw data you set from outside (file contents) |
| `#[salsa::tracked]` | computed and remembered |
| `#[salsa::interned]` | de-duplicated; equal values share one number |

`Type<'db>` is interned. That is why it is small and `Copy`, and why comparing
two types can be a fast number comparison.

## 3.4 `Box<T>` — a value stored on the heap

You saw this in the AST:

```rust
pub struct ExprCall {
    pub func: Box<Expr>,          // ← Box
    pub arguments: Arguments,
}
```

`Box<T>` means "this value lives on the heap, and here is a pointer to it".

Why is `func` boxed? Because `Expr` contains `ExprCall`, which contains an
`Expr`... forever. Rust needs to know how big a struct is. A `Box` is always
pointer-sized, so it breaks the loop.

To read through a `Box`, use `&*` or `.as_ref()`:

```rust
match call.func.as_ref() {
    Expr::Name(n) => { /* f(x) — calling a plain name */ }
    Expr::Attribute(a) => { /* obj.f(x) — calling a method */ }
    _ => {}
}
```

You will write `.as_ref()` on `call.func` constantly. Get used to it.

## 3.5 `SmallVec` and `ThinVec` — cheaper lists

Ruff cares a lot about memory. So instead of `Vec<T>` you often see:

```rust
pub decorator_list: thin_vec::ThinVec<Decorator>,
```

- **`Vec<T>`** — normal growable list. 3 words of memory (pointer, length,
  capacity).
- **`ThinVec<T>`** — same idea, but 1 word. Better when usually empty. Most
  functions have no decorators, so this saves a lot across a big project.
- **`SmallVec<[T; 2]>`** — stores up to 2 items *inside itself*, with no heap
  allocation. Goes to the heap only if it grows past 2.

For your code, `SmallVec` is the right choice for "usually one value, sometimes
a few". The plan suggests it for `Values<'db>` for exactly that reason: an
expression usually has one possible value.

You use all three the same way as `Vec`: `.iter()`, `.push()`, `.len()`.

## 3.6 The visitor pattern

To walk the AST, you implement a trait:

```rust
use ruff_python_ast::visitor::source_order::{
    SourceOrderVisitor, TraversalSignal, walk_expr,
};

struct FindCalls<'a> {
    found: Vec<&'a ast::ExprCall>,
}

impl<'a> SourceOrderVisitor<'a> for FindCalls<'a> {
    fn visit_expr(&mut self, expr: &'a ast::Expr) {
        if let ast::Expr::Call(call) = expr {
            self.found.push(call);
        }
        walk_expr(self, expr);        // ← keep going deeper
    }
}
```

Two things that trip people up:

**1. You must call `walk_expr` yourself.** If you forget it, the walk stops at
that node and never sees the children. There is no automatic recursion.

**2. `enter_node` controls whether to go deeper.** This is how you say
"do not walk into nested functions":

```rust
fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    match node {
        AnyNodeRef::StmtFunctionDef(_) => TraversalSignal::Skip,   // stop here
        _ => TraversalSignal::Traverse,                            // go deeper
    }
}
```

This replaces your `_scan_children` logic from `parser.py`. It is the same idea,
written as a trait instead of a nested function.

## 3.7 `AnyNodeRef` — one type for any node

Sometimes you want "any AST node, I do not care which". That is `AnyNodeRef`:

```rust
enum AnyNodeRef<'a> {
    StmtFunctionDef(&'a StmtFunctionDef),
    ExprCall(&'a ExprCall),
    // ... every node type
}
```

It is a borrow of any node. The visitor's `enter_node` uses it, because it must
handle every kind of node.

## 3.8 Two words that are not English

You will see these in ty's comments and names. They are jargon.

**"salsa ingredient"** — a value that salsa can use as a cache key. Not any
value works: it must be an input, tracked, or interned type. If you try to
cache on a plain `String`, salsa will refuse.

**"cycle recovery"** — what happens when a query asks for itself, directly or
through a chain. Instead of looping forever, salsa notices and returns a
fallback value. ty uses `Type::Divergent` as that fallback. The plan uses the
same trick for return-value queries.

## 3.9 A checklist for reading unfamiliar ty code

When you open a ty file and feel lost, look for these in order:

1. **What does the function take?** `db` plus what? That tells you the inputs.
2. **What does it return?** `Option<...>` means it can fail quietly.
3. **Is it `#[salsa::tracked]`?** If yes, it is cheap to call again.
4. **Find the `match`.** The `match` on `Type` or `Expr` is usually the heart of
   the function.
5. **Ignore the lifetimes.** `'db` is noise for understanding. Read
   `Type<'db>` as just `Type`.

That last one is the most useful tip in this chapter. Lifetimes matter when
*writing* code. When *reading* code, mentally delete them.

---

## Check yourself

1. Why can't you store a `Type<'db>` in a struct that lives forever?
2. What happens if you forget to call `walk_expr` inside `visit_expr`?
3. Why is `func` in `ExprCall` a `Box<Expr>` and not just `Expr`?
4. What does `#[salsa::tracked]` change about calling a function twice?

---

→ Next: [`04-positions-and-text.md`](04-positions-and-text.md)
