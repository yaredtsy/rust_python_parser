# 04.01 — Objects in keys

**You build:** summary mode for a method called on an object. The object goes
into the key as its class plus its attributes' fingerprints, two steps deep.
The method's walk starts with an object rebuilt from that: named by its place
among the inputs, holding exactly the attributes the key carried. And
constructors start working in summary mode, minus `__init__`'s writes (step 03).
**Fixture:** `python/in_keys.py`
**Plan:** `plan/03-call-tree/08-keys.md` § "Fingerprinting" (instances,
depth), § "Materializing", `10-one-call.md` § "Constructors"

---

## The fixture

```python
class Box:                          # no __init__: the caller writes its state
    def run(self): self.handler.process()

def json_box(): b = Box(); b.handler = JsonHandler(); b.run()   # … JsonHandler.process
def csv_box():  b = Box(); b.handler = CsvHandler();  b.run()   # … CsvHandler.process

def shared():
    a = Box(); a.handler = JsonHandler()
    b = Box(); b.handler = JsonHandler()
    a.run()
    b.run()                         # one Box.run summary for both

def use(b): b.run()
def passed():
    b = Box(); b.handler = CsvHandler()
    use(b)                          # use > Box.run > CsvHandler.process

def deep():                         # s.app.config.handler, three steps from self
    …
    s.run()                         # inline: JsonHandler.process. summary: nothing

class Runner:
    def run(self): self.cb()

def again(n):
    r = Runner()
    r.cb = again(n - 1)             # summary: pending, `again` is still running
    r.run()                         # inline: nothing. summary: log
    return log

def again_closure(n):
    r = again_closure(n - 1)        # summary: pending
    def g(): r()
    g()                             # inline: nothing. summary: log
    return log
```

## Predict

1. Write `Box.run`'s key in `json_box`. What does it hold for `self`?
2. `shared` has two different `Box` objects. How many summaries of `Box.run`
   does summary mode compute? Is that right?
3. Why can't the key hold the object's origin?
4. In `again`, `r.cb` holds a pending value when `r.run()` is reached. What
   would the key say about `cb` if you keyed the call right there?

---

## Build

### Decision 1: an object in a key is its class plus its attributes

```rust
pub enum Fp<'db> {
    // …
    Instance { class: Definition<'db>, attrs: SmallVec<[(Name, FpId); 4]> },   // sorted by name
    Opaque(Definition<'db>),                                                   // class only
    Bound { func: Definition<'db>, recv: FpId },                               // a bound method's receiver
}
```

`json_box` calls `run` with `self = Instance { Box, [handler: Instance { JsonHandler, [] }] }`.
`csv_box` gives `Instance { Box, [handler: Instance { CsvHandler, [] }] }`. Two
keys, two walks, two different children: the per-path tree, from the key.

The attributes come from the caller's state just before the call, so
fingerprinting needs the env. `key`, `fingerprint`, `fp_one`, `fp_free` and
`fp_defaults` all take `env: &Env<'db>`, the caller's env at the call.
`fp_free` and `fp_defaults` need it too: an object held by a closure's free
value has its attributes in that same env.

`key` has four callers. Each passes the env it holds at that point:

| caller | env |
|---|---|
| `solve` (`mod.rs`), the summary root | `&Env::default()`: no object exists yet |
| `inline_tree` (`mod.rs`), the inline root | `&objects`: what the prologue built (step 03.08) |
| `walk_inline` (`call.rs`) | `env`, the caller's (the same attrs `start` copies) |
| `call_project` (`call.rs`) | `env` |

Inline mode keys only to count contexts (`Stats.contexts`), but the count is
compared with summary mode's, so it gets the real env too.

**Why not the origin:** an origin is an id in the caller's walk. It means
nothing to the callee, which is walked once and reused by every caller with
the same key. Same reason as a closure's activation in step 02.07: rule 2, in
the key.

**Why the attributes:** the method's walk reads them. `run` reads
`self.handler`. If the key held only the class, both callers would share a
walk that can't know which handler it has.

**One object's attributes, from a flat map.** `Env.attrs` is keyed by
`(OriginId, Name)`. Fingerprinting wants one object's attributes, sorted by
name. Add `Env::attrs_of(origin)`, which scans the map and sorts. Keep the flat
map: `bind_attr`, `join` and every `attrs` copy stay as they are. A path holds
few attributes, so the scan is cheap. A map per object
(`HashTrieMap<OriginId, HashTrieMap<Name, Values>>`) is the change to make if
a profile shows the scan, not before.

### Decision 2: two steps deep, and then the class only

Every step counts: an attribute, a bound method's receiver, a closure's free
value or default. At `fp_depth` (2), an instance becomes `Opaque(class)`: its
class, none of its attributes. An instance that holds no state (`OPAQUE`) is
always `Opaque(class)`.

**Why a limit:** without one, a key holds everything reachable from the
arguments. `self.parent.parent.parent…` makes keys huge, and an object that
refers to itself (`node.root = node`) makes them infinite. A limit keeps every
key small and finite.

**What it costs:** `deep`. `run` reads `self.app.config.handler`, three steps
from `self`. The key stops at `self.app.config` as `Opaque(Config)`, so in the
walk `config` has no `handler`, and the class side knows none: no edge. Inline
mode, with no keys, finds it. The fixture's mode-specific blocks pin the
difference. It is a known limit (`plan/03-call-tree/15` § "`fp_depth = 2`
loses reads at depth 3"), to be measured: K = 2 against K = 3, on the fixtures
and one real project, before the default is final.

`fp_depth`'s comment in `Limits` (`mod.rs`) says "closures nest at most this
deep". Change it to "steps from a parameter (attribute, receiver, free value,
default), in keys and in summaries".

### Decision 3: where an object was built is not in the key

`shared`'s two boxes were built on different lines. Their fingerprints are
equal, so `b.run()` reuses `a.run()`'s summary. Walks: `shared`, `Box.run`,
`JsonHandler.process`: 3, and one reuse.

**This sharing is the point.** A key with allocation sites would give every
`Pipeline(JsonHandler())` in the project its own `run` walk, finding the same
thing every time. Type sensitivity, keyed by what an object holds, is what
makes the number of contexts small.

What is lost is *identity*: the callee can't tell your object from another
equal one. Step 02 gets it back, without the key: a summary names objects by
their place among the inputs.

### Decision 4: a walk rebuilds input objects from the key, named by path

```rust
pub enum OriginKey {
    Opaque,
    Entry { act: ActivationId, path: Path },       // an input object, named by where it sits
    New { site: TextRange, act: ActivationId },    // built in this walk (step 06 widens this)
}

pub struct Path { pub root: Root, pub steps: SmallVec<[Step; 2]> }
pub enum Root { Param(u8), Free(u8), Default(u8) }
pub enum Step { Attr(Name), Free(u8), Default(u8), Recv }
```

`materialize` turns `Instance { class, attrs }` for parameter `i` into
`Instance { class, origin: Entry(Param(i)) }`, and each attribute into its own
values at `Param(i).name`, written into the walk's starting env. `Opaque(class)`
gets an `Entry` origin too, with no attributes: it is still one specific caller
object, so writes to it can be tracked (step 03).

`Entry { act, param: i }` from step 03.07 becomes `Entry { act, path: Param(i) }`
(`Path { root: Param(i), steps: [] }`). It has two users: `from_fp` here, and
the annotation fallback in `bind.rs`. Keep `act`. A summary walk is one run,
so there it is always the walk's own run and changes nothing. But inline mode
runs every call inside one walk, and its annotation fallback still makes
`Entry` origins per call. Without `act`, `f(u); g(u)` with `u` unknown gives
`f`'s `h` and `g`'s `k` one origin, and `f`'s writes show up in `g`: the step
03.07 bug again (`03-classes/python/chains.py` `separate` pins it). In prose
below, `Entry(Param(0).handler)` is short for
`Entry { act: <this run>, path: Param(0).handler }`.

**Why two summary walks can't collide:** each `walk_context` starts a new
`Walk` with its own origin table, and a call in summary mode walks the callee
in another `Walk`. Origins never cross: keys hold fingerprints, and results
and effects come back as paths (steps 02 and 03).

`run`'s walk for `json_box`'s key: `self = Instance { Box, Entry(Param(0)) }`,
`(Entry(Param(0)), handler) = Instance { JsonHandler, Entry(Param(0).handler) }`.
`self.handler.process()` finds the state, then `JsonHandler.process`.

**Objects inside closures.** A closure in the key carries free values and
defaults, and they can hold objects. Each one gets a path too:

| where the object sits | its path |
|---|---|
| free value `i` of the walked closure itself (`key.free`) | `Free(i)` |
| default `i` of the walked closure itself (`key.defaults`) | `Default(i)` |
| free value `n` of a closure at path `p` | `p + [Free(n)]` |
| default `n` of a closure at path `p` | `p + [Default(n)]` |

`i` and `n` index the fingerprint's lists, which are sorted by name. The
closure's free values go into its flat activation, as in part 02. Their
objects' **attributes** don't: they go into the walk's starting env, the one
`materialize` returns, because `member` reads attributes from the env being
walked. A flat activation's `attrs` are never read.

**Why defaults get a path:** an object that only a default holds is still a
caller's object. If the walk calls the closure without that argument, the
callee writes to its parameter, and that write lands on the default's object.
Without a path, the effect has no name to be reported under, and it is lost
with no mark. `Default` is new: step 02 resolves it by reading the closure's
defaults, like `Free` reads its activation.

**A union at one path:** `Union(xs)` rebuilds every member at the same path,
so two objects share one origin. That is why `from_fp` writes attributes
weakly. For one object there is nothing there yet, so a weak write is the
same as a strong one. For a second member at the same path, a strong write
would replace the first member's attributes. Sharing an origin is a known
imprecision, which CPA (step 06) mostly removes.

### Decision 5: a method call goes through a summary

Today `call_values` sends `BoundMethod` and `Class` callees down the inline
path only (the `Mode::Inline` guards on those arms). In summary mode they fall
through to ty. Drop the guards and branch on the mode inside the loops, as the
plain-function loop already does.

A method is a function whose first slot is the receiver. `call_project` gains
`receiver: Option<AbstractValue<'db>>` and hands it to `bind_for_callee`,
which passes it to `bind_parameters` (it passes `None` today). The receiver is
then slot 0: it is fingerprinted like any argument, and `materialize` rebuilds
it at `Param(0)`. A classmethod's receiver is `Class(c)`, which keys as
`Fp::Class`. Nothing else changes.

Summary mode doesn't touch `after`. A summary-mode call doesn't change the
caller's env until effects (step 03).

### Decision 6: constructors in summary mode

`Box()` makes a new origin in the caller's walk: `New { site, act }`, the same
key inline mode's `construct` uses. Then, if the class has a project
`__init__` (`sem.init_of`, through the MRO), key it with that object as
`self`, `get` it, and record the edge as a construct:

```rust
pub enum Target<'db> {
    Call(ContextId),
    Construct { class: Definition<'db>, init: Option<ContextId> },   // ← new
}
```

`Target` gains a lifetime, and so do `CallEdge` and `Frame.calls`.

The new object has no attributes in the env yet, so `self` keys as
`Instance { class, [] }` without a special case.

`__init__`'s value is never used, so `get`'s answer only matters for
starting the walk. If `__init__` is still running (`Tree()` inside
`Tree.__init__`), `get` answers `Pending`, and there is nothing to defer
until its writes become effects (step 03, then step 08 for the cycle).

What's missing: `__init__`'s writes to `self`. They are effects, step 03. Until
then, only fixtures whose state is written by the caller pass in summary mode,
which is why `Box` has no `__init__`.

### Decision 7: render draws a construct as its class

`render` today reads each target as `let Target::Call(ctx) = *t;`, which
stops compiling once `Construct` exists. Every target names one definition
and the contexts under it:

| target | node named by | contexts under it |
|---|---|---|
| `Call(ctx)` | `contexts[ctx].func` | `[ctx]` |
| `Construct { class, init }` | `class` | `init`, or none |

`sites_union` collects targets, not contexts. `group_by_function` groups by the
naming definition, so two `Construct`s of one class at one site (two `__init__`
keys) are one node, with both contexts' children. A node with no contexts is a
leaf, as in inline mode. The recursion guard compares the class's qualified
name, which is what inline mode's guard compares (`construct` → `inline_node`
with the class's name). So `Tree()` inside `Tree.__init__` is `Tree [recursive]`
in both modes.

Order: `group_by_function` sorts by definition position. Inline mode orders a
site's callees as functions, then methods, then classes, each sorted by
position. These agree unless one site calls both a class and a function
(`(A if c else f)()`). No fixture does. If one ever does, add the kind to the
sort.

### Decision 8: a pending value inside an object defers the call

```python
def again(n):
    r = Runner()
    r.cb = again(n - 1)   # Pending(0): `again` is running
    r.run()
```

`needs_defer` defers a call when the callee or an argument *is* pending. Here
neither is: the argument is `r`, an object. But `Runner.run`'s key reads
`r.cb`, which is `Pending(0)`. Keyed now, `cb` would fingerprint as `Unknown`,
`run` would be walked with no `cb`, and the `log` edge would be lost with no
mark. Deferring is the answer part 02 gave for arguments. It now has to look
as deep as the key would. That is step 08's rule ("the check before keying
looks as far as the key would"), and it is needed now. `Call` is the only
deferred operation it uses, and part 02 already has it.

There are two halves, and each is useless without the other:

1. **Before keying: `reaches_pending`.** It follows exactly what
   `fingerprint` reads: attributes to `fp_depth`, a bound method's receiver,
   and a closure's free values and defaults. It is checked on every argument,
   on a closure callee's own free values and defaults, and on a bound-method
   callee's receiver. It applies only when some callee is project code:
   `Function`, `Closure`, `BoundMethod` or `Class`. Today `needs_defer` ignores
   the last two.
2. **In the fixpoint: keying reads this round's values.** A deferred `Call`
   replays `call_values` with `substitute`d callee and arguments, but against
   `d.before`, the env as it was. `r.cb` there is still `Pending(0)`.
   Substituting only the arguments would key the same `Unknown` every round.
   So `replay` keeps its round's values on the walk (`Walk.now`, a field
   instead of the local `now`). `fp_one`'s `Pending(id)` arm fingerprints
   `now[id]` in its place. Outside a replay it is never reached, because half
   1 deferred the call. A `debug_assert` there catches a gap in
   `reaches_pending`.

This matches step 02.07, where a closure's pending free value is substituted
when the summary is finished, not dropped.

`again` shows it. Round 0: `r.run()` is deferred. Round 1: `Pending(0)` is
`[Function(log)]` (round 0's return), so `run` keys with
`self = Instance { Runner, [cb: Function(log)] }`, and its walk finds `log`.
Inline mode stops at the recursion guard with `Unknown` and finds nothing.
Summary mode finds more, as in `02/pending.py`.

`again_closure` is the same gap for functions, and it is in part 02's code
today: `g()`'s key reads `g`'s free value `r`, which is `Pending(0)`, and
`fp_one` turns it into `Unknown`. Summary mode shows `again_closure.g` with no
child and no mark. With both halves, `g` is keyed in round 1 with `r = log`.

**What doesn't change:**
- **Inline mode:** it never makes a `Pending`, and `needs_defer` runs only in
  summary mode.
- **Code with no recursion:** a `Pending(id)` is an index into
  `Walk.deferred`, so a walk that deferred nothing holds none.
  `needs_defer` returns `false` at once when `deferred` is empty, so the deep
  check costs nothing there.
- **Part 02's recursion fixtures** (`pending.py`, `groups.py`,
  `closures_across.py`): their pending values sit directly in a callee or an
  argument, which today's check already defers. `chain`'s closure holds a
  pending `r`, but it is returned, not called or passed, so it is never
  keyed in `chain`'s walk. The deep check gives the same answers there, and
  `now` is never read.

Not covered here: the pending value's own attributes and methods (`p.name`,
`p.m()` with `p` pending). Until step 08, `eval_attribute` answers those with
ty's type of the expression, as it does today. Those are step 08's `Member`
and `Write`.

### Yours

- `Fp::Instance`, `Fp::Opaque`, `Fp::Bound`, `fp_depth` (from step 02.07,
  now counting attributes too, and its comment), and `env` threaded through
  `key`, `fingerprint`, `fp_one`, `fp_free` and `fp_defaults`, with the four
  callers of decision 1.
- `Env::attrs_of`.
- `Path`, `Root`, `Step` (with `Default`), `OriginKey::Entry { act, path }` in
  `from_fp` and the annotation fallback, and `materialize` for objects, bound
  methods, and objects inside closures.
- Summary-mode method calls: the receiver through `call_project` and
  `bind_for_callee`.
- Constructors in summary mode, `Target::Construct`, and `render` for it.
- `reaches_pending` in `needs_defer`, and `Walk.now` read by `fp_one`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/env.rs
/// One object's attributes, sorted by name. Scans the flat map.
pub fn attrs_of(&self, origin: OriginId) -> Vec<(Name, Values<'db>)> {
    let mut out: Vec<(Name, Values<'db>)> = self
        .attrs
        .iter()
        .filter(|((o, _), _)| *o == origin)
        .map(|((_, n), v)| (n.clone(), v.clone()))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}
```

```rust
// src/interp/key.rs: the new arms of fp_one (a Solver method, as today)
fn fp_one(&mut self, w: &Walk<'db>, env: &Env<'db>, file: ProgramFile<'db>,
          v: AbstractValue<'db>, depth: u8, cl: &mut ClosureStack<'db>) -> FpId {
    let fp = match v {
        AbstractValue::Instance { class, origin }
            if !origin.holds_state() || depth == self.opts.limits.fp_depth => Fp::Opaque(class),
        AbstractValue::Instance { class, origin } => Fp::Instance {
            class,
            attrs: env
                .attrs_of(origin)
                .into_iter()
                .map(|(name, vals)| (name, self.fingerprint(w, env, file, &vals, depth + 1, cl)))
                .collect(),
        },
        AbstractValue::BoundMethod { func, receiver } => Fp::Bound {
            func,
            recv: self.fp_one(w, env, file, w.values[receiver], depth + 1, cl),
        },
        // a replay: this round's value (decision 8)
        AbstractValue::Pending(id) => match &w.now {
            Some(now) => return self.fingerprint(w, env, file, &now[id], depth, cl),
            None => {
                debug_assert!(false, "a pending value reached a key: reaches_pending missed it");
                Fp::Unknown
            }
        },
        // Function, Class, Module, Ty, Unknown, Closure: as in part 02
        // …
    };
    self.fps.intern(fp)
}
```

```rust
// src/interp/key.rs: materializing one fingerprint at a path (a Walk method, as today)
fn from_fp(&mut self, sol: &Solver<'db>, fp: FpId, at: Path,
           cl: &mut Vec<(Definition<'db>, ActivationId)>, env: &mut Env<'db>) -> Values<'db> {
    match &sol.fps[fp] {
        Fp::Instance { class, attrs } => {
            let origin = self.origins.intern(OriginKey::Entry { act: self.frame.act, path: at.clone() });
            for (name, a) in attrs {
                let v = self.from_fp(sol, *a, at.attr(name), cl, env);
                env.bind_attr(origin, name, v, false); // weak: a union shares this origin
            }
            smallvec![AbstractValue::Instance { class: *class, origin }]
        }
        Fp::Opaque(class) => {
            let origin = self.origins.intern(OriginKey::Entry { act: self.frame.act, path: at });
            smallvec![AbstractValue::Instance { class: *class, origin }]
        }
        Fp::Bound { func, recv } => {
            let r = self.from_fp(sol, *recv, at.recv(), cl, env);
            r.into_iter()
                .map(|r| AbstractValue::BoundMethod { func: *func, receiver: self.values.intern(r) })
                .collect()
        }
        Fp::Union(ids) => {
            let mut out = Values::new();
            for &i in ids {
                out.extend(self.from_fp(sol, i, at.clone(), cl, env));
            }
            out
        }
        Fp::Closure { func, free, defaults } => {
            let activation = self.activations.push(Env::flat());
            cl.push((*func, activation));
            for (n, (name, fp)) in free.iter().enumerate() {
                let v = self.from_fp(sol, *fp, at.free(n as u8), cl, env); // attrs → env
                self.activations[activation].bind(name, v);
            }
            let d = defaults
                .iter()
                .enumerate()
                .map(|(n, (name, fp))| (name.clone(), self.from_fp(sol, *fp, at.default(n as u8), cl, env)))
                .collect();
            self.defaults.insert((*func, activation), d);
            cl.pop();
            smallvec![AbstractValue::Closure { func: *func, activation }]
        }
        // Function, Class, Module, Data, Unknown, Back: as in part 02
        // …
    }
}
```

`materialize` calls it with `Path::root(Root::Param(i))` for each parameter,
`Root::Free(i)` and `Root::Default(i)` for the walked closure's own lists, and
`&mut env`, the env it returns.

```rust
// src/interp/call.rs: decision 8, half 1
/// Would keying these values meet a `Pending`? Looks exactly as far as `fingerprint`.
fn reaches_pending(&self, sol: &Solver<'db>, vals: &Values<'db>, env: &Env<'db>, depth: u8) -> bool {
    let limit = sol.opts.limits.fp_depth;
    vals.iter().any(|v| match *v {
        AbstractValue::Pending(_) => true,
        AbstractValue::Instance { origin, .. } if origin.holds_state() && depth < limit => env
            .attrs_of(origin)
            .iter()
            .any(|(_, a)| self.reaches_pending(sol, a, env, depth + 1)),
        AbstractValue::BoundMethod { receiver, .. } => {
            self.reaches_pending(sol, &smallvec![self.values[receiver]], env, depth + 1)
        }
        AbstractValue::Closure { func, activation } if depth < limit => {
            sol.sem.free_variables(func).iter().any(|n| {
                self.reaches_pending(sol, &self.lookup_in(sol, func, activation, n), env, depth + 1)
            }) || self
                .defaults
                .get(&(func, activation))
                .into_iter()
                .flatten()
                .any(|(_, d)| self.reaches_pending(sol, d, env, depth + 1))
        }
        _ => false,
    })
}

fn needs_defer(&self, sol: &Solver<'db>, callees: &Values<'db>,
               args: &EvaluatedArgs<'db>, env: &Env<'db>) -> bool {
    if self.deferred.is_empty() {
        return false; // a Pending is an index into `deferred`: none exist yet
    }
    if callees.iter().any(|c| matches!(c, AbstractValue::Pending(_))) {
        return true;
    }
    let project: SmallVec<[AbstractValue<'db>; 2]> = callees
        .iter()
        .copied()
        .filter(|c| match *c {
            AbstractValue::Function(d) | AbstractValue::Class(d) => sol.sem.is_project(d),
            AbstractValue::Closure { func, .. } | AbstractValue::BoundMethod { func, .. } => {
                sol.sem.is_project(func)
            }
            _ => false,
        })
        .collect();
    if project.is_empty() {
        return false;
    }
    let arg = args.positional.iter().chain(args.keywords.iter().map(|(_, v)| v))
        .any(|v| self.reaches_pending(sol, v, env, 0));
    arg || project.iter().any(|c| match *c {
        // its own free values and defaults sit at depth 1, as `key` reads them
        AbstractValue::Closure { .. } => self.reaches_pending(sol, &smallvec![*c], env, 0),
        // the receiver is slot 0
        AbstractValue::BoundMethod { receiver, .. } => {
            self.reaches_pending(sol, &smallvec![self.values[receiver]], env, 0)
        }
        _ => false,
    })
}
```

`replay` (`defer.rs`) sets `self.now = Some(self.resolved.clone())` where it
made the local, writes each result to `self.now`, and takes it back at the end.

```rust
// src/interp/call.rs: call_values, the two loops in summary mode
for (func, recv) in methods {
    match sol.opts.mode {
        Mode::Inline => { /* as today */ }
        Mode::Summary => out.extend(
            self.call_project(sol, func, None, Some(recv), args, &mut edge_targets, &*env),
        ),
    }
}
for class in classes {
    let range = self.site_range(site);
    match sol.opts.mode {
        Mode::Inline => { /* as today */ }
        Mode::Summary => out.extend(
            self.construct_summary(sol, class, range, args, &mut edge_targets, &*env),
        ),
    }
}
```

```rust
// src/interp/call.rs
/// Summary mode: a new object here, and `__init__` keyed on it.
fn construct_summary(&mut self, sol: &mut Solver<'db>, class: Definition<'db>, site: TextRange,
                     args: &EvaluatedArgs<'db>, targets: &mut SmallVec<[Target<'db>; 2]>,
                     env: &Env<'db>) -> Values<'db> {
    let origin = self.origins.intern(OriginKey::New { site, act: self.frame.act });
    let obj = AbstractValue::Instance { class, origin };
    let init = sol.sem.init_of(class).filter(|&i| sol.sem.is_project(i)).and_then(|init| {
        let parsed = sol.sem.parsed(init);
        let node = sol.sem.function_node(init, &parsed)?;
        let bound = self.bind_for_callee(sol, init, node, Some(obj), args, None);
        let ctx = sol.key(self, init, &bound, None, env);
        sol.get(ctx); // its value is never used; its writes are effects (step 03)
        Some(ctx)
    });
    targets.push(Target::Construct { class, init });
    if args.has_empty() { Values::new() } else { smallvec![obj] }
}
```

```rust
// src/interp/render.rs
/// The definition a target's node is named by, and the context under it.
fn callee_of<'db>(s: &Solver<'db>, t: &Target<'db>) -> (Definition<'db>, Option<ContextId>) {
    match *t {
        Target::Call(ctx) => (s.contexts[ctx].func, Some(ctx)),
        Target::Construct { class, init } => (class, init),
    }
}
// sites_union: Vec<(SiteId, SmallVec<[Target<'db>; 2]>)>
// group_by_function: groups by callee_of(..).0, collecting the Some contexts
// render: children = if ctxs.is_empty() { vec![] } else { render(s, &ctxs, path) }
```

</details>

---

## Run

```rust
#[test]
fn p04_01_in_keys() {
    check_both("04-class-summaries/python/in_keys.py");
    let st = stats_for("04-class-summaries/python/in_keys.py", "shared", &summary());
    assert_eq!((st.walks, st.reuse_hits), (3, 1));
}
```

`check_both` from now on: every fixture of this part runs in both modes.

Then run the whole suite. Methods and constructors used to go to ty in
summary mode. Now they are summaries, so a part 02 or 03 fixture that runs in
both modes and calls a method can change its summary-mode tree or its counts.
A change that matches the inline tree is the step working. Any other change
is a bug, or a difference that belongs in step 09's list.

---

## Where you are

```
Fp        += Instance { class, attrs }, Opaque(class), Bound { func, recv }
OriginKey  = Opaque | Entry { act, path } | New { site, act }
Path       = Root (Param | Free | Default) + Steps (Attr | Free | Default | Recv)
Target    += Construct { class, init }
needs_defer looks as deep as the key; a replay keys with this round's values
```

A method called on an object works in summary mode, when the caller wrote the
object's state. Objects can go into a call. Step 02 brings them back out.

---

## Exercise

1. `b.handler = b` (an object that holds itself). Write its fingerprint at
   `fp_depth = 2`.
2. Two `Box` objects with *different* handlers are both passed to `use`.
   How many `use` contexts, how many `Box.run` contexts?
3. In `shared`, the second `b.run()` is reused. If `Box.run` wrote
   `self.count = 1`, would reusing it be wrong? (Step 03 answers it. Guess
   first.)
4. In `again`, drop half 2 of decision 8 and keep half 1. What does the tree
   show, and is it marked?

<details><summary>Answers</summary>

1. `Instance { Box, [handler: Instance { Box, [handler: Opaque(Box)] }] }`:
   depth 0 is the argument, depth 1 its `handler`, and at depth 2 the limit
   makes it `Opaque`. The self-reference ends there instead of looping.
2. Two of each: the two `b`s have different fingerprints, so `use` gets two
   keys, and each `use` walk calls `run` with a different `self`.
3. Not wrong, as long as the summary reports the write in terms of its input
   ("parameter 0's `count` is now 1") and each caller applies it to *its own*
   object. That is exactly what an effect is.
4. `Runner.run` with no child, unmarked. The call is deferred, but every
   replay keys `r.cb` from `d.before`, where it is still `Pending(0)`.
   `fp_one` turns that into `Unknown` (the `debug_assert` fires in a test
   build). The fixpoint converges on a summary with no `cb`. Deferring only
   helps if the replay can see the resolved value.

</details>

---

→ Next: [`02-objects-coming-back.md`](02-objects-coming-back.md)
