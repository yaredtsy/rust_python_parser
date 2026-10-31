# 04.04 — Resolve, then write

**You build:** the order of the two halves of applying a summary. Phase 1
reads every value the summary mentions (returns, effect targets, effect
values, new objects' attributes) from the state **before** the call. Phase 2
writes. Never interleaved.
**Fixture:** `python/resolve_first.py`
**Plan:** `plan/03-call-tree/10-one-call.md` § "Phase 1: resolve against the
snapshot", § "Phase 2: write", `15-checks-and-build-order.md` fixtures 1a, 1b

---

## The fixture

```python
class Box:
    def __init__(self, h): self.h = h
    def replace(self, h):
        old = self.h
        self.h = h
        return old
class Pair:
    def __init__(self, x, y): self.x = x; self.y = y
    def swap(self):
        t = self.x
        self.x = self.y
        self.y = t

def replacing():
    b = Box(JsonHandler())
    old = b.replace(CsvHandler())
    old.process()          # JsonHandler.process only
    b.h.process()          # CsvHandler.process only

def swapping():
    p = Pair(JsonHandler(), CsvHandler())
    p.swap()
    p.x.process()          # CsvHandler.process only
    p.y.process()          # JsonHandler.process only
```

## Predict

1. `replace`'s summary: returns `[Ref(Param(0).h)]`, effect
   `Param(0).h = [Ref(Param(1))]`. If the caller applies the effect first, then
   resolves the returns, what is `old`?
2. `swap` has two effects, each reading the other's attribute. Apply them one
   by one, each reading the current state. What do `p.x` and `p.y` end up as?
3. Inline mode gets both right without any rule. Why?

---

## Build

### Decision 1: phase 1 reads everything from the snapshot; phase 2 writes

```rust
let before = env.clone();                                   // a pointer copy, taken once per call expression
// Phase 1: resolve. Reads `before` only. Writes nothing.
let plan = self.resolve(s, &res, call, &before);
// Phase 2: write. New objects' attributes first, then effects.
self.apply(&plan, strong_ok, env);
out.extend(plan.returns);
```

```rust
pub struct Plan<'db> {
    pub returns: Values<'db>,
    pub objects: Vec<(OriginId, Vec<(Name, Values<'db>)>)>,
    pub effects: Vec<(SmallVec<[OriginId; 2]>, Name, Values<'db>, bool)>,   // targets, attr, value, strong
}
```

**Why:** every `Ref(path)` in a summary means "the object at this path **when
the call started**". That is how `finish` wrote it: `old = self.h` reads the
attribute before `self.h = h` runs, and `materialize` named that object
`Entry(Param(0).h)`. Reading the path after the effect has been applied
answers a different question: "what is there now".

- `replacing`, effects first: `b.h` becomes `CsvHandler`, then
  `Ref(Param(0).h)` reads `CsvHandler`. `old.process()` shows
  `CsvHandler.process`, and `JsonHandler.process` is gone. A wrong edge and a
  missing one, with no mark.
- `swapping`, one effect at a time: `x = Ref(Param(0).y)` writes `CsvHandler`
  into `x`; then `y = Ref(Param(0).x)` reads the *new* `x`, `CsvHandler`. Both
  are `CsvHandler`.

With phase 1 first, `Ref(Param(0).h)` is resolved to `JsonHandler` while it is
still there, and both of `swap`'s values are resolved before either is written.

**Why inline mode never needed this:** inline mode runs the statements in
order, in one walk: `old = self.h` reads the state at that line, which is the
old handler. A summary has lost the order of its statements; it keeps only
what is true at the start and at the end. Phase 1 is how the caller puts the
order back: everything the summary says about the start is read at the start.

### Decision 2: new objects first, then effects

In phase 2, the attributes of the call's new objects (`Obj(j)`) are written
before the effects. An effect's value can be a new object, and that object
should already hold its state when it lands. (An effect's *target* is never a
new object: step 03, exercise 3.)

### Decision 3: one snapshot per call expression

When a call site has several callees (`h.process()` where `h` is two
classes), each callee's phase 1 reads the same `before`, taken once for the
call expression. Callee 2's phase 1 never sees callee 1's writes. They are
alternatives, not a sequence: Python runs one of them. Step 06 adds combos,
which are alternatives too, under the same rule.

### Yours

- Split applying a summary into `resolve` (to a `Plan`) and `apply`.
- One `before` per call expression, shared by every callee.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/call.rs
fn resolve(&mut self, s: &mut Solver<'db>, res: &FunctionResult<'db>, call: &CallSite<'db>,
           before: &Env<'db>) -> Plan<'db> {
    // new objects: one origin each, attributes resolved against `before`
    let objects = res.objects.iter().enumerate().map(|(j, o)| {
        let origin = self.origins.intern(call.new_key(Made::Obj(j as u16)));
        let attrs = o.attrs.iter()
            .map(|(n, v)| (n.clone(), self.resolve_svals(s, v, call, before)))
            .collect();
        (origin, attrs)
    }).collect();
    let returns = self.resolve_svals(s, &res.returns, call, before);
    let effects = res.effects.iter().map(|e| (
        self.follow(s, &e.target, call, before).iter().filter_map(|v| v.origin()).collect(),
        e.attr.clone(),
        self.resolve_svals(s, &e.value, call, before),
        e.strong,
    )).collect();
    Plan { returns, objects, effects }
}

fn apply(&mut self, plan: &Plan<'db>, strong_ok: bool, env: &mut Env<'db>) {
    for (origin, attrs) in &plan.objects {
        for (name, v) in attrs {
            env.bind_attr(*origin, name, v.clone(), true);         // a new object: nothing to keep
        }
    }
    for (targets, attr, v, strong) in &plan.effects {
        let strong = *strong && targets.len() == 1 && strong_ok;      // counted before the filter (03.04)
        let targets: SmallVec<[OriginId; 2]> = targets.iter().copied().filter(|&o| self.holds_state(o)).collect();
        for &t in &targets {
            env.bind_attr(t, attr, v.clone(), strong);
        }
    }
}
```

</details>

---

## Run

```rust
#[test]
fn p04_04_resolve_first() {
    check_both("04-class-summaries/python/resolve_first.py");
}
```

To see the bug with your own eyes: swap the two lines in `call_project`
(apply, then resolve), run the test, and read the diff. Then swap them back.

---

## Where you are

```
call_project:  key → get → resolve (phase 1, reads `before`) → apply (phase 2, writes)
```

These are the two fixtures the plan puts first among its core fixtures:
simple fixtures pass with this bug present, so these are the ones that keep it
out of real projects.

---

## Exercise

1. `def take(b): h = b.h; b.h = None; return h`. Write the summary, and say
   what the caller's `take(box)` returns if phase 2 ran first.
2. A summary has an effect `Param(0).a = [Ref(Param(0).b)]` and another
   `Param(0).b = [Obj(0)]`. In phase 2, does the order of the two effects
   matter?
3. Why must `before` be taken *after* the arguments are evaluated, and not at
   the start of the statement?

<details><summary>Answers</summary>

1. Returns `[Ref(Param(0).h)]`, effect `Param(0).h = [Ty(None)]`, strong. With
   phase 2 first, `Ref(Param(0).h)` reads `None`, and the caller's `h.x()`
   loses its edge. With phase 1 first, it is the box's old handler.
2. No. Both values were resolved in phase 1, against `before`: `a` gets the old
   `b`, `b` gets the new object. Phase 2 only writes resolved values, and the
   two effects write different attributes.
3. The arguments are evaluated before the call runs, and they can change
   state (a call inside an argument, step 01.03, decision 3). The callee's
   inputs are the state *after* the arguments and *before* its body: that is
   where `Param(i)` and every path from it are read.

</details>

---

→ Next: [`05-deep-writes.md`](05-deep-writes.md)
