# 2. Rust refresher

Only the parts you need to read ty's code. Every example is close to real ty
code, so you learn Rust and ty at the same time.

If you know a section already, skip it.

---

## 2.1 Enums — the most important thing

In Python, a value can be anything. In Rust, you must say what it can be.

An **enum** is a type that can be *one of several shapes*. This is not like a
Python `Enum` (which is just named numbers). A Rust enum can carry data.

```rust
enum Shape {
    Circle(f64),           // a circle, holding its radius
    Rect(f64, f64),        // a rectangle, holding width and height
    Point,                 // a point, holding nothing
}
```

A `Shape` is **exactly one** of those three. Not two. Not none.

Ruff's `Expr` type is exactly this idea, just bigger. Here is the real one
(shortened):

```rust
pub enum Expr {
    Call(ExprCall),                 // f(x)
    Name(ExprName),                 // x
    Attribute(ExprAttribute),       // a.b
    Lambda(ExprLambda),             // lambda x: x
    NumberLiteral(ExprNumberLiteral),
    // ... 34 variants in total
}
```

So a Python expression in Ruff is *one of 34 shapes*. Each shape holds its own
struct with its own fields.

> **Compare to parso:** in parso, every node is the same Python object with a
> `.type` string and a `.children` list. You check `node.type == "atom_expr"`.
>
> In Ruff, the shape *is* the type. You cannot ask for `.children` on an
> `ExprName`, because `ExprName` does not have children. The compiler stops you.
> This catches many bugs before you run anything.

## 2.2 `match` — how you read an enum

To use an enum, you must handle its shapes. That is `match`:

```rust
fn describe(expr: &Expr) -> &str {
    match expr {
        Expr::Call(_)      => "a function call",
        Expr::Name(_)      => "a variable name",
        Expr::Attribute(_) => "an attribute like a.b",
        _                  => "something else",
    }
}
```

Three things to notice:

- `Expr::Call` — you write the enum name, then `::`, then the shape name.
- `(_)` — the underscore means "there is data here, but I do not want it".
- `_ =>` at the end means "everything else". Without it, the compiler will
  complain that you forgot some shapes. **This is a feature.** When Ruff adds a
  new expression type, your code stops compiling and you go fix it.

To *use* the data inside, give it a name instead of `_`:

```rust
fn callee_of(expr: &Expr) -> Option<&Expr> {
    match expr {
        Expr::Call(call) => Some(&call.func),   // `call` is the ExprCall struct
        _ => None,
    }
}
```

Now `call` is the `ExprCall` value, and `call.func` is the thing being called.

## 2.3 `if let` — match for one shape only

When you only care about one shape, `match` is noisy. Use `if let`:

```rust
// long way
match expr {
    Expr::Name(name) => println!("name is {}", name.id),
    _ => {}
}

// short way — same thing
if let Expr::Name(name) = expr {
    println!("name is {}", name.id);
}
```

And there is `let ... else`, which you will see a lot in ty. It means
"take this shape, or leave the function now":

```rust
fn print_name(expr: &Expr) {
    let Expr::Name(name) = expr else {
        return;                      // not a Name? stop here.
    };
    // from here down, `name` is definitely an ExprName
    println!("{}", name.id);
}
```

This keeps code flat instead of deeply nested. ty uses it everywhere.

## 2.4 `Option` — Rust has no `None` by accident

Python lets any variable be `None`. Rust does not. If a value might be missing,
you must say so with `Option`:

```rust
enum Option<T> {      // this is built in; you do not write it
    Some(T),          // there is a value
    None,             // there is no value
}
```

`T` means "any type". `Option<String>` is "maybe a String".

```rust
fn find_name(expr: &Expr) -> Option<&Name> {
    match expr {
        Expr::Name(n) => Some(&n.id),
        _ => None,
    }
}
```

To use it, you must handle both cases. The short way is `?`:

```rust
fn name_length(expr: &Expr) -> Option<usize> {
    let name = find_name(expr)?;      // if None, return None right now
    Some(name.len())                  // if Some, `name` is the value
}
```

The `?` is very common in ty. Read it as: *"give me the value, or give up and
return `None` from this function."*

> **Why this matters for you:** ty returns `Option` almost everywhere.
> `inferred_type()` returns `Option<Type>`. A missing answer is normal, not an
> error. Your code must handle "ty does not know" as a common case, not a bug.

## 2.5 `Result` — for things that can fail with a reason

`Option` says "value or nothing". `Result` says "value or an error":

```rust
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

`?` works here too. If it is `Err`, the function returns that error.

```rust
fn read_and_parse(path: &Path) -> Result<Module, Error> {
    let text = std::fs::read_to_string(path)?;   // failed? return the error
    let module = parse(&text)?;
    Ok(module)
}
```

Use `Option` when "nothing" is normal. Use `Result` when you want to explain
what went wrong.

## 2.6 Ownership and borrowing — the short version

This is the part of Rust that is different from every other language. Here is
the minimum.

Every value has **one owner**. When the owner goes away, the value is freed.
There is no garbage collector.

```rust
let s = String::from("hello");   // `s` owns the string
let t = s;                       // now `t` owns it. `s` is dead.
println!("{}", s);               // ERROR: s was moved
```

That is annoying, so most of the time you **borrow** instead. A borrow is a
reference. You write `&`:

```rust
let s = String::from("hello");
let t = &s;                      // `t` borrows. `s` still owns.
println!("{} {}", s, t);         // fine, both work
```

Two rules for borrows:

1. You can have **many** read-only borrows (`&T`) at the same time.
2. Or **one** writable borrow (`&mut T`). But not both at once.

```rust
let mut v = vec![1, 2, 3];
let a = &v;          // read borrow
let b = &v;          // another read borrow — fine
// let c = &mut v;   // ERROR: cannot write while reads exist
```

**Why this matters in ty:** you will see `&dyn Db` everywhere (a read borrow of
the database) and `&mut db` in a few places (a write borrow). Taking `&mut db`
tells salsa that files changed. It also **cancels** every read happening on
other threads. That is by design, and chapter 6 explains it.

## 2.7 Structs and `impl`

A struct is a group of fields, like a Python class with no methods:

```rust
pub struct ExprName {
    pub range: TextRange,
    pub id: Name,
    pub ctx: ExprContext,
}
```

Methods go in a separate `impl` block:

```rust
impl ExprName {
    fn is_dunder(&self) -> bool {
        self.id.starts_with("__")
    }
}
```

`&self` is like Python's `self`, but borrowed. Three forms:

| Form | Meaning | Python feel |
|---|---|---|
| `&self` | read the struct | normal method |
| `&mut self` | change the struct | method that sets fields |
| `self` | consume the struct | method that destroys the object |

## 2.8 Traits — like an interface, or a Python protocol

A **trait** is a set of methods that a type promises to have.

```rust
trait Ranged {
    fn range(&self) -> TextRange;
}
```

Any type can promise to have it:

```rust
impl Ranged for ExprName {
    fn range(&self) -> TextRange {
        self.range
    }
}
```

Now `ExprName` has a `.range()` method. This is real — `Ranged` is a real Ruff
trait, and nearly every AST node implements it.

> **A trap that will catch you once:** if you write `node.range()` and Rust says
> "no method named range", you probably forgot to import the trait:
>
> ```rust
> use ruff_text_size::Ranged;   // ← without this, .range() does not exist
> ```
>
> In Rust, a trait's methods only exist if the trait is imported. This confuses
> everyone at first.

## 2.9 `dyn Trait` — a value chosen at runtime

Sometimes you want "any type that has this trait", decided while running:

```rust
fn analyse(db: &dyn Db) { ... }
```

`&dyn Db` means "a borrow of *something* that implements `Db`". You do not know
which type. This is like passing an object in Python and calling a method on it.

ty uses `&dyn Db` in almost every function. That is how your code can work with
`ProjectDatabase` without depending on it directly.

## 2.10 `Copy` and `Clone`

- **`Clone`** = "you can make a copy, but say so": `x.clone()`.
- **`Copy`** = "copying is so cheap it happens automatically".

Numbers are `Copy`. `String` is `Clone` but not `Copy` (copying allocates
memory, so Rust makes you ask for it).

```rust
let a = 5;
let b = a;          // copied. `a` still works.

let s = String::from("hi");
let t = s.clone();  // must say .clone()
```

**Why this matters:** ty's `Type<'db>` is `Copy`. It is a small handle — just a
number pointing into the database. You can pass it around freely with no cost.
The plan's chapter on the value domain says your `AbstractValue` should be
`Copy` too, for the same reason.

## 2.11 Iterators

Rust's iterators look like Python generators, but they chain with methods:

```rust
// Python
names = [c.name for c in classes if c.is_public]

// Rust
let names: Vec<_> = classes
    .iter()                          // walk the list
    .filter(|c| c.is_public)         // keep some
    .map(|c| c.name.clone())         // change each one
    .collect();                      // build a Vec at the end
```

Nothing happens until `.collect()`. Before that, it is just a plan.

Two you will see often in ty:

```rust
.filter_map(|x| some_option_returning_fn(x))   // filter + map together;
                                               // None values are dropped
.flat_map(|x| returns_an_iterator(x))          // map, then flatten the result
```

`filter_map` is very common because so much of ty returns `Option`.

## 2.12 Reading a real ty function

Now put it together. This is close to real ty code:

```rust
fn callee_definition<'db>(
    model: &SemanticModel<'db>,
    call: &ast::ExprCall,
) -> Option<Definition<'db>> {
    let ty = call.func.inferred_type(model)?;
    match ty {
        Type::FunctionLiteral(f) => Some(f.definition(model.db())),
        Type::ClassLiteral(c)    => Some(c.definition(model.db())),
        _ => None,
    }
}
```

Line by line:

| Line | What it says |
|---|---|
| `fn callee_definition<'db>(` | a function; `'db` is a lifetime (chapter 3) |
| `model: &SemanticModel<'db>` | borrow a semantic model |
| `call: &ast::ExprCall` | borrow a call node |
| `-> Option<Definition<'db>>` | returns a definition, or nothing |
| `call.func.inferred_type(model)?` | ask ty for the type; give up if unknown |
| `match ty {` | look at which shape the type is |
| `Type::FunctionLiteral(f) =>` | it is a function; `f` is the function |
| `_ => None` | any other shape: we have no answer |

If you can read that, you can read most of ty.

---

## Check yourself

1. What does `?` do at the end of an expression?
2. Why does `match` complain if you forget a shape?
3. Why might `node.range()` fail to compile even though the method exists?
4. What is the difference between `&self` and `&mut self`?

---

→ Next: [`03-rust-in-ty-code.md`](03-rust-in-ty-code.md)
