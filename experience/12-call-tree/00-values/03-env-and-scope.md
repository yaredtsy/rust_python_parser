# 00.03 — `Env`: names, objects, and where ty comes in

The environment is what one walk knows: what each local name holds, and what
each traced object's attributes hold. The rule it serves: *resolve from what
you know first, fall back to ty second.*

**Reading:** [`plan/03-call-tree/05-the-walk.md`](../../../plan/03-call-tree/05-the-walk.md)
§ "The environment", and
[`plan/03-call-tree/04-values.md`](../../../plan/03-call-tree/04-values.md) § "`lift`".

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

Jedi is **lazy**: when `inner` needs `y`, it walks back to the caller's context
to evaluate `x`, and from there back to `outer`'s.

pylspt is **eager**. At each call site the arguments are evaluated once, in the
caller's env, and turned into the callee's key. The callee's walk starts from
that key ([`01-summaries/02`](../01-summaries/02-the-key.md)). By the time
`inner`'s body is walked, `y` is already a `JsonWriter` instance. Nothing walks
back to a caller, so `Env` has no link to one.

```rust
#[derive(Clone)]
pub struct Env<'db> {
    locals:    rpds::HashTrieMap<Name, Values<'db>>,
    attrs:     rpds::HashTrieMap<(OriginId, Name), Values<'db>>,
    written:   rpds::HashTrieMap<(OriginId, Name), Write>,   // Must or May
    scope:     BodyScope,
    enclosing: Option<ActivationId>,    // the run where this function was DEFINED. Never the caller
}
```

| field | answers |
|---|---|
| `locals` | what is local `x` bound to, here? |
| `attrs` | what does this object's `.attr` hold? Keyed by origin ([`02-origins.md`](02-origins.md)) |
| `written` | which attributes did this walk write, and on every path or only some? The summary reports them to the caller ([`01-summaries/03`](../01-summaries/03-the-result.md)) |
| `scope` | am I allowed to see `x` here? The guard below |
| `enclosing` | if `x` isn't local, is it a free variable of a nested `def`? Closures only, [`02-functions/04`](../02-functions/04-closures-and-nested-functions.md) |

**`enclosing` is not the caller.** `outer → middle → inner` is a call chain,
and none of them is nested in another, so all three have `enclosing: None`.

| link | follows | lives in | used for |
|---|---|---|---|
| enclosing | where the function was **defined** | `Env.enclosing` | free variables of closures |
| caller | where the function was **called from** | the solver's stack and the call edges | recursion groups, the rendered tree |

## Why a persistent map

The walk takes a snapshot of the env at every call site (the state "before" the
call, [`01-summaries/04`](../01-summaries/04-one-call.md)) and forks it at every
branch. With `rpds::HashTrieMap`, a clone is a pointer copy and an insert shares
every untouched branch. With an owned `HashMap`, each clone is O(n) allocation,
and at this call volume that would be the whole profile.

## Why `scope` is a field

```python
def f(x):
    g()

def g():
    print(x)     # NameError at runtime: g has no x
```

Two unrelated functions can each have an `x`. A lookup checks the scope, or
walks `enclosing` only when ty's semantic index says the name is a free variable
of a nested function. Never a general "look further out". And there is no
caller link to look through: if there were, `g` would find `f`'s `x`.

## Late binding inside one walk

A `def` statement binds `Closure { func, activation }`, pointing at the run it
ran in. That run's slot holds its *current* env, updated after every statement.
A lookup reads it at call time, as Python reads a cell:

```python
h = A()
def inner(): h.go()
h = B()
inner()          # B.go
```

## The two-line rule

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>) -> Values<'db> {
    self.resolve_from_env(expr, env)                     // precise, path-aware
        .unwrap_or_else(|| self.resolve_from_ty(expr))   // fast, path-free
}
```

The env holds what this path knows ("this call's `w` is that `JsonWriter`").
ty is asked only when the env has nothing to say.

## `lift`: ty's answer, as values

When ty does answer, its `Type<'db>` becomes `Values<'db>`. The full function is
in [`plan/03-call-tree/04`](../../../plan/03-call-tree/04-values.md#lift-from-ty-back-into-values);
the shape:

| ty type | lifts to | without the arm |
|---|---|---|
| `FunctionLiteral` | `Function(def)` | |
| `ClassLiteral`, `GenericAlias` (`Box[int]`) | `Class(def)` | `Box[int]()` has no `__init__` edge |
| `SubclassOf` (`type[Base]`) | `Class(Base)` | `cls()` with `cls: type[Base]` has no edge |
| `ModuleLiteral` | `Module(m)` | |
| `BoundMethod` | `BoundMethod { func, receiver }`, receiver lifted | `f = get_obj().method; f()` has no edge |
| `Union` | **each member, separately** | the union is flattened: the whole point is lost |
| `NominalInstance` | `Instance { class, origin: OPAQUE }` | |
| `TypeAlias` | what it stands for | `type H = A \| B; def f(h: H)` lifts to nothing |
| `Dynamic`, `Never` | `Unknown` | |
| anything else | `Ty(t)` | |

Every arm above the last is a call edge. A value that falls to `Ty(..)` mostly
can't be called, so a missing arm doesn't fail loudly; it drops edges.

The `Union` arm is the line most likely to get "simplified" back into a bug. ty
hands you a set; your job is to keep its members separate. Flatten it back into
one `Type::Union` and you have rebuilt `ty_ide::outgoing_calls`.

`lift` doesn't cap the union. Its caller passes the result through `cap_fanout`,
the one place the cap lives: over `max_union_fanout`, it first drops values that
can't produce a tree node (stdlib, site-packages, `Ty(..)`), and only then gives
up with `Unknown`.

---

## Exercise

Trace `uses_all_five` from [`01-abstractvalue.md`](01-abstractvalue.md)
(`python/shapes.py`) by hand, as the walk:

1. `a = plain`: is `plain` found in the env, or does this go through `lift`?
   Which arm?
2. `c = Thing()`: does this touch `lift`?
3. After the function ends, has anything been added to `attrs` or `written`?
4. `uses_all_five` is walked with an empty key. Is that the same as saying its
   `Env` is empty when the walk starts?

<details><summary>Answers</summary>

1. `plain` is a module-level name, not a local, so the env misses and ty
   answers: `FunctionLiteral`, lifted to `Function(plain)`.
2. No. `Thing` lifts to `Class(Thing)`, but calling a class is the walk's own
   constructor: it mints a `New` origin for the site
   ([`03-classes/01`](../03-classes/01-constructing-an-instance.md)). It never
   asks ty what `Thing()` returns.
3. Nothing in `written`: no statement assigns to an attribute. `attrs` may hold
   whatever `Thing.__init__`'s summary wrote on the new object, if it wrote
   anything.
4. Yes, at the start: no parameters, no free variables. Names fill in as the
   statements bind them.

</details>

---

→ Next: [`04-annotation-fallback.md`](04-annotation-fallback.md)
