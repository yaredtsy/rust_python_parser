# 00.03 — `Env`: the thing Jedi calls `parent_context`

(In `call_resolver.py`, `parent_context` is the context the current call is
resolved *in* — the activation of the function whose body is being walked.
That's the whole `Env`. It is not a link to a parent env; the closest thing
`Env` has to one is `enclosing`, below, and that means something else.)

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

Jedi resolves this **lazily**: when `inner` needs `y`, it goes back to the
caller's context to evaluate the argument expression `x`, and from there
back to `outer`'s. Your Rust version does the same job **eagerly**:
`bind_parameters` evaluates each argument in the caller's env *at the call
site* and copies the resulting values into the callee's `locals`. By the
time `inner`'s body runs, `y` is already `Instance{JsonWriter}` — nothing
needs to walk back to the caller, so `Env` has no link to it:

```rust
#[derive(Clone)]
pub struct Env<'db> {
    locals:    rpds::HashTrieMap<Name, Values<'db>>,
    attrs:     rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    scope:     ScopeId<'db>,
    enclosing: Option<ActivationId>,    // the run where this function was DEFINED — never the caller
}
```

| field | answers |
|---|---|
| `locals` | "what is parameter/local `x` bound to, in this activation?" |
| `attrs` | "what is `origin.attr` bound to?" — shared across the whole path, not per-frame; see `02-classes/03` |
| `scope` | "am I even allowed to see `x` here?" — the guard below |
| `enclosing` | "if `x` isn't local, is it a free variable of a nested `def`?" — the env of the activation where that `def` ran. Closures only, `01-functions/04` |

⚠ **`enclosing` is not the caller.** The `outer → middle → inner` chain
above is a *call* chain, and none of those functions is nested in another,
so all three have `enclosing: None`. The caller relationship lives in the
call tree (`Frame.parent`, `05-frame-pathkey-and-budget.md`), and name
lookup never uses it. Two separate links, two separate names:

| link | follows | stored in | used for |
|---|---|---|---|
| enclosing | where the function was **defined** | `Env.enclosing` | free variables in closures |
| caller | where the function was **called from** | `Frame.parent` | the tree, `is_ancestor` |

A nested function gets its `enclosing` from its *value*, not from whoever
calls it: the `def` statement binds the name to
`Closure { func, activation }`, where `activation` is the run of the
enclosing function the `def` ran in, and `bind_parameters` uses it as
`enclosing`. Lookup reads that run's *current* env, so later assignments
are seen, like a Python closure. So a closure returned from a factory
and called from a completely different function still sees the factory's
locals. Full mechanism: `plan/03-call-tree/05` § "The environment".

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
matches before trusting a hit — or, for genuine closures, walk `enclosing`
specifically because the name is a *free variable of a nested function*,
never as a generic "search outward" fallback. And there is no caller link to
search: if `Env` had one and lookup walked it, `g` would find `f`'s `x` above,
which is this exact bug.

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
fn lift_at(&mut self, ty: Type<'db>, depth: u8) -> Values<'db> {
    if depth > MAX_LIFT_DEPTH { return smallvec![Unknown] }   // recursive aliases
    match ty {
        Type::FunctionLiteral(f) => smallvec![AbstractValue::Function(f)],
        Type::ClassLiteral(c)    => smallvec![AbstractValue::Class(c)],
        Type::ModuleLiteral(m)   => smallvec![AbstractValue::Module(m.module(self.db))],

        // `obj.method` ty already bound → BoundMethod{func, interned receiver}.
        // Drop this arm and it becomes Ty(BoundMethod): not callable, no edge.
        Type::BoundMethod(_) => { /* vnoc::bound_method_function + _self, see plan 04 */ }

        // ★ the important case: EXPLODE, never re-flatten
        Type::Union(u) => u.elements(self.db).iter().flat_map(|t| self.lift_at(*t, depth + 1)).collect(),

        Type::NominalInstance(i) => smallvec![AbstractValue::Instance {
            class: i.class(db, env).class_literal(db),
            origin: OriginId::OPAQUE,   // ty gave us this; we didn't construct it.
                                        // Shared by ALL opaque instances → never holds attrs.
        }],

        // class-ish types that aren't ClassLiteral — normalise, then the
        // ClassLiteral arm applies:
        Type::GenericAlias(_) => /* Box[int] → Box     (vnoc::generic_alias_origin) */,
        Type::SubclassOf(_)   => /* type[Base] → Base  (vnoc::subclass_of_class); type[Any] → Unknown */,
        Type::TypeAlias(a)    => self.lift_at(a.value_type(self.db), depth + 1),   // may be a union

        Type::Dynamic(_) | Type::Never | Type::Divergent(_) => smallvec![AbstractValue::Unknown],
        other => smallvec![AbstractValue::Ty(other)],
    }
}
```

**Every arm above `other` is a call edge.** Anything that falls to `Ty(..)`
can't be called (`as_callable` says no), so a missing arm doesn't fail
loudly, it just drops edges:

| arm | without it | example |
|---|---|---|
| `BoundMethod` | every bound method ty hands you is uncallable | `f = get_obj().method; f()` |
| `GenericAlias` | constructing a specialised generic gives no `__init__` edge | `Box[int]()` |
| `SubclassOf` | `cls()` with `cls: type[Base]` and no known caller gives no edge — the annotation fallback case, `04-annotation-fallback.md` | `def make(cls: type[Base]): cls()` |
| `TypeAlias` | a param annotated with an alias lifts to nothing | `type H = A \| B; def f(h: H): h.run()` |

The full arms, wrappers and reasoning are in
[`plan/03-call-tree/04`](../../../plan/03-call-tree/04-value-domain.md#converting-from-ty--the-boundary).

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
