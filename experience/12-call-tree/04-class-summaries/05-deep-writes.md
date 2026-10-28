# 04.05 — Deep writes

**You build:** writes to input objects the key didn't reach. When a walk reads
an attribute of an input object that its key didn't hold, the result is still
named by its path: `Entry(Param(0).app.config.sink)`. A write to it is an
effect at that path, and the caller follows the path in its own state. Paths
are capped at `max_path`; past it, a write becomes a weak `Unknown`, and the
summary is marked.
**Fixtures:** `python/deep_write.py`, `python/deep_write_capped.py`
(`max_path = 2`)
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Objects the key
does not reach", `09-summaries.md` § "Finishing a walk" (point 3)

---

## The fixture

```python
class Sink:    def __init__(self, h): self.h = h
class Config:  def __init__(self, sink: Sink): self.sink = sink
class App:     def __init__(self, config: Config): self.config = config
class Service: def __init__(self, app: App): self.app = app

def rewire(svc: Service):
    svc.app.config.sink.h = JsonHandler()

def main():
    svc = Service(App(Config(Sink(CsvHandler()))))
    rewire(svc)
    svc.app.config.sink.h.process()     # Python: JsonHandler.process
```

`deep_write_capped.py` is the same code with `# limits: max_path=2`: there,
summary mode keeps `CsvHandler.process` and marks `rewire` `[incomplete]`.

## Predict

1. Write `rewire`'s key with `fp_depth = 2`. Which object is the last one it
   describes?
2. In `rewire`'s walk, what is `svc.app.config.sink`, with your step 04 code?
   What happens to the write?
3. What would `main`'s tree show if the write were silently dropped? Is that
   an imprecise tree or a wrong one?

---

## Build

### Decision 1: an input object the key didn't reach is still named by its path

`rewire`'s key: `svc = Instance { Service, [app: Instance { App, [config: Opaque(Config)] }] }`.
In the walk, `svc.app.config` is `Instance { Config, Entry(Param(0).app.config) }`
with no attributes. `.sink` misses the state and goes to the class side, where
ty knows `Config.sink: Sink` (from the annotation in `__init__`). That lifts to
`Instance { Sink, OPAQUE }`, and the write to its `h` is dropped. `main` keeps
`CsvHandler.process`: an edge Python never takes, and the one it does take is
missing. Nothing is marked.

So `member` gets a third step:

```rust
// member(), the Instance arm, after the class side answered `found`
// 3. an input object's attribute the key didn't hold: name it by its path
fn track_entry_attr(&mut self, recv_origin: OriginId, name: &Name, found: Values<'db>) -> Values<'db> {
    let OriginKey::Entry(path) = &self.origins[recv_origin] else { return found };
    found.into_iter().map(|v| match v {
        AbstractValue::Instance { class, origin } if origin == OriginId::OPAQUE => {
            AbstractValue::Instance { class, origin: self.entry_or_beyond(path.attr(name)) }
        }
        other => other,
    }).collect()
}
```

Now `svc.app.config.sink` is `Instance { Sink, Entry(Param(0).app.config.sink) }`.
The write to `h` is a write to an input object, recorded in `written`, and
`finish` reports it as the effect
`Param(0).app.config.sink.h = [Obj(0)]`, strong. `main` follows
`svc → app → config → sink` in its own state, finds its real `Sink`, and
writes `h`.

**Why this keeps rule 1:** `rewire` never reads `main`'s deep values. The
class comes from ty, which answers the same for every caller, and the path is
a *location*, not a value. Two callers with equal keys get the same summary,
and each follows the path to its own object.

**What it does not fix:** reads. A read at that depth still gets only the
class from ty, not the caller's object state, which is why `deep` in step 01
stays a known limit. Writes reach the caller by path; reads need the key.

### Decision 2: paths are capped, and a write past the cap is marked

`max_path` (6) bounds the steps in an `Entry` origin, a `Ref`, and an effect
target. Past it, the object gets:

```rust
pub enum OriginKey {
    Entry(Path),
    Beyond { at: Path, attr: Name },   // deeper than max_path: `at` is the deepest path that fits
    New { … },
}
```

A `Beyond` object holds no state, and every object below it shares its origin.
A write to it becomes the **fallback effect**: a weak `[Unknown]` at
`at.attr`, the deepest place the summary can still name. The summary gets the
`DEEP_WRITE` gap, and `Stats.deep_write_fallbacks` counts it.

In `deep_write_capped.py` (`max_path = 2`), `Param(0).app.config` is the
deepest path that fits, so the write to `.sink.h` becomes
`Param(0).app.config.sink = [Unknown]`, weak. `main` sees "the old `Sink`, or
something unknown": `.h` on the old `Sink` gives `CsvHandler`, on `Unknown`
gives nothing. So `main` keeps `CsvHandler.process`, gains nothing, and
`rewire` is marked `[incomplete]`.

**Why not drop it:** dropping gives the same tree with no mark: silently wrong.
**Why not write deeper paths:** a summary must stay finite and small, like a
key. **Why weak `Unknown` and not strong:** the summary doesn't know *which*
object below `at.attr` changed, so it can only say that what is there may now
be something else.

`max_path` is a precision limit: it depends only on the code (how deep the
walk reaches), so a `DEEP_WRITE` summary is still reusable. It must be at
least `fp_depth`, since materializing a key already creates `Entry` paths that
deep.

### Yours

- `track_entry_attr` as `member`'s step 3, for `Instance` receivers with an
  `Entry` origin.
- `OriginKey::Beyond`, `entry_or_beyond(path)`, `max_path` in `Limits`.
- `bind_attribute` on a `Beyond` receiver: record the fallback.
- `finish`: the fallback effect, `Gaps::DEEP_WRITE`, and
  `Stats.deep_write_fallbacks`.

<details><summary>Reference: this step's code</summary>

```rust
fn entry_or_beyond(&mut self, path: Path, max_path: u8) -> OriginId {
    if path.steps.len() <= max_path as usize {
        return self.origins.intern(OriginKey::Entry(path));
    }
    // the deepest prefix that fits, and the first step past it
    let at = path.prefix(max_path as usize);
    let attr = path.steps[max_path as usize].as_attr().expect("only Attr steps go this deep").clone();
    self.origins.intern(OriginKey::Beyond { at, attr })
}
```

```rust
// bind_attribute: the receivers' origins, sorted out
for r in &recvs {
    match r {
        AbstractValue::Instance { origin, .. } if self.holds_state(*origin) => targets.push(*origin),
        AbstractValue::Instance { origin, .. } if self.is_beyond(*origin) => self.deep_writes.push(*origin),
        _ => {}                                      // OPAQUE, Unknown, Ty: dropped
    }
}
```

```rust
// finish: one fallback effect per Beyond origin written to
for &b in &self.deep_writes {
    let OriginKey::Beyond { at, attr } = &self.origins[b] else { unreachable!() };
    effects.push(Effect { target: at.clone(), attr: attr.clone(),
                          value: smallvec![SVal::Plain(PlainVal::Unknown)], strong: false });
    gaps |= Gaps::DEEP_WRITE;
    s.stats.deep_write_fallbacks += 1;
}
```

</details>

---

## Run

```rust
#[test]
fn p04_05_deep_writes() {
    check_both("04-class-summaries/python/deep_write.py");
    check_both("04-class-summaries/python/deep_write_capped.py");
}
```

---

## Where you are

```
member(Instance): state → property → class side → named by path, if the receiver is an input
OriginKey = Entry(Path) | Beyond { at, attr } | New { … }
Gaps      = CYCLE | DEEP_WRITE
```

---

## Exercise

1. `rewire` with `max_path = 3`. Is `Param(0).app.config.sink` within the
   cap? What does `main` show?
2. Why does `track_entry_attr` only rename `OPAQUE` instances, and not, say, a
   function value found on the class side?
3. With `max_path = 2`, a walk writes both `svc.app.config.sink.h` and
   `svc.app.config.sink.g`. How many fallback effects does the summary get?

<details><summary>Answers</summary>

1. Yes: `app`, `config`, `sink` are three steps. It is an `Entry` origin, the
   write is a strong effect at that path, and `main` shows
   `JsonHandler.process`, as with the default cap of 6.
2. An `OPAQUE` instance is exactly "an object of this class, identity
   unknown". Under an input object, its identity *is* known: it is whatever
   the caller has at that path. A function or a class found on the class side
   has no identity to name: it is the same value for every caller.
3. One per `Beyond` origin written to. Both writes go through the same
   `Beyond { at: Param(0).app.config, attr: sink }` origin (every object below
   `at.attr` shares it), so one fallback effect.

</details>

---

→ Next: [`06-split-by-type.md`](06-split-by-type.md)
