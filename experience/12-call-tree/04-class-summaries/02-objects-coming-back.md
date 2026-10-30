# 04.02 — Objects coming back

**You build:** returning objects from a summary. A returned object is the
caller's own object, named by its path among the inputs (`Ref`), or a new
object the callee built, with its final attributes (`Obj`). The caller turns
each back into an object of its own walk.
**Fixture:** `python/coming_back.py`
**Plan:** `plan/03-call-tree/09-summaries.md` § "Values relative to the
inputs", § "Finishing a walk", `10-one-call.md` § "Phase 1"
**Read with it:** `00-values/09-the-four-conversions.md`

---

## The fixture

```python
def ident(x): return x
def make_box(h):
    b = Box(); b.handler = h; return b
def make_json():
    b = Box(); b.handler = JsonHandler(); return b

def identity():
    b = Box(); b.handler = CsvHandler()
    c = ident(b)
    c.handler = JsonHandler()     # c IS b
    b.run()                       # JsonHandler.process
def built_inside():
    make_json().run()             # JsonHandler.process
def built_with_arg():
    make_box(CsvHandler()).run()  # CsvHandler.process
def two_from_one_site():
    a = make_box(JsonHandler())
    b = make_box(CsvHandler())
    a.run()                       # JsonHandler.process only
    b.run()                       # CsvHandler.process only
```

## Predict

1. `ident`'s walk returns its parameter. If its summary said "an `Instance {
   Box, [handler: CsvHandler] }`" (the fingerprint), and the caller built a new
   object from that, what would `b.run()` in `identity` show?
2. `make_json` builds two objects. Which of them must its summary describe,
   and how does the caller learn that the box's `handler` is the other one?
3. `two_from_one_site` calls `make_box` twice from one function. Do the two
   returned boxes get one origin or two in `two_from_one_site`'s walk?

---

## Build

### Decision 1: a returned object is "yours at this path", "new", or "opaque"

```rust
pub enum SVal<'db> {
    Plain(PlainVal<'db>),
    Ref(Path),                                         // ← new: the caller's object at this path, when the call started
    Obj(u16),                                          // ← new: objects[j], built in this call
    Bound { func: Definition<'db>, recv: Box<SVal<'db>> },   // ← new
    Closure { func: Definition<'db>, free: Box<[(Name, SVals<'db>)]>,   // step 02.07
              defaults: Box<[(Name, SVals<'db>)]> },
    Back(u8),                                                           // step 02.07
}

pub enum PlainVal<'db> { /* … */ Opaque(Definition<'db>) }   // an object with no tracked identity

pub struct NewObject<'db> {
    pub class: Definition<'db>,
    pub site: SiteId,                                  // where it was built, in this function
    pub attrs: Vec<(Name, SVals<'db>)>,               // its final state, sorted
}

pub struct FunctionResult<'db> {
    pub calls: Vec<CallEdge<'db>>,
    pub returns: SVals<'db>,
    pub objects: Vec<NewObject<'db>>,                  // ← new: objects built here that escape
    pub status: Status,
}
```

`finish` converts by origin:

| value in the walk | in the summary |
|---|---|
| `Instance { origin: Entry(path) }` | `Ref(path)` |
| `Instance { origin: New { … } }` | `Obj(j)`, and the object goes into `objects` |
| `Instance { origin: OPAQUE }` | `Plain(Opaque(class))` |
| `BoundMethod { func, receiver }` | `Bound { func, recv: <receiver, converted> }` |
| anything else | as in part 02 |

**Why not the fingerprint:** identity. `ident(b)` must give back `b` itself.
If the caller got a new object built from the fingerprint, `c` and `b` would
be two objects, `c.handler = JsonHandler()` would land on `c`, and `b.run()`
would show `CsvHandler.process`. A wrong edge, with nothing to mark it.

**Why `Ref` is enough for identity:** the callee doesn't know which object the
caller passed, but it knows *where* among the inputs it was: "parameter 0".
The caller knows which object that was. The path is the handshake.

### Decision 2: a new object carries its final state, and only if it escapes

`make_json` builds a `Box` and a `JsonHandler`, and returns the box. Its
summary:

```
returns: [Obj(0)]
objects: [0] Box         attrs: [handler: [Obj(1)]]
         [1] JsonHandler attrs: []
```

`objects` holds every new object **reachable** from what leaves the call: the
returns, and (step 03) effects. An object built and dropped inside the call is
not described. `finish` walks out from the returns, following attributes, and
numbers objects as it finds them, so equal walks give equal summaries.

`make_box(h)`'s box holds `handler: [Ref(Param(0))]`: the caller's argument,
whatever it was. One summary serves both of `two_from_one_site`'s calls,
because the summary never names the handler, only where it came from.

### Decision 3: the caller resolves every `SVal` into its own walk

```rust
fn resolve_sval(&mut self, s: &mut Solver<'db>, sv: &SVal<'db>, call: &CallSite<'db>, before: &Env<'db>) -> Values<'db> {
    match sv {
        SVal::Ref(path) => self.follow(s, path, call, before),          // your object at that path
        SVal::Obj(j) => {
            let origin = self.origins.intern(OriginKey::New {
                site: call.site, act: self.frame.act, made: Made::Obj(*j),
            });
            smallvec![AbstractValue::Instance { class: call.result.objects[*j as usize].class, origin }]
        }
        SVal::Bound { func, recv } => /* resolve recv, bind func to each */,
        SVal::Closure { .. } => /* step 02.07 */,
        SVal::Plain(p) => smallvec![p.to_value()],                       // Opaque(c) → Instance { c, OPAQUE }
    }
}
```

- **`Ref(path)`**: start at the root (`Param(i)` is the `i`-th bound argument
  of this call), then follow each step: `Attr` reads the attribute as `member`
  would, `Recv` takes a bound method's receiver, `Free` a closure's free value.
- **`Obj(j)`**: a new origin in the caller, keyed by this call site and `j`.
  Then each of the object's attributes is resolved and written into the
  caller's state.

`OriginKey::New` gains `made: Made`, either `Instance` (a constructor at this
site) or `Obj(j)` (a callee's object `j`). Step 06 adds two more fields.

**Why the call site in the origin:** `two_from_one_site` calls `make_box` from
two sites, so the two boxes get two origins, and their handlers stay apart. A
site is evaluated once per walk, so one site plus one `j` always means one
object.

**Why `before`:** a `Ref` means "the object at this path **when the call
started**". Step 04 shows what goes wrong if it is read after the call's own
writes. For now, read every path from the env as it was before the call: the
`before` snapshot, a pointer copy.

### Yours

- `SVal::Ref`, `Obj`, `Bound`, `PlainVal::Opaque`, `NewObject`,
  `FunctionResult.objects`.
- `finish`: the conversion table and the reachable-objects walk.
- `resolve_sval`, `follow`, `Made`, and writing a new object's attributes into
  the caller's env.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/summary.rs: finishing, the conversion
fn to_sval(&mut self, v: AbstractValue<'db>, exit: &Env<'db>, objs: &mut ObjectNumbering<'db>) -> SVal<'db> {
    use AbstractValue::*;
    match v {
        Instance { class, origin } => match &self.origins[origin] {
            OriginKey::Entry(path) => SVal::Ref(path.clone()),
            OriginKey::New { site, .. } => SVal::Obj(objs.number(origin, class, *site)),
            _ => SVal::Plain(PlainVal::Opaque(class)),        // OPAQUE (and Beyond, step 05)
        },
        BoundMethod { func, receiver } => SVal::Bound {
            func,
            recv: Box::new(self.to_sval(self.values[receiver], exit, objs)),
        },
        // Closure, Function, Class, Module, Ty, Unknown: as in part 02
        other => SVal::Plain(plain(other)),
    }
}

/// Give each New origin a number the first time it is reached, then describe
/// its attributes, which may reach more New origins.
fn describe_objects(&mut self, exit: &Env<'db>, objs: &mut ObjectNumbering<'db>) -> Vec<NewObject<'db>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < objs.len() {
        let (origin, class, site) = objs.get(i);
        let attrs = exit.attrs_of(origin)
            .map(|(name, vals)| (name.clone(), vals.iter().map(|v| self.to_sval(*v, exit, objs)).collect()))
            .collect();
        out.push(NewObject { class, site, attrs });
        i += 1;
    }
    out
}
```

The exit env is the join of `frame.exits` (step 03.03).

</details>

---

## Run

```rust
#[test]
fn p04_02_coming_back() {
    check_both("04-class-summaries/python/coming_back.py");
}
```

---

## Where you are

```
SVal = Plain | Ref(Path) | Obj(j) | Bound | Closure
FunctionResult { calls, returns, objects: [NewObject { class, site, attrs }], status }
OriginKey::New { site, act, made: Instance | Obj(j) }
```

Objects go into calls and come back out, keeping their identity. The one thing
left for part 03's fixtures is a write the callee makes on an object the
caller can see.

---

## Exercise

1. `def pair(a, b): return a if c else b`. Write the summary's returns.
2. `def f(x): y = Box(); y.inner = x; return y.inner`. What does `f` return,
   `Ref` or `Obj`?
3. A callee builds a `Box`, stores it in a module global, and returns nothing.
   Is the box in `objects`? Why is that consistent with step 03.04?

<details><summary>Answers</summary>

1. `[Ref(Param(0)), Ref(Param(1))]`: two alternatives, each "your object at
   that parameter".
2. `Ref(Param(0))`: `y.inner` is the value of `x`, which is the input object
   at parameter 0. The `Box` doesn't escape, so it is not in `objects`.
3. No: it is not reachable from the returns or (later) the effects. Writes to
   module globals are dropped (step 03.04), so nothing the caller could reach
   holds it.

</details>

---

→ Next: [`03-effects.md`](03-effects.md)
