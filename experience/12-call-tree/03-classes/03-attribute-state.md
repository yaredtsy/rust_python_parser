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
    p.run(1)                                      # one Pipeline.run node, both process calls
```

`read_json` / `read_csv` is the fixture the whole design exists for: one
method, `run`, whose callee depends on what the object it runs on was built
with.

## Predict

1. Run it with your step 02 code. What does `Pipeline.run` show, and why?
2. `__init__`'s write happens in `__init__`'s frame. `run` is called from
   `read_json`'s frame, after `__init__` has returned. Where does the write
   have to be, for `run` to see it?
3. In `either`, `run` is walked on two objects. How many `Pipeline.run` nodes
   should there be?

---

## Build

### Decision 1: state lives in the env, keyed by (origin, name)

```rust
pub struct Env<'db> {
    pub locals: rpds::HashTrieMap<Name, Values<'db>>,
    pub attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,   // ← new
    pub scope: BodyScope<'db>,
    pub enclosing: Option<ActivationId>,
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
AbstractValue::Instance { class, origin } => {
    if let Some(v) = env.attrs.get(&(origin, name.clone())) {
        return v.clone();                                  // 1. this object, on this path
    }
    // 2. the class side, through ty: step 02
}
```

**The order is the design.** What this path wrote on this object beats what
the class declares, which beats ty's general answer. Each step down trades
precision for coverage.

Tracing `self.handler.process(data)` in `run`: `self` is the object. `.handler`
hits step 1: `__init__` wrote it. `.process` is a second `member` call, on the
`JsonHandler` object, and hits step 2: `JsonHandler.process`.

### Decision 3: a write goes to every object the receiver may be

```rust
fn bind_attribute(&mut self, s: &mut Solver<'db>, attr: &ExprAttribute, v: Values<'db>, env: &mut Env<'db>) {
    let recvs = self.eval(s, &attr.value, env);       // `a.b.c = v`: the receiver is `a.b`
    let targets: SmallVec<[OriginId; 2]> = recvs.iter()
        .filter_map(|r| match r { AbstractValue::Instance { origin, .. } => Some(*origin), _ => None })
        .collect();
    let strong = targets.len() == 1;                  // exactly one object: replace
    for origin in targets {
        env.bind_attr(origin, &attr.attr.id, v.clone(), strong);
    }
}
```

`bind_target` (step 01.03) calls it for `Expr::Attribute` targets.

- **Only the last name is written.** `a.b.c = X()` evaluates `a.b` (two reads)
  and writes `c` on whatever `a.b` is.
- **One object: replace. Several: add.** If the receiver may be two objects,
  Python wrote only one of them, so each gets "old value or new value". This
  is a **weak** write. Step 05 tests it.
- **No object: nothing is written.** A write to `Unknown` or to `ty`'s answer
  is dropped. Guessing which object changed would corrupt unrelated paths.

### Decision 4: state flows into a call, and back out of it

A callee starts with the caller's state, and the caller continues with the
callee's state at its exits:

```rust
// walk_inline
let mut start = self.env_from_bound(s, def, node, &bound, enclosing);
start.attrs = env.attrs.clone();                 // the callee sees every object as it is now
// … walk; the frame records the env at every `return`, and at fall-through …
if let Some(exit) = Env::join(callee.exits) {
    env.attrs = exit.attrs;                       // the caller continues with what the callee left
}
```

`Frame` gains `exits: Vec<Env>`: the env at each `return` and at the end of
the body. `walk_inline` takes the caller's env (`&mut Env`, as `eval` has
since step 01.03), which is how a constructor's writes reach `read_json`.

If every path of the callee raises, there are no exits, and the caller's state
is left as it was.

**Several callees at one site are alternatives, not a sequence.** In `either`,
`p.run` is `run` on two objects, and Python makes one of those calls, not
both. So each callee starts from the **same** state, the one before the call,
and the caller continues with the join of what they left:

```rust
// eval_call, inline mode, once the arguments are evaluated
let before = env.clone();                         // a pointer copy
let mut after = Vec::new();
for callee in &callees {
    let mut e = before.clone();                   // every callee starts here
    // … walk_inline(…, &mut e) as above, for this callee …
    after.push(Some(e));
}
if let Some(joined) = Env::join(after) {
    *env = joined;                                // one callee: exactly its state
}
```

Walking them one after another on the same `env` would let the second callee
see the first one's writes, and the tree would depend on the order the
callees happen to be walked in (step 01.09 sorts them, but that order is for
the nodes, not for the state). Part 04 has the same rule in summary mode: one
snapshot per call expression (step 04.04, decision 3).

**This is why inline mode is the easy case.** One walk holds every object, and
state just flows along the calls. Part 04 has to say the same thing in a
summary that knows nothing about its caller's objects: "parameter 0's
`handler` is now parameter 1".

### Decision 5: one node per (site, function), even with several receivers

In `either`, `p` is two objects, so `p.run` is two bound methods of the same
function, and inline mode walks `run` twice. They are one call in the source,
so they make **one** node. Its children are the merge of both walks' children:
a child with the same site and name as one already there merges into it,
recursively; a new one is added, and the nodes of each site are kept in
definition order (step 01.09).

Summary mode's renderer does the same thing with a union of contexts (step
02.03), so this keeps the two modes' trees equal. It could already happen in
part 01, with two closures of one `def` at one call site. Objects make it
common.

### Yours

- `Env.attrs`, `bind_attr`, and `attrs` in `Env::join`.
- `bind_attribute`, called from `bind_target`.
- The state step in `member`.
- `Frame.exits`, and the two lines in `walk_inline`.
- Merging nodes per (site, function) in inline mode.

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
}
```

```rust
// src/interp/tree.rs
/// Add `new` to `nodes`, merging with a node of the same site and name.
pub fn merge_into(nodes: &mut Vec<Node>, new: Node) {
    match nodes.iter_mut().find(|n| n.site == new.site && n.qname == new.qname) {
        Some(existing) => {
            for child in new.children {
                merge_into(&mut existing.children, child);
            }
            for m in new.marks {
                if !existing.marks.contains(&m) { existing.marks.push(m) }
            }
        }
        None => nodes.push(new),
    }
}
```

Every place that pushed a node onto `frame.children` now calls
`merge_into(&mut self.frame.children, node)`.

</details>

---

## Run

```rust
#[test]
fn p03_03_state() {
    check("03-classes/python/state.py");
}
```

---

## Where you are

```
Env   { locals, attrs: (OriginId, Name) → Values, scope, enclosing }
Frame { …, exits: [Env] }
member(Instance): the object's state, then the class (ty)
bind_attribute: strong for one object, weak for several, dropped for none
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
