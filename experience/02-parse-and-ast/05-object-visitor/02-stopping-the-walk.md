# Object 5 · Part 2 — Stopping the walk

`TraversalSignal::Skip`, the root guard, `AnyNodeRef`, and the four parity
behaviours you must carry over from `parser.py`.

**Part 2 of 3** · prev: [Part 1 — How the walk works](01-how-the-walk-works.md) ·
next: [Part 3 — Examples, exercises, exam](03-examples-and-exam.md)

Part 1 established the three layers: `visit_X` (your override) → `walk_X`
(where `enter_node`/`leave_node` live) → `X::visit_source_order` (the generated
child list). Everything below hangs off `enter_node`, so that is the layer to
keep in mind.

---

## `TraversalSignal::Skip` — the rule from `parser.py`

Your `_scan_children` **stops** at a nested def or class (`parser.py:78-80`):
calls inside a nested function belong to *that* function, not the enclosing one.

`enter_node` expresses this directly:

```rust
fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    match node {
        AnyNodeRef::StmtFunctionDef(_) | AnyNodeRef::StmtClassDef(_) => {
            // emit a node for it, recurse separately with a FRESH visitor,
            // and do not descend here
            TraversalSignal::Skip
        }
        _ => TraversalSignal::Traverse,
    }
}
```

That gives you the nesting your wire format wants: one visitor per scope, each
producing its own children.

Note what `Skip` on a `StmtFunctionDef` suppresses — the *whole* of layer 2 for
that node, which is decorators, name identifier, type params, parameters (and
therefore default-argument expressions), return annotation, and body. All of it,
in one signal. That is exactly parso's early `return`.

### ⚠ But `enter_node` fires for your own root too

If you start a visitor at a `StmtFunctionDef` and `enter_node` skips
`StmtFunctionDef`, you skip the very thing you were asked to walk — and get
nothing. Not an error: an empty scope, for every function in the file.

So the visitor needs to know its own root:

```rust
struct ScopeScanner<'a> {
    root: TextRange,              // the range of the scope we started at
    out: Vec<Node>,
}

fn enter_node(&mut self, node: AnyNodeRef<'a>) -> TraversalSignal {
    if node.range() == self.root {
        return TraversalSignal::Traverse;      // this is me; go in
    }
    match node { … }
}
```

### Why *range* equality and not the other two options

`AnyNodeRef` gives you three ways to ask "is this my root?", and they are not
interchangeable:

| comparison | what it does | cost | verdict |
|---|---|---|---|
| `node == self.root` | derived `PartialEq` — **deep structural** equality of the pointed-to nodes | O(subtree) | ✗ wrong *and* slow |
| `node.ptr_eq(self.root)` | pointer + kind equality (`node.rs:584`) | O(1) | ✓ correct, needs an `AnyNodeRef` root |
| `node.range() == self.root` | two `u32` compares on `Copy` data | O(1) | ✓ what to use |

**[verified]** `generated.rs:5829` — `AnyNodeRef` derives `PartialEq`, so `==`
compares *values*, recursively, and two structurally identical sibling
functions would compare equal. `node.rs:584` — `ptr_eq` is the referential
comparison, and it exists precisely because `==` is not one.

`plan/02-mapping/01` sketches `node == self.root`. That is the bad row of the
table; it is not merely "check what it does at your revision" — at `ac201b8` it
is a deep compare that can return `true` for the wrong node. Use ranges (or
`ptr_eq`). A range is also cheaper to store and trivially `Copy`, which is why
`ScopeScanner` holds a `TextRange` rather than an `AnyNodeRef`.

Ranges are unique within a file in practice, with one caveat worth checking in
exercise G: a node and its sole child can share a range in some ASTs. In ruff
they generally do not — `Stmt::Expr` and its inner `Expr` do — so the guard is
"first node whose range matches", and since the root is entered first, it wins.

---

## `AnyNodeRef` — one type for any node

```rust
// generated.rs:5828   [verified]
/// A flattened enumeration of all AST nodes.
#[derive(Copy, Clone, Debug, is_macro::Is, PartialEq)]
pub enum AnyNodeRef<'a> {
    StmtFunctionDef(&'a StmtFunctionDef),
    StmtClassDef(&'a StmtClassDef),
    ExprCall(&'a ExprCall),
    Identifier(&'a Identifier),
    Decorator(&'a Decorator),
    …    // 94 variants
}
```

- **"Flattened"** is the key word: there is no `AnyNodeRef::Stmt(..)` wrapping a
  `StmtFunctionDef`. `AnyNodeRef::from(&Stmt)` matches the inner variant out.
  That is why `enter_node` can match `AnyNodeRef::StmtFunctionDef(_)` directly.
- **`Copy`** — it is one tag plus one pointer. Pass it by value; the trait
  signature does.
- **`is_macro::Is`** generates `node.is_stmt_function_def()`,
  `node.as_expr_call()` and friends, so you can write predicates without a
  `match`.
- It implements `Ranged`, so `node.range()` always works.
- 94 variants is the reason `enter_node` takes it rather than `&Stmt`:
  `enter_node` is called for decorators, parameters, keywords, comprehensions,
  identifiers and f-string elements too, and `&Stmt` could represent none of
  them.

You will mostly `match` two or three variants and `_ => Traverse` the rest.

---

## The four behaviours you must carry over

From `plan/00-orientation/01`:

### Quirk 8 — lambdas are dropped, **with their subtree**

```rust
AnyNodeRef::ExprLambda(_) => TraversalSignal::Skip,
```

`parser.py:121` returns `None` for `lambdef`, and because parso's `Lambda` is a
subclass of `Function`, line 80's early `return` drops **the whole subtree**. So
`log(x)` inside `lambda x: log(x)` must not appear anywhere in your output.

Emit nothing, and `Skip`. Not "emit nothing and keep walking" — that would find
the inner call, because `ExprLambda::visit_source_order` visits `parameters`
then `body` (`generated.rs:10644` **[verified]**), and the call is in the body.

Note the asymmetry with a nested `def`: a lambda is `Skip`ped **and not
recorded**, a `def` is `Skip`ped **and pushed to `defs`** for a separate walk.
Same signal, opposite bookkeeping.

### Quirk 10 — position dedup

```rust
struct ScopeScanner<'a> {
    seen: FxHashSet<TextRange>,
    …
}
// before emitting:
if !self.seen.insert(range) { return; }
```

`_scan_children` drops nodes sharing an identical 4-tuple position. In Rust that
is one 8-byte hash key instead of a tuple of four boxed integers.

### Decorators — skip them (object 3's trap)

`decorator_list` is the **first** thing `StmtFunctionDef::visit_source_order`
visits, so the default walk enters it before anything else. Override
`visit_decorator` to do nothing:

```rust
fn visit_decorator(&mut self, _d: &'a Decorator) {
    // deliberately empty: parso puts decorators OUTSIDE the funcdef,
    // so calls inside them belong to the enclosing scope.
    // PARITY: plan/02-mapping/01, experience/02 object 3.
}
```

Note what this does *not* do: it does not call `walk_decorator`. That is the
"forgetting to walk" bug — used deliberately, which is the only time it is
correct. It also means `enter_node` never fires for the `Decorator` node or
anything under it, since `enter_node` lives inside `walk_decorator` (layer 1).
That is the intended effect, and it is a second reason the empty body needs the
`// PARITY:` comment: it silences two hooks, not one.

And recall object 3's other half: `StmtFunctionDef.range` **includes** the
decorators (the parser takes `start` before parsing them,
`parser/statement.rs:2895` + `:3021` **[verified]**). So skipping
`visit_decorator` fixes the *children*, not the *range* — you still owe the
range adjustment from object 3.

### Default arguments — they stay with the function

`edges.py`'s `default_args` asks the question explicitly:

```python
def default_args(x=build(), *, y=log()):
    return x, y
```

`parameters` is a field of `StmtFunctionDef`, visited by layer 2 *between* the
name and the body, and `ParameterWithDefault::visit_source_order` visits
`parameter` then `default` (`node.rs:373` **[verified]**). So when you scan
`default_args` **as a root**, `build()` and `log()` are its children — and when
you scan its parent, the `Skip` at the def hides them. Both calls land on
`default_args`.

That matches parso, where defaults are inside the funcdef node too — so unlike
decorators, this one needs **no** special case. It is on the list only because
it looks like it ought to need one.

---

## The recursive driver, and `walk_node`

`Skip` leaves you with one visitor per scope, so you need a way to start a walk
at an arbitrary node — not a `Stmt`, not an `Expr`, but whatever you stashed in
`defs`. That is exactly `walk_node`:

```rust
// source_order.rs:222   [verified]
pub fn walk_node<'a, V>(visitor: &mut V, node: AnyNodeRef<'a>)
where V: SourceOrderVisitor<'a> + ?Sized,
{
    if visitor.enter_node(node).is_traverse() {
        node.visit_source_order(visitor);
    }
    visitor.leave_node(node);
}
```

So `defs: Vec<AnyNodeRef<'a>>` is the right storage type, and the driver is:

```rust
fn scan<'a>(root: AnyNodeRef<'a>, source: &str) -> Vec<Node> {
    let mut scanner = ScopeScanner {
        root: root.range(),
        seen: FxHashSet::default(),
        defs: Vec::new(),
        calls: Vec::new(),
    };
    walk_node(&mut scanner, root);          // ★ the entry point

    let mut out = emit_calls(&scanner.calls, source);
    for def in scanner.defs {
        out.push(emit_def(def, scan(def, source)));   // fresh scanner, one level down
    }
    out
}
```

For the module itself, `walk_body(&mut scanner, &parsed.syntax().body)` is the
right entry: a module body is not a node (see the hooks table in
[part 1](01-how-the-walk-works.md)), and `root` can be the module's full range
so the guard never matches anything real.

**One visitor per scope.** That is the design, and it falls out of `Skip`.

---

---

**Next:** [Part 3 — Examples, exercises, exam](03-examples-and-exam.md), where
you write both visitors and prove each quirk on a fixture.
