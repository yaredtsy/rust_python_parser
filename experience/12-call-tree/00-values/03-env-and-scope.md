# 00.03 — `Env`: the thing Jedi calls `parent_context`

The environment is the single idea this whole project hangs off:
*resolve from what you know first, fall back to ty second.* This file is the
mechanism; [`01-functions/01-calling-a-plain-function.md`](../01-functions/01-calling-a-plain-function.md)
is the first place you'll watch it run end to end.

**Reading:** [`plan/03-call-tree/03-the-abstract-interpreter.md`](../../../plan/03-call-tree/03-the-abstract-interpreter.md)
§"The one-sentence design", and
[`plan/03-call-tree/05-binding-arguments.md`](../../../plan/03-call-tree/05-binding-arguments.md)
§"The environment".

---

## What `as_context(arguments)` gave Jedi, restated

```python
def outer():
    w = JsonWriter()
    middle(w)

def middle(x):
    inner(x)          # x → evaluated in outer's context → JsonWriter instance

def inner(y):
    y.write(1)        # y → middle's context → x → outer's context → JsonWriter
```

Jedi resolves this by **lazily** walking a chain of contexts, each one
pointing back to the context that called it. Your Rust version does the
same job with an ordinary, **eagerly** populated map:

```rust
#[derive(Clone)]
pub struct Env<'db> {
    locals: rpds::HashTrieMap<Name, Values<'db>>,
    attrs:  rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    scope:  ScopeId<'db>,
    parent: Option<Arc<Env<'db>>>,
}
```

| field | answers |
|---|---|
| `locals` | "what is parameter/local `x` bound to, in this activation?" |
| `attrs` | "what is `origin.attr` bound to?" — shared across the whole path, not per-frame; see `02-classes/03` |
| `scope` | "am I even allowed to see `x` here?" — the guard below |
| `parent` | "if `x` isn't local, is it a free variable from an enclosing *function*?" — closures, `01-functions/04` |

## Why a persistent map, not `FxHashMap` + `.clone()`

You mint one `Env` **per call site** — every `resolve_call` in
`01-functions/01` ends with `bind_parameters` producing a new one. If `Env`
were a `HashMap` you own outright, cloning it for every call would be
O(n) allocation, and at realistic call volume that dominates your profile
before anything else does.

`rpds::HashTrieMap` (or `im`'s equivalent) gives you **structural sharing**:
cloning is a pointer copy, and inserting one new key produces a new map that
shares every untouched branch with the old one. This is not a performance
nicety you can add later — it is the difference between the port being fast
and being an embarrassment, because this is the single most-executed
operation in the whole interpreter.

## Why `scope` is a field, not implied by `locals`

```python
def f(x):
    g()

def g():
    print(x)     # NameError at runtime — g has no x
```

Two unrelated functions can each have a parameter named `x`. Without a
`scope` field tying `locals` to "the function this environment actually
belongs to", a lookup of `x` while walking `g`'s body could accidentally
return *`f`'s* `x`, if the two environments ever got confused with each
other (e.g. through a bug in how frames are threaded through recursion).
`plan/03-call-tree/05` calls this out explicitly: *"this is the bug you will
hit around week two if you skip it."* A lookup must check the scope
matches before trusting a hit — or, for genuine closures, walk `parent`
specifically because the name is a *free variable of a nested function*,
never as a generic "search outward" fallback.

## The two-line rule, now concrete

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>) -> Values<'db> {
    self.resolve_from_env(expr, env)                    // precise, path-aware
        .unwrap_or_else(|| self.resolve_from_ty(expr))   // fast, path-free
}
```

Every lookup in this design is this shape: check `env` first (because `env`
is where path-specific information — "this call bound `w` to *this*
`JsonWriter`" — actually lives), and only ask `ty` when `env` has nothing to
say. `04-annotation-fallback.md` inserts a third tier into exactly this
chain.

## `lift` — converting a `ty::Type` back into your domain

The moment `resolve_from_ty` *does* get asked, its `Type<'db>` answer has to
become `Values<'db>` before anything downstream can use it:

```rust
fn lift(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>) -> Values<'db> {
    match ty {
        Type::FunctionLiteral(f) => smallvec![AbstractValue::Function(f)],
        Type::ClassLiteral(c)    => smallvec![AbstractValue::Class(c)],
        Type::ModuleLiteral(m)   => smallvec![AbstractValue::Module(m.module(db))],

        // ★ the important case: EXPLODE, never re-flatten
        Type::Union(u) => u.elements(db).iter().flat_map(|t| lift(db, env, *t)).collect(),

        Type::NominalInstance(i) => smallvec![AbstractValue::Instance {
            class: i.class(db, env).class_literal(db),
            origin: OriginId::OPAQUE,   // ty gave us this; we didn't construct it.
                                        // Shared by ALL opaque instances → never holds attrs.
        }],

        Type::Dynamic(_) | Type::Never => smallvec![AbstractValue::Unknown],
        other => smallvec![AbstractValue::Ty(other)],
    }
}
```

The `Union` arm is the one line in the whole project most likely to get
"simplified" back into a bug. `ty` hands you a set; your job is to turn that
set into **separate, enumerable values**, each free to produce its own
subtree later. The moment you flatten it back into one `Type::Union`, you've
quietly rebuilt `ty_ide::outgoing_calls` and lost the entire point
(`plan/03-call-tree/02`, `plan/03-call-tree/04`).

`lift` doesn't cap the union itself. Its caller passes the result through
`cap_fanout`, the one place the cap lives: if there are more values than
`limits.max_union_fanout`, it first drops the ones that can't produce a tree
node (stdlib, site-packages, `Ty(..)`), and only gives up with `Unknown` if
the project values alone are still too many. So `A | B | C | D | E | F` with
only `A` and `B` from the project keeps `A` and `B`.
[`06-scaling-budget-and-memoization.md`](06-scaling-budget-and-memoization.md)
covers why, what number to start with, and how to change or turn off the cap.

## Exercise

Trace `uses_all_five` from [`01-abstractvalue.md`](01-abstractvalue.md) by
hand, as if you were the interpreter:

1. `env` is empty (module scope, nothing bound yet).
2. `a = plain` — is `plain` found in `env`, or does this go through `lift`?
   Which arm of `lift` fires?
3. `c = Thing()` — walk through `eval_call_result`'s three cases
   (`plan/03-call-tree/10`, case (a)): which one applies, and does it touch
   `lift` at all?
4. After this function returns, is anything added to `env.attrs`? Why or
   why not?

<details><summary>Answers</summary>

2. `plain` is a module-level name, not yet in any `locals` map at the point
   this function starts — so `resolve_from_env` misses and `resolve_from_ty`
   runs, giving `Type::FunctionLiteral(plain)`, and `lift`'s first arm fires.
3. Case (a), the constructor case — `Thing` resolves to `Class`, so a *fresh*
   `OriginId` is minted directly; `lift` is never consulted, because this
   path never asks ty what `Thing()` returns, it constructs the instance
   itself. (This is the point of "be eager" — see
   [`01-functions/05-returns-and-value-flow.md`](../01-functions/05-returns-and-value-flow.md).)
4. Nothing — `uses_all_five` never assigns through an attribute
   (`something.x = ...`), so `attrs` is untouched. `attrs` only grows when a
   statement assigns to an `Expr::Attribute` target, which is
   `02-classes/03`'s subject.

</details>

---

→ Next: [`04-annotation-fallback.md`](04-annotation-fallback.md)
