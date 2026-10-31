# 03.03 — Attribute state

**You build:** objects that hold things. `self.handler = handler` writes into
the env, keyed by (origin, name). `self.handler.process()` reads it back,
before asking ty. And the state flows into a call and back out of it, so what
`__init__` wrote is there when `run` is called.
**Fixture:** `python/state.py`
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Where attribute
state lives", § "Write: `bind_attribute`"

---

## The fixture

```python
class Pipeline:
    def __init__(self, handler):
        self.handler = handler
    def run(self, data):
        self.handler.process(data)

def read_json(): Pipeline(JsonHandler()).run(1)   # … Pipeline.run > JsonHandler.process
def read_csv():  Pipeline(CsvHandler()).run(2)    # … Pipeline.run > CsvHandler.process

def make(h): return Pipeline(h)
def via_factory(): make(JsonHandler()).run(1)     # … Pipeline.run > JsonHandler.process

def either(c):
    if c: p = Pipeline(JsonHandler())
    else: p = Pipeline(CsvHandler())
    p.run(1)                                      # two objects: a Pipeline.run node for each
```

`read_json` / `read_csv` is the fixture the whole design exists for: one
method, `run`, whose callee depends on what the object it runs on was built
with.

## Predict

1. Run it with your step 02 code. What does `Pipeline.run` show, and why?
2. `__init__`'s write happens in `__init__`'s frame. `run` is called from
   `read_json`'s frame, after `__init__` has returned. Where does the write
   have to be, for `run` to see it?
3. In `either`, `run` is walked on two objects. Does the second walk see what
   the first one wrote?

---

## Build

Where this step's code goes:

```
eval_attribute ─ member(recv, name, env)        reads (origin, name) first      Decision 2
bind_target ──── bind_attribute(attr, v, env)   writes (origin, name)           Decision 3
eval_call ─ call_values(…, env: &mut Env)       each callee from `before`, join  Decision 5
              ├─ call_function(…, env)  ─┐
              └─ construct(…, env)      ─┴─ inline_node(…, env)
                                              └─ walk_inline(…, env)  attrs in, exits out   Decision 4
walk_stmt(Return) ─ frame.exits.push(env)                                         Decision 4
```

### Decision 1: state lives in the env, keyed by (origin, name)

```rust
// src/interp/env.rs
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    pub attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,   // ← new
    pub scope: Option<BodyScope<'db>>,
    pub enclosing: Option<ActivationId>,
    pub flat: bool,
}
```

**Why in the env, not in a heap on the side:** branches. In `either`, each arm
builds its own `Pipeline`. If an arm writes `p.handler = X`, the other arm
must not see it, and after the `if` both writes are possible. The env already
forks and joins exactly like that (step 01.09). `Env::join` unions `attrs` the
same way it unions `locals`.

Part 04 adds a second reason: every call takes a snapshot of the state before
it, and a persistent map makes that a pointer copy.

### Decision 2: read the object's own state first, then the class

```rust
// member(), the Instance arm, before step 02's lookup
AbstractValue::Instance { class, origin } => {
    if let Some(v) = env.attrs.get(&(origin, Name::from(name))) {
        return v.clone();                                  // 1. this object, on this path
    }
    // 2. the class side, through ty: step 02, unchanged
}
```

`member` gains `env: &mut Env<'db>`, passed from `eval_attribute`. This step
only reads it. It is `&mut` because step 06 calls a property getter from
here, and a call can change state.

**The order is the design.** What this path wrote on this object beats what
the class declares, which beats ty's general answer. Each step down trades
precision for coverage.

Tracing `self.handler.process(data)` in `run`: `self` is the object. `.handler`
hits step 1: `__init__` wrote it. `.process` is a second `member` call, on the
`JsonHandler` object, and hits step 2: `JsonHandler.process`.

### Decision 3: a write goes to every object the receiver may be

```rust
// src/interp/member.rs
pub fn bind_attribute(&mut self, sol: &mut Solver<'db>, attr: &ExprAttribute,
                      v: Values<'db>, env: &mut Env<'db>) {
    let recvs = self.eval(sol, &attr.value, env);       // `a.b.c = v`: the receiver is `a.b`
    let targets: SmallVec<[OriginId; 2]> = recvs.iter()
        .filter_map(|r| match r { AbstractValue::Instance { origin, .. } => Some(*origin), _ => None })
        .collect();
    let strong = targets.len() == 1;                    // exactly one object: replace
    for origin in targets {
        env.bind_attr(origin, &attr.attr.id, v.clone(), strong);
    }
}
```

```rust
// src/interp/walk.rs, bind_target
Expr::Attribute(a) => self.bind_attribute(sol, a, v, env),   // was: eval the receiver only
```

- **Only the last name is written.** `a.b.c = X()` evaluates `a.b` (two reads)
  and writes `c` on whatever `a.b` is.
- **One object: replace. Several: add.** If the receiver may be two objects,
  Python wrote only one of them, so each gets "old value or new value". This
  is a **weak** write. Step 05 tests it.
- **No object: nothing is written.** A write to `Unknown` or to `ty`'s answer
  is dropped. Guessing which object changed would corrupt unrelated paths.
- **Not covered:** an attribute inside an unpacked target (`a, self.x = …`)
  goes through `bind_from_ty`, which only evaluates it, so that write is
  dropped. `self.n += 1` isn't a write either: `AugAssign` doesn't bind.

### Decision 4: state flows into a call, and back out of it

A callee starts with the caller's state, and the caller continues with the
callee's state at its exits:

```rust
// walk_inline, gains `env: &mut Env<'db>`: the caller's env
let mut start = self.env_from_bound(node, &bound);
start.scope = Some(sol.sem.body_scope(def, node));
start.enclosing = enclosing;
start.attrs = env.attrs.clone();                 // ← the callee sees every object as it is now
if let Some(end) = self.walk_stmts(sol, &node.body, start) {
    let none = self.none_value(sol);
    self.frame.returns.extend(none);             // fell off the end
    self.frame.exits.push(end);                  // ← and that is an exit
}
let callee = std::mem::replace(&mut self.frame, caller);
if let Some(exit) = Env::join(callee.exits.into_iter().map(Some).collect()) {
    env.attrs = exit.attrs;                      // ← the caller continues with what the callee left
}
```

```rust
// walk_stmt, Stmt::Return
self.frame.returns.extend(v);
self.frame.exits.push(env);                      // ← the state at this `return`
return None;
```

`Frame` gains `exits: Vec<Env>`: the env at each `return` and at the end of
the body. Only `attrs` is taken from it. The callee's locals stay in the
callee.

`env` goes down the whole inline path: `call_values` → `call_function` /
`construct` → `inline_node` → `walk_inline`. That is how a constructor's
writes reach `read_json`: `construct` walks `__init__` on the caller's env,
and `.run` is evaluated after it on the same env.

If every path of the callee raises, there are no exits, and the caller's state
is left as it was. A `[recursive]` leaf walks nothing, so it changes nothing.

### Decision 5: several callees at one site are alternatives, not a sequence

In `either`, `p.run` is `run` on two objects, and Python makes one of those
calls, not both. So each callee starts from the **same** state, the one before
the call, and the caller continues with the join of what they left.
`call_values` takes `env: &mut Env<'db>` (it was `&Env`) and does this around
its three inline loops:

```rust
// call_values
let before = env.clone();                        // a pointer copy
let mut after: Vec<Option<Env<'db>>> = Vec::new();

// in each inline loop (functions, methods, classes), around the call:
let mut e = before.clone();                      // every callee starts here
out.extend(self.call_function(sol, func, None, Some(recv), range, args, &mut e));
after.push(Some(e));

// after the loops
if ask_ty {
    after.push(Some(before.clone()));            // ty's callee: no state change you can see
}
if let Some(joined) = Env::join(after) {
    *env = joined;                               // one callee: exactly its state
}
```

**Why not walk them one after another on `env`:** the second callee would see
the first one's writes, and the tree would depend on the order the callees
happen to be walked in. The sort in `call_values` is for the order of the
nodes, not for the state. Part 04 has the same rule in summary mode: one
snapshot per call expression (step 04.04, decision 3).

**Why `ask_ty` counts as an arm:** a callee that is `{project function,
Unknown}` may be the unknown one at runtime. Then nothing you can see
changed, so "as before" has to be one of the possibilities. Without it, a
strong write in the project callee would look certain.

Summary mode's paths pass through unchanged. They push nothing to `after`, so
`env` stays as it was. The deferred replay in `defer.rs` calls `call_values`
on a copy, `&mut d.before.clone()`, and drops it: state effects in summary
mode are part 04.

**This is why inline mode is the easy case.** One walk holds every object, and
state just flows along the calls. Part 04 has to say the same thing in a
summary that knows nothing about its caller's objects: "parameter 0's
`handler` is now parameter 1".

### Two receivers, two nodes

`p.run` in `either` is two bound methods of one function, so `run` is walked
twice and makes two `Pipeline.run` nodes at that site, one per object, in the
order the objects joined. The walker doesn't merge them. Merging nodes of the
same function is done at a later layer.

### Yours

- `Env.attrs`, `bind_attr`, and `attrs` in `Env::join`.
- `bind_attribute`, called from `bind_target`.
- The state read in `member`, and `env` on `member`.
- `Frame.exits`, the push in `walk_stmt`'s `Return` and at fall-through, and
  the attrs in and out of `walk_inline`.
- `env: &mut Env` on `call_values`, `call_function`, `construct`,
  `inline_node`, `walk_inline`. The before / after join in `call_values`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs
impl<'db> Env<'db> {
    pub fn bind_attr(&mut self, origin: OriginId, name: &Name, v: Values<'db>, strong: bool) {
        let key = (origin, name.clone());
        let v = match (strong, self.attrs.get(&key)) {
            (false, Some(old)) => {
                let mut m = old.clone();
                m.extend(v);
                dedup(&mut m);
                m
            }
            _ => v,
        };
        self.attrs.insert_mut(key, v);
    }

    /// Union per name. A name missing from an arm was not assigned on that path.
    pub fn join(arms: Vec<Option<Env<'db>>>) -> Option<Env<'db>> {
        let mut arms = arms.into_iter().flatten();
        let mut out = arms.next()?;
        for arm in arms {
            union_into(&mut out.locals, &arm.locals);
            union_into(&mut out.attrs, &arm.attrs);      // ← new
        }
        Some(out)
    }
}

/// The old loop body of `join`, made generic over the key.
fn union_into<'db, K: Clone + Eq + std::hash::Hash>(
    out: &mut rpds::HashTrieMap<K, Values<'db>>,
    arm: &rpds::HashTrieMap<K, Values<'db>>,
) {
    for (key, vals) in arm.iter() {
        let merged = match out.get(key) {
            Some(old) if old == vals => continue,
            Some(old) => {
                let mut m = old.clone();
                m.extend(vals.iter().copied());
                dedup(&mut m);
                m
            }
            None => vals.clone(),
        };
        out.insert_mut(key.clone(), merged);
    }
}
```

```rust
// src/interp/call.rs, call_values: the inline loops in full
let before = env.clone();
let mut after: Vec<Option<Env<'db>>> = Vec::new();
for (def, enclosing) in targets {
    let v = match sol.opts.mode {
        Mode::Inline => {
            let range = self.site_range(site);
            let mut e = before.clone();
            let v = self.call_function(sol, def, enclosing, None, range, args, &mut e);
            after.push(Some(e));
            v
        }
        Mode::Summary => self.call_project(sol, def, enclosing, args, &mut edge_targets, env),
    };
    out.extend(v);
}
for (func, recv) in methods {
    let range = self.site_range(site);
    let mut e = before.clone();
    out.extend(self.call_function(sol, func, None, Some(recv), range, args, &mut e));
    after.push(Some(e));
}
for class in classes {
    let range = self.site_range(site);
    let mut e = before.clone();
    out.extend(self.construct(sol, class, range, args, &mut e));
    after.push(Some(e));
}
// … edge bookkeeping unchanged …
if ask_ty {
    after.push(Some(before.clone()));
    // … ty's type of the call, unchanged …
}
if let Some(joined) = Env::join(after) {
    *env = joined;
}
```

`call_project` still takes `&Env`: pass `env` (it reborrows).

```rust
// src/interp/defer.rs, replay
let mut before = d.before.clone(); // summary mode: state effects are part 04's
self.call_values(sol, site, &callee, &args, None, &fallback, Some(edge), &mut before)
```

</details>

---

## Run

```rust
#[test]
fn p03_03_state() {
    check("03-classes/python/state.py");
}
```

Then 03.01 and 03.02 again: neither writes an attribute, so `attrs` stays
empty and their trees don't change.

---

## Where you are

```
Env   { locals, attrs: (OriginId, Name) → Values, scope, enclosing, flat }
Frame { …, exits: [Env] }
member(Instance): the object's state, then the class (ty)
bind_attribute: strong for one object, weak for several, dropped for none
a call: attrs in, join of exits out; several callees: each from `before`, joined
```

The constructor-injected handler resolves. That is most of what real
application code needs from objects.

---

## Exercise

1. `class C: def __init__(self): self.h = A(); def reset(self): self.h = B()`.
   `c = C(); c.reset(); c.h.go()`. Which `go`? Trace where `B` comes from.
2. A method that raises on every path writes `self.h = X()` first. Does the
   caller see the write? Does Python?
3. Why is "a write to `Unknown` is dropped" safe, and "a write to `Unknown`
   writes every object" not?

<details><summary>Answers</summary>

1. `B.go` only. `reset` replaces `(c, h)` with `B` (one object: a strong
   write). Its exit env carries that back to the caller, which continues with
   `h = B`.
2. No: the callee has no exits, so `walk_inline` leaves the caller's state as
   it was. Python does see it, if the caller catches the exception. Exceptions
   are not modelled; the same gap exists for every value set before a raise.
3. Dropping it can only lose an edge (the read later sees the old value, or
   falls to ty). Writing every object would add the value to objects that
   never received it, and give edges Python never takes, anywhere in the
   program.

</details>

---

→ Next: [`04-two-instances-and-opaque.md`](04-two-instances-and-opaque.md)
