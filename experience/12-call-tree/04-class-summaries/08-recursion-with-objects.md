# 04.08 — Recursion with objects

**You build:** part 02's recursion, for objects. A pending value can now be an
object, so reading its attribute and writing to it become deferred operations
too. And one new rule makes the fixpoint stop: an object doesn't keep its
identity around a cycle.
**Fixture:** `python/recursion_objects.py`
**Plan:** `plan/03-call-tree/12-recursion.md` (all of it, with objects),
`15-checks-and-build-order.md` § "Known limits" (identity, `Tree.fill`)

---

## The fixture

```python
def walk(node):
    r = walk(node)
    r.close()                   # summary: Handle.close
    return Handle()

class Node:
    left: "Node"
    def accept(self, v: "Visitor"): v.visit(self)
class Visitor:
    def visit(self, node: Node):          self.visit_node(node)
    def visit_node(self, node: Node):     self.visit_children(node)
    def visit_children(self, node: Node):
        node.left.accept(self)            # a cycle of four
        self.found = Report()
def visitor(root: Node):
    v = Visitor()
    root.accept(v)
    v.found.emit()              # Report.emit

def build(n, h):
    if n: return Branch(build(n - 1, h))
    return Leaf(h)
def identity_limit():
    build(1, JsonHandler()).child.h.process()    # Python: JsonHandler.process. Here: nothing

class Tree:
    def fill(self, n, h):
        if n:
            self.fill(n - 1, h)
            self.h.process()    # Python: JsonHandler.process. Here: nothing
        self.h = h
```

## Predict

1. In `walk`, `r` is pending. `r.close` is an attribute read on it. With part
   02's operations (`Apply`, `Call`), how would you record it?
2. `build`'s round 0 returns a `Branch` whose `child` holds nothing yet, and a
   `Leaf`. In round 1, `child` gets the objects of round 0's summary. In round
   2? What happens if each round's objects get new origins?
3. In `visitor`, `self.found = Report()` runs in `visit_children`, which is in
   a cycle with `accept`. How does the write reach `visitor`'s `v`?

---

## Build

### Decision 1: two more deferred operations

```rust
pub enum DeferredOp<'db> {
    Apply  { ctx: ContextId, at: (SiteId, u8, u8), combo: Combo<'db>, callee: AbstractValue<'db>, strong_ok: bool },
    Call   { site: SiteId, callee: Values<'db>, args: EvaluatedArgs<'db> },
    Member { base: Values<'db>, name: Name, site: SiteId },   // ← `p.name`, p pending
    Write  { base: Values<'db>, name: Name, value: Values<'db> },   // ← `p.name = v`, p pending
}
```

| use of a pending value `p` | becomes |
|---|---|
| `p.name` | `Member`, value `Pending(new)` |
| `p.name = v` | `Write` |
| `p.m()` | `Member` (the bound method), then `Call` on its pending value |
| `f(x)` where `f`'s key would read `p` **anywhere** | `Call` |

"Anywhere" now goes deep: a pending value inside an argument's attributes (to
`fp_depth`), inside a closure's free values, or as a bound method's receiver,
would be read by `fingerprint`. So the check before keying looks as far as the
key would.

`Apply` now carries what step 02's resolve needs (the site, callee index and
combo, for `Obj` origins) and `strong_ok`.

In `walk`: `walk(node)` is `Apply` → `Pending(0)`. `r.close` is
`Member { base: [Pending(0)], name: close }` → `Pending(1)`. `r.close()` is
`Call { callee: [Pending(1)] }` → `Pending(2)`. Round 1: `Pending(0)` is round
0's `Handle`, `.close` binds `Handle.close` on it, and the call records the
edge.

### Decision 2: writes and effects replayed in the fixpoint are never strong

A replayed operation's writes (`Write`, and the effects of the summaries
`Apply` and `Call` apply) are **joined** into the walk's exit state, never
replacing what is there.

**Why:** the statements after the operation were walked once, in the first
walk, without its result. Its write happened "somewhere in the middle", and
the only place left to put it is the end, alongside what the walk already
had there.

That is the `Tree.fill` known limit: `self.h.process()` runs after the inner
`fill` call, which is pending. The read of `self.h` is concrete (`self` isn't
pending), so it was evaluated in the first walk and saw no `h`. The inner
call's effect `self.h = h` arrives in the fixpoint, at the exit, after the
statement that needed it. The fixpoint repeats operations, not statements.
Inline mode has the same gap for a different reason: it never walks the
recursive call at all. The fixture pins it in both modes. Replaying the
statements after a pending call is listed for later
(`plan/03-call-tree/15` § "Growth path", v1.2).

### Decision 3: an object doesn't keep its identity around a cycle

```python
def build(n, h):
    if n: return Branch(build(n - 1, h))
    return Leaf(h)
```

Round 0: `build` returns `Obj(0)` (a `Branch`, `child` empty) and `Obj(1)` (a
`Leaf`). Round 1: the recursive call resolves round 0's summary, so `child`
holds two objects, which `finish` exports as `Obj(2)` and `Obj(3)` with their
own attributes. Round 2: `child` holds round 1's objects, which hold round 0's…
Each round adds a level. The summary never stops changing.

The rule: **in a member's own summary, an object that came from a member's
summary (its own included) is exported as `Opaque(class)`**, and a closure
that did as `Function(func)` with no captured values. Inside the walk, the
object keeps its identity and attributes as usual; only the summary forgets
them.

Round 1: `child = [Opaque(Branch), Opaque(Leaf)]`. Round 2: the same. Stop.

**What it costs:** `identity_limit`. `build(1, h).child` is
`Opaque(Branch) | Opaque(Leaf)`, so `.h` comes from ty's declared type for
`Leaf.h`, which for an unannotated parameter is `Unknown`. No edge, where
Python finds `JsonHandler.process`. The fixture pins it. Inline mode loses it
too: there the recursive call is `Unknown` from the start.

**Why this terminates:** values only grow, and every value is now bounded:
fingerprints by `fp_depth`, paths by `max_path`, unions by the fan-out caps,
and objects by this rule. `max_rounds` (step 02.06) stays as the safety net.

To know which objects "came from a member's summary", tag their origins:
`New { made: Obj(j), .. }` created while resolving an `Apply` or `Call` to a
group member get a flag, and `finish` exports a flagged origin as `Opaque`.

### Decision 4: the fixpoint compares effects and objects too

`set_current` (step 02.06) now compares `returns`, `effects` and `objects`:
everything a caller reads. Effects and objects are sorted (step 03), so `==`
works.

### The visitor

`visitor`'s cycle is `accept → visit → visit_node → visit_children → accept`,
four members, headed by `accept`. `root` comes from its annotation (step
03.07), and `node.left` is named by its path with class `Node` from the
declaration (step 05), so the inner `accept` has the same key as the outer one:
a cycle, not an endless chain of new contexts.

`visit_children`'s write `self.found = Report()` is concrete: an effect on its
`Param(0)` in its round-0 summary. It travels up through the group's `Apply`
operations, weak (decision 2) and as `Opaque(Report)` (decision 3, the object
went around the cycle), and `visitor` applies it to `v`. `v.found.emit()`
finds `Report.emit`. Each method is walked once.

(Iterating `node.children` instead of `node.left` would not form a cycle:
containers are not modelled, so the loop variable is `Unknown`, and the inner
call has no target.)

### Yours

- `Member` and `Write` operations, the deep pending check before keying, and
  `Apply`'s extra fields.
- Weak application of everything replayed.
- The identity flag on origins, and its export rule in `finish`.
- `set_current` comparing effects and objects.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/defer.rs: replaying the two new operations
DeferredOp::Member { base, name, site } => {
    let base = substitute(&base, &now);
    let mut env = d.before.clone();
    base.iter().flat_map(|r| self.member_value(s, *r, &name, site, &mut env)).collect()
}
DeferredOp::Write { base, name, value } => {
    let base = substitute(&base, &now);
    let value = substitute(&value, &now);
    for r in &base {
        if let AbstractValue::Instance { origin, .. } = r
            && self.holds_state(*origin)
        {
            self.exit_writes.push((*origin, name.clone(), value.clone()));   // joined into the exit, weak
        }
    }
    Values::new()
}
```

`finish_round` joins `exit_writes`, and the effects of every replayed
`Apply`/`Call`, into the exit env with `bind_attr(…, strong: false)` before
computing effects.

```rust
// src/interp/summary.rs: to_sval, the identity rule
OriginKey::New { .. } if self.came_from_member(origin) => SVal::Plain(PlainVal::Opaque(class)),
```

</details>

---

## Run

```rust
#[test]
fn p04_08_recursion_objects() {
    check_both("04-class-summaries/python/recursion_objects.py");
}
```

Then run `groups.py` from step 02.06 again: its parser now splits `combine`
per type (step 06), and its tree must not change.

---

## Where you are

```
DeferredOp = Apply | Call | Member | Write
in a member's summary: objects from a member's summary are Opaque(class)
the fixpoint compares returns, effects and objects
```

Every structure in `00-values/README.md` now exists in your code.

---

## Exercise

1. `def chain(n): return chain(n - 1).next if n else Node()`. Which
   operations does the walk record?
2. Why is the identity rule applied when *exporting* a summary, and not to the
   values inside the walk?
3. In `visitor`, the effect reaches `v` weak. Is anything lost because it is
   weak?

<details><summary>Answers</summary>

1. `Apply` for `chain(n - 1)` → `Pending(0)`, then
   `Member { base: [Pending(0)], name: next }` → `Pending(1)`, which is
   returned. Round 1 reads round 0's `Node`, whose `next` comes from ty.
2. Inside the walk, the object's attributes are needed right away: `r.close()`
   in `walk` needs to know `r` is a `Handle` with its methods, and in general a
   chain of reads on it. Only the *summary* has to stay finite across rounds,
   so only the summary forgets.
3. Nothing here: `v.found` held nothing before, so "old or new" is just the
   new value. A weak effect only costs precision when there was an old value,
   which then stays possible alongside the new one.

</details>

---

→ Next: [`09-retiring-inline-mode.md`](09-retiring-inline-mode.md)
