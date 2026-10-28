# 04.03 — Effects

**You build:** writes that cross a call. A walk records every attribute it
writes, and whether on every path. Its summary reports the writes to input
objects as **effects**: "parameter 0's `handler` is now parameter 1",
relative to the inputs. The caller applies them to its own objects. A
constructor's writes to `self` are effects like any other, so every part 03
fixture passes in summary mode at the end of this step.
**Fixtures:** `python/effects.py`, then every `03-classes/python/` fixture in
both modes
**Plan:** `plan/03-call-tree/05-the-walk.md` § "Must-write tracking",
`09-summaries.md` § "Finishing a walk" (point 3), `10-one-call.md` § "Phase 2"

---

## The fixture

```python
class Pipeline:
    def __init__(self, handler): self.handler = handler
    def run(self): self.handler.process()
def install(p, h): p.handler = h
def maybe(p, h, c):
    if c: p.handler = h
def fresh(p): p.handler = JsonHandler()
def outer_install(p, h): install(p, h)

def ctor():            Pipeline(JsonHandler()).run()                  # JsonHandler.process
def installed():       p = Pipeline(CsvHandler()); install(p, JsonHandler()); p.run()   # Json only
def maybe_installed(c):p = Pipeline(CsvHandler()); maybe(p, JsonHandler(), c); p.run() # both
def fresh_installed(): p = Pipeline(CsvHandler()); fresh(p); p.run()                    # Json only
def through_two():     p = Pipeline(CsvHandler()); outer_install(p, JsonHandler()); p.run()  # Json only
```

## Predict

1. Run `03-classes/python/state.py` in summary mode with your step 02 code.
   What does `Pipeline.run` show, and which write never reached the caller?
2. Write `install`'s summary: its returns, and the one effect, with its
   target, attribute, value and whether it is strong.
3. `maybe` writes on one path. In its walk, what does `p.handler` hold at the
   exit, and how should the caller combine that with what it had?

---

## Build

### Decision 1: a walk records what it wrote, and whether on every path

```rust
pub enum Write { Must, May }

pub struct Env<'db> {
    pub locals: …,
    pub attrs: …,
    pub written: rpds::HashTrieMap<(OriginId, Name), Write>,   // ← new
    pub scope: …,
    pub enclosing: …,
}
```

- a strong write (`bind_attr` with one target) records `Must`; a weak one
  records `May`;
- an effect applied from a callee records `Must` only if it replaced the old
  value (decision 4), else `May`;
- `Env::join` keeps `Must` only where every arm reaching the join has `Must`.
  Anything else becomes `May`.

**Why a written set, and not a diff of attributes at the end:** a diff of
"exit state against starting state" misses a write whose value happens to
equal the old one. `p.handler = p.handler` changes nothing in the state, but
it is a write, and on a path where `p.handler` was two values it makes a
difference to strength. The set records the write itself.

### Decision 2: the summary reports writes to input objects, relative to the inputs

```rust
pub struct Effect<'db> {
    pub target: Path,          // the object written to, as it was when the call started
    pub attr: Name,
    pub value: SVals<'db>,
    pub strong: bool,          // written on every path that exits normally
}

pub struct FunctionResult<'db> {
    pub calls: …, pub returns: …,
    pub effects: Vec<Effect<'db>>,   // ← new, sorted
    pub objects: …, pub status: …,
}
```

`finish` reads the exit env's `written` set. For every `(origin, name)` whose
origin is `Entry(path)`: an effect with that target, the exit value converted
to `SVals`, and `strong` from `Must`. Writes to `New` origins are not effects:
those objects are described whole in `objects` (step 02), if they escape. New
objects reachable from effect values go into `objects` too.

| function | effect |
|---|---|
| `install(p, h)` | `Param(0).handler = [Ref(Param(1))]`, strong |
| `maybe(p, h, c)` | `Param(0).handler = [Ref(Param(1)), Ref(Param(0).handler)]`, weak |
| `fresh(p)` | `Param(0).handler = [Obj(0)]`, strong; `objects[0]` is a `JsonHandler` |
| `Pipeline.__init__(self, handler)` | `Param(0).handler = [Ref(Param(1))]`, strong |

`maybe`'s value includes the old handler (`Ref(Param(0).handler)`, the object
`materialize` put there) because at its exit the join of "wrote" and "didn't"
holds both.

Effects are sorted (by target, then attribute), so two equal summaries
compare equal with `==`. The recursion fixpoint needs that (step 02.05).

### Decision 3: `__init__` is not special

A constructor keys `__init__` with `self` as parameter 0 (step 01), so
`self.handler = handler` is an effect on `Param(0)`, and the caller applies it
to the new object's origin. There is no constructor rule for state. A write
anywhere is the same write (step 03.05), and an effect anywhere is the same
effect.

### Decision 4: the caller applies each effect to its own objects

For each effect: resolve the target path to origins, resolve the value to
values, then write. The write **replaces** the old value only when all three
hold:

- `effect.strong`: the callee wrote it on every path;
- the target resolved to exactly one origin;
- `strong_ok`: this call site had one callee (step 06 adds: and one combo).

Otherwise the new values are added to the old ones: a weak write. A target
that resolves to `OPAQUE`, `Unknown` or nothing is dropped (step 03.04). Each
write records `Must` or `May` in the caller's own `written` set, so the effect
travels on: `outer_install` gets `install`'s effect on its own `Param(0)`, and
reports it as its own.

```rust
for e in &res.effects {
    let targets = self.follow(s, &e.target, call, &before);       // read before the call
    let value = self.resolve_svals(s, &e.value, call, &before);
    let strong = e.strong && targets.len() == 1 && strong_ok;
    for t in targets.iter().filter_map(|t| t.state_origin(self)) {
        env.bind_attr(t, &e.attr, value.clone(), strong);         // records Must or May
    }
}
```

Both `follow` and `resolve_svals` read `before`, the state when the call
started, never the state after an earlier effect of the same call. Step 04 is
about exactly that.

### The gate

Switch every test of `03-classes/` to `check_both`. They all pass in summary
mode now. If one doesn't, it is one of the four conversions: print the key
(`fingerprint`), the starting env (`materialize`), the summary (`finish`) and
what the caller made of it (`resolve`). The bug is at the first one that looks
wrong.

### Yours

- `Write`, `Env.written`, recording in `bind_attr`, and the join rule.
- `Effect`, `FunctionResult.effects`, `finish` reading `written`.
- Applying effects in `call_project`, after resolving returns and objects.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs
impl<'db> Env<'db> {
    pub fn bind_attr(&mut self, origin: OriginId, name: &Name, v: Values<'db>, strong: bool) {
        // … the value, as in step 03.03 …
        let key = (origin, name.clone());
        let w = if strong { Write::Must } else { Write::May };
        let w = match (w, self.written.get(&key)) {
            (Write::May, Some(Write::Must)) => Write::Must,   // a weak write on top of a must-write stays must
            (w, _) => w,
        };
        self.written.insert_mut(key, w);
    }
}

// in Env::join, for `written`: a key is Must only if every arm has it as Must
```

```rust
// src/interp/summary.rs: in finish, after returns
let mut effects: Vec<Effect<'db>> = exit.written.iter()
    .filter_map(|((origin, name), w)| match &self.origins[*origin] {
        OriginKey::Entry(path) => Some(Effect {
            target: path.clone(),
            attr: name.clone(),
            value: exit.attr(*origin, name).iter().map(|v| self.to_sval(*v, &exit, &mut objs)).collect(),
            strong: matches!(w, Write::Must),
        }),
        _ => None,
    })
    .filter(|e| !e.value.is_empty())        // no value reached the write: no effect
    .collect();
effects.sort_by(|a, b| (&a.target, &a.attr).cmp(&(&b.target, &b.attr)));
let objects = self.describe_objects(&exit, &mut objs);   // after effects: they reach objects too
```

</details>

---

## Run

```rust
#[test]
fn p04_03_effects() {
    check_both("04-class-summaries/python/effects.py");
}
```

and every `p03_*` test switched to `check_both`.

---

## Where you are

```
Env            += written: (OriginId, Name) → Must | May
FunctionResult += effects: [Effect { target: Path, attr, value, strong }]
call_project:     resolve returns, objects and effects against `before`; then write
```

Summary mode now gives the same trees as inline mode on every fixture of parts
01–03 (part 04's first fixture keeps its one named difference, `deep`).

---

## Exercise

1. `def swap_in(p, h): old = p.handler; p.handler = h; return old`. Write the
   summary.
2. In `maybe_installed`, the effect is weak, and its value already contains
   the old handler. Is adding the old values again (the weak write) harmful?
3. Why can't an effect target a `New` origin?

<details><summary>Answers</summary>

1. Returns `[Ref(Param(0).handler)]`. Effect `Param(0).handler = [Ref(Param(1))]`,
   strong. Step 04 is about what the caller must do to get `old` right.
2. No: values are deduplicated, so the old handler appears once. The weak rule
   matters when the effect's value *doesn't* contain the old one: a strong
   effect applied to two candidate objects, or to one object through a call
   site with two callees.
3. A `New` origin is an object built during the call. The caller has never
   seen it, so there is nothing of the caller's to write to. It reaches the
   caller as `Obj(j)` with its whole final state, which already includes every
   write the callee made to it.

</details>

---

→ Next: [`04-resolve-then-write.md`](04-resolve-then-write.md)
