# 11. Reading the source without getting lost

54 crates is a lot. This chapter is a map, plus the habits that make a big Rust
codebase readable.

---

## The order to read things

Do not browse randomly. Read these five files, in this order. That is enough to
work.

### 1. `ty_ide/src/call_hierarchy/outgoing_calls.rs` (797 lines)

**Read this first.** It is the closest existing thing to your problem, and you
will copy most of its structure.

Look for:

- `pub fn outgoing_calls(...)` — the entry point. See the two-line prologue
  from chapter 6.
- `struct OutgoingCallsFinder` — the visitor.
- `fn record_callee(...)` — **this is the function you will replace.** Everything
  else you keep.
- `fn walk_callable_signature(...)` — handles decorators, defaults, annotations,
  return types. Places where calls hide that you would forget.

### 2. `ty_ide/src/call_hierarchy.rs` (416 lines)

- `CallHierarchyItem::from_definition` — turning a `Definition` into a name,
  kind, and range. You need the same.
- The header comment explains why these entry points are *not*
  `#[salsa::tracked]`. Chapter 6 quoted it. Follow that decision.

### 3. `ty_python_semantic/src/semantic_model.rs`

- `pub struct SemanticModel` and its methods.
- `trait HasType` and `trait HasDefinition` at the bottom.
- The macro-generated `impl HasType for $ty` blocks — this tells you which node
  types you can call `.inferred_type()` on.

### 4. `ty_python_semantic/src/types/ide_support.rs` (2000+ lines)

This is the "public bridge" module — the place where ty exposes internals to
IDE features. If you build inside the ruff workspace, this is also the natural
place to add your own helper functions.

Useful ones: `definitions_for_name`, `definitions_for_attribute`,
`static_member_type_for_attribute`, `resolved_call_signature`.

### 5. `ty_python_semantic/src/types.rs` — just the `Type` enum

Do not read all of it; it is enormous. Read the `pub enum Type<'db>` block and
its comments. That is your vocabulary for everything else.

---

## Habits for reading Rust code

### Habit 1: read the signature, skip the body

```rust
pub fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize)
    -> Vec<OutgoingCall>
```

Before reading one line of the body, you already know:

- it needs a database and a file (so it is doing whole-program work)
- it takes an offset (so it is cursor-based, like an editor feature)
- it returns a list (so there can be zero or many results)

Half of understanding a function is in its signature.

### Habit 2: mentally delete lifetimes

```rust
fn definitions_for_name<'db>(model: &SemanticModel<'db>, name: &ast::ExprName)
    -> Vec<ResolvedDefinition<'db>>
```

Read it as:

```
fn definitions_for_name(model, name) -> Vec<ResolvedDefinition>
```

Lifetimes matter when you *write* code. When you *read* code, they are noise.

### Habit 3: find the `match`

Most ty functions have one `match` that is the real logic, surrounded by
setup and cleanup. Find it and read that first.

### Habit 4: check for `#[salsa::tracked]`

If a function has it, calling it repeatedly is cheap. That changes how you use
it. Look at the attribute before you plan around performance.

### Habit 5: read the tests

Ruff's tests often use `insta` snapshots. Find a `snapshots/` folder next to a
module and read the `.snap` files — they show real input and real output. This
is often faster than reading the implementation.

---

## Searching effectively

```bash
cd /Users/yared/Documents/Programing/ruff

# where is a type defined?
grep -rn "pub enum Type<'db>" crates/

# what is public in a crate?
grep -n "^pub use\|^pub fn\|^pub struct\|^pub enum" crates/ty_ide/src/lib.rs

# is this function public or private?  ← ask this constantly
grep -n "fn member_lookup_with_policy" crates/ty_python_semantic/src/types.rs

# who calls this?
grep -rn "definitions_for_attribute" crates/
```

That third one matters more than it looks. In `types.rs` there are 10 `pub fn`
and 17 `pub(crate) fn`. **Before planning to use a method, check whether you can
actually reach it.** `pub(crate)` means "only inside this crate" — you cannot
call it from your own crate unless you build inside the ruff workspace.

---

## Visibility, quickly

| Written | Who can call it |
|---|---|
| `pub fn` | anyone |
| `pub(crate) fn` | only code inside the same crate |
| `pub(super) fn` | only the parent module |
| `fn` | only the same module |

And for modules:

```rust
pub mod types;              // outsiders can reach types::...
mod infer;                  // private; only this crate
```

`ty_python_semantic` has `pub mod types;` and inside it `pub mod ide_support;`.
That is why `ty_python_semantic::types::ide_support::resolved_call_signature`
works, even though it is not re-exported at the crate root.

---

## What to ignore

You can safely never open these:

- `ruff_linter` — a thousand lint rules. Nothing for you.
- `ruff_python_formatter`, `ruff_formatter` — code formatting.
- `ruff_server`, `ruff_wasm`, `ty_wasm` — other front ends.
- `ruff_dev`, `ruff_benchmark`, `ruff_macros` — build tooling.
- **`ruff_python_semantic`** — the linter's binding table. **Not types.** See
  chapter 1.

That removes about 40 of the 54 crates.

---

## When the compiler shouts at you

Rust error messages are long but usually correct. Common ones:

**"no method named `range` found"**
→ You forgot `use ruff_text_size::Ranged;`. (Chapter 2.8.)

**"cannot borrow `db` as mutable because it is also borrowed as immutable"**
→ Something is still holding a read borrow. Usually you need to end an earlier
borrow first — put it in its own `{ }` block.

**"expected `Db`, found `Db`"** — two identical-looking types
→ You have two versions of `salsa` in your dependency tree. Run
`cargo tree -d -p salsa` to find the duplicate. (See
[`plan/04-build/01`](../plan/04-build/01-wiring-cargo.md).)

**"lifetime may not live long enough"**
→ You are trying to keep a `Type<'db>` past the database borrow. Convert to
owned data first. (Chapter 3.1.)

**"non-exhaustive patterns: `Expr::Foo` not covered"**
→ Ruff added a new AST node. Add the arm, or add `_ => {}`.

---

## Building and checking

```bash
cargo check          # fast: type-check only, no binary. Use this constantly.
cargo build          # slower: makes a binary
cargo test           # run tests
cargo tree -d        # find duplicate dependencies
```

Use `cargo check` in your edit loop. It is several times faster than
`cargo build` and catches almost everything.

Expect a **cold build of 5–10 minutes** the first time (54 crates plus a bundled
typeshed). After that, changing only your own crate takes 10–30 seconds, and
`cargo check` about 5.

---

## Where to go next

You have finished the tutorial. Now the plan will make sense.

| Read this | For |
|---|---|
| [`plan/README.md`](../plan/README.md) | the map of the whole project |
| [`plan/03-call-tree/`](../plan/03-call-tree/) | the real work — 10 chapters |
| [`plan/04-build/00-dev-cli.md`](../plan/04-build/00-dev-cli.md) | build this first, so you can iterate |
| [`plan/05-reference/api-cheatsheet.md`](../plan/05-reference/api-cheatsheet.md) | copy-paste API reference |
| [`plan/05-reference/glossary.md`](../plan/05-reference/glossary.md) | Jedi word ↔ ty word |

Suggested first day of actual work:

1. Get `cargo check` to pass with ty as a dependency.
2. Print the resolved Python version for your project
   ([`plan/04-build/01`](../plan/04-build/01-wiring-cargo.md) has the ten-line
   program).
3. Parse one file and print every function name with its line and column.

That third step uses chapters 4, 5, 6, and 7 together. If you can do it, you
understand enough to start.

---

## Check yourself

1. Which file should you read first, and which function in it will you replace?
2. What does `pub(crate)` mean for you as an outside user?
3. What does the error "expected `Db`, found `Db`" usually mean?
4. Which command should you use in your edit loop, and why?

---

That is the end of the tutorial. Go to [`plan/README.md`](../plan/README.md).
