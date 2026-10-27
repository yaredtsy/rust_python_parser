# 03.10 — One call

What happens when the walk reaches a call expression: evaluate, snapshot, key,
get the callee's summary, resolve it against the snapshot, then write.

---

## `eval_call`

```rust
fn eval_call(s: &mut Solver<'db>, w: &mut Walk<'db>, call: &ast::ExprCall, env: &Env<'db>) -> Values<'db> {
    let callees = eval(s, w, &call.func, env);                  // 1. the function
    let args    = eval_arguments(s, w, &call.arguments, env);   // 2. the arguments, left to right
    let before  = env.clone();                                  // the pre-call state: an O(1) clone
    let site    = w.site_of(call);

    // A callee, or anything a key would read from an argument, is pending:
    // the whole call waits for the fixpoint (12).
    if w.has_pending(&callees) || w.args_have_pending(&args, &before) {
        w.calls.push(CallEdge { site, targets: SmallVec::new() });   // filled in the fixpoint
        return smallvec![w.defer(DeferredOp::Call { site, callee: callees, args }, &before)];
    }

    let single = callees.len() == 1;
    let mut out = Values::new();
    let mut targets = SmallVec::new();
    for (t, callee) in callees.iter().enumerate() {             // 3. then the call
        out.extend(call_one(s, w, site, t as u8, *callee, &args, &before, single, &mut targets));
    }
    w.calls.push(CallEdge { site, targets });
    s.cap_fanout(out)
}
```

`call_one` has three cases:

| callee | goes to | value |
|---|---|---|
| a class | [§ Constructors](#constructors) | the new instance |
| a project function, closure or bound method | `call_project` | the callee's returns, translated |
| anything else | `External(def)` or `Unresolved` | ty's declared return type, lifted. For a `Ty(Callable)`, ty's return type of calling it |

**The builtin fast path skips the callee, never the arguments.** A call like
`print(...)` is skipped by name, before resolving the callee. Steps 1 and 2 have
already run, so `print(make())` still reaches `make`. That matches the Jedi
driver, which finds `make()` as its own call.

**Multiple callees** each produce their own targets, and the call's value is the
union. `if x: return A() else: return B()` in the callee gives both.

---

## `call_project`

```rust
fn call_project(s: &mut Solver<'db>, w: &mut Walk<'db>, site: SiteId, t: u8,
                callee: AbstractValue<'db>, func: Definition<'db>, bound: &Bound<'db>,
                before: &Env<'db>, single_callee: bool,
                targets: &mut SmallVec<[Target<'db>; 2]>) -> Values<'db> {
    let combos: SmallVec<[Combo<'db>; 4]> = s.policy.split(func, bound).collect();   // CPA, capped
    let strong_ok = single_callee && combos.len() == 1;
    let runs = !bound.has_empty_slot();          // a parameter with no value: the call cannot run
    let mut out = Values::new();
    for (k, combo) in combos.iter().enumerate() {
        let ctx = s.key(w, func, combo, callee, before);
        let at  = (site, t, k as u8);
        targets.push(Target::Call(ctx));         // recorded now, even when the value waits
        match s.get(ctx) {
            Got::Ready(res) if runs => {
                // Phase 1: resolve every SVal against `before`. No writes.
                let plan = w.resolve(s, &res, combo, callee, before, at);
                // Phase 2: write. New objects' attributes first, then effects.
                w.apply(&plan, strong_ok);
                out.extend(plan.returns);
            }
            Got::Ready(_) => {}                  // edge only (below)
            Got::Pending if runs => {
                // The callee is in a recursion group still being walked. Record it, don't enter it.
                let op = DeferredOp::Apply { ctx, at, combo: combo.clone(), callee, strong_ok };
                out.push(w.defer(op, before));
            }
            Got::Pending => {}
        }
    }
    out
}
```

`bound` is [`06`](06-binding-arguments.md)'s parameter matching, kept as values
in caller terms: parameter slot to `Values`, receiver first, defaults filled.

### CPA: one context per combination

`split` takes the cartesian product of the relevant slots' values: a call
`build(h)` with `h` either `JsonHandler` or `CsvHandler` becomes two combos and
two contexts. Irrelevant slots don't split; they are `Unknown` in the key anyway
([`08`](08-keys.md#fingerprinting)). When the product is over `split_cap`, the
slots collapse into one combo whose fingerprints are `Fp::Union`, and
`Stats.cpa_collapses` counts it.

```rust
pub struct Combo<'db> { pub slots: SmallVec<[Values<'db>; 4]> }   // one value per split slot; several when collapsed
```

### A call with no value

An empty `Values` means no value reaches that point: every path to it raised,
or it waits on a recursion group that hasn't produced anything yet
([`04`](04-values.md#the-type)).

- **No callee value**: the site gets an edge with no targets, and the value is
  empty.
- **A parameter with no value**: Python never makes this call. The Jedi driver
  still shows it, because it finds calls by syntax. So the edge is recorded,
  with the empty slot keyed as `Unknown`, and the callee is walked so its
  subtree renders. Its returns and effects are not used, and the value is
  empty.

This keeps the fixpoint monotone: a call that waits on a value contributes
nothing until the value arrives ([`12`](12-recursion.md#the-fixpoint)).

---

## Phase 1: resolve against the snapshot

Every `SVal` in `returns`, `effects` (target and value) and `objects` is resolved
against `before`, the state when the call started.

- `Ref(path)`: start at the root, then follow each step.
  - `Param(i)` is combo argument `i`. `Free(i)` is free variable `i` of the
    callee closure, read from its activation.
  - `Attr` reads `before`'s attribute store, then the class side and ty, as
    `member` does. Reading past the caller's own key gives the caller's own
    path-named objects ([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)).
  - `Free(n)` reads a closure value's activation. `Recv` reads a bound
    method's receiver.
- `Obj(j)`: origin `New { site, t, combo: k, made: Obj(j) }`.
- `Closure { func, free }`: a new activation holding the resolved free values.
- `Plain(v)`: the matching `AbstractValue`.

Resolving after writing gives wrong trees:

```python
class Box:
    def replace(self, h):
        old = self.h
        self.h = h
        return old       # summary: returns Ref(Param(0).h). Effect: Param(0).h = Ref(Param(1))

old = box.replace(NewHandler())
old.process()            # must be the OLD handler's process
```

If the effect were applied first, `Ref(Param(0).h)` would read the new handler,
and the caller would never see the old one. A swap breaks the same way
([`15`](15-checks-and-build-order.md#fixtures-for-the-core), fixtures 1a and 1b).

## Phase 2: write

Write each new object's attributes into `Env.attrs`. Then apply each effect to
every origin its target resolved to. The write replaces the old value only when
all three hold:

- `effect.strong`
- the target resolved to exactly one origin
- `strong_ok`: one callee at this site and one combo

Otherwise the new value is joined with the old one. A target that resolves to
`OPAQUE`, `Unknown` or nothing is dropped. A target that resolves to a `Beyond`
object becomes this walk's own fallback effect and `DEEP_WRITE` gap
([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)). Every write
records `Must` (replaced) or `May` (joined) in `written`, so this call's effects
reach the caller's own summary correctly.

Phase 2 of combo `k` does not affect phase 1 of combo `k + 1`, because phase 1
always reads `before`. The same holds across callees `t`, because `before` is
taken once per call expression.

---

## Constructors

`Foo(args)` is a call to `Foo.__init__` with a new receiver. For each callee
class `t` and each combo `k`:

1. Mint origin `New { site, t, combo: k, made: Instance }`. Its fingerprint is
   `Instance { class, attrs: [] }`.
2. Find `__init__` through the class's MRO (`vnoc::member` on the class). Key
   it with the new receiver as parameter 0.
3. `get`, resolve and apply as above. `__init__`'s writes to `self` arrive as
   effects on `Param(0)` and land on the new origin. Objects it builds (a
   `self.log = Logger()`) are its `objects[j]`, with origins
   `New { site, t, combo: k, made: Obj(j) }`.
4. The call's value is the new instance. The edge target is
   `Construct { class, init: Some(ctx) }`.

`init` is `None` when no class in the MRO has an `__init__` body: a dataclass, a
`NamedTuple`, an attrs class, or no `__init__` at all. The class still gets its
edge, the instance has no attributes, and `member` falls through to ty's field
types.

`super().__init__(...)` inside `__init__` is an ordinary call on `self`
([`07`](07-attributes-and-self.md#inherited-attributes-and-super)). Its effects
land on the same origin.

Two calls to `Foo(h)` with equal fingerprints share one `__init__` summary but
get two origins, one per site, so their attributes never mix.

---

## Fixtures

```python
# 1. simple factory
def make(): return JsonWriter()
def main(): w = make(); w.write(1)            # JsonWriter.write

# 2. two returns: both
def build(c): return JsonWriter() if c else CsvWriter()
def main(): build(True).write(1)              # JsonWriter.write and CsvWriter.write

# 3. chained returns, depth 2
def outer(k): return build(k)
def main(): outer(1).write(1)                 # both writes

# 4. generator: Unknown, not JsonWriter
def gen(): yield JsonWriter()
def main():
    for w in gen(): w.write(1)                # no write edge

# 5. await is transparent
async def afetch(): return JsonWriter()
async def main(): (await afetch()).write(1)   # JsonWriter.write

# 6. a call whose argument never arrives still shows
def fail(): raise ValueError()
def main(): log(fail())                       # main > fail, main > log

# 7. CPA: one context per handler class
def build(h): return Job(h, h.key())
def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()                             # see 15, fixture 3
```

---

→ Next: [`11-closures-and-callbacks.md`](11-closures-and-callbacks.md)
