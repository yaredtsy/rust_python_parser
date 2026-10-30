# 04.06 — Split by type

**You build:** CPA, the Cartesian product algorithm. A call whose arguments
hold several values is split into one context per combination of values. The
objects each combination returns get origins of their own, so values that
were never together in Python are never together in the walk.
**Fixture:** `python/split.py`
**Plan:** `plan/03-call-tree/10-one-call.md` § "CPA: one context per
combination", `04-values.md` § "`OriginId`" (the `t` and `combo` fields),
`15-checks-and-build-order.md` fixture 3

---

## The fixture

```python
class JsonHandler:
    def key(self): return JsonKey()
    def process(self, k): k.encode()
class CsvHandler:
    def key(self): return CsvKey()
    def process(self, k): k.encode()
class Job:
    def __init__(self, h, k): self.h = h; self.k = k
    def go(self): self.h.process(self.k)

def build(h):
    return Job(h, h.key())

def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()
    # Python: JsonHandler.process > JsonKey.encode, or CsvHandler.process > CsvKey.encode.
    # Never JsonHandler.process > CsvKey.encode.
```

Inline mode shows all four `encode` edges. Summary mode, with this step, shows
the two real ones.

## Predict

1. Inline mode walks `build` once with `h = JsonHandler | CsvHandler`. What
   do `job.h` and `job.k` hold, and which `encode`s does `go` reach?
2. Summary mode until now keys `build` with `h = Union(JsonHandler,
   CsvHandler)`. Same problem, or a different one?
3. If `build` is split into `build(JsonHandler)` and `build(CsvHandler)`, each
   returns `Obj(0)`, a `Job`. In `main`, at the one call site `build(h)`, what
   keeps the two `Job`s apart?

---

## Build

### Decision 1: one context per combination of argument values

```rust
pub struct Combo<'db> {
    pub slots: SmallVec<[Values<'db>; 4]>,   // one value per split slot; several when collapsed
}

// call_project
let combos: SmallVec<[Combo<'db>; 4]> = s.split(func, &bound).collect();
for (k, combo) in combos.iter().enumerate() {
    let ctx = s.key(self, func, combo, before);
    targets.push(Target::Call(ctx));
    // get, resolve, apply: as before, per combo
}
```

`split` takes the Cartesian product of the **relevant** slots' values (step
02.04): `build(h)` with two handlers is two combos, `build(JsonHandler)` and
`build(CsvHandler)`. Each is walked as its own context. Inside each, `h` is one
object, so `h.key()` is one key class, and the `Job` it builds pairs them
right.

**Why the correlation is lost without it:** a single walk with
`h = JsonHandler | CsvHandler` computes `h.key()` as `JsonKey | CsvKey`, and
from then on "which key goes with which handler" is gone. Unions are sets of
alternatives, not pairs. Splitting at the call keeps each alternative's whole
walk separate.

**This is a precision rule, so rule 3 holds:** the split depends only on the
call's values and the callee's code (which slots are relevant).

### Decision 2: a cap, and a collapse

The product can explode: four parameters with three values each is 81
contexts. Above `split_cap` (4) combinations, the relevant slots collapse into
**one** combo whose fingerprints are `Fp::Union`: part 02's behaviour.
`Stats.cpa_collapses` counts it.

Irrelevant slots never split: they are `Unknown` in the key anyway.

### Decision 3: objects from different combos get different origins

Both `build` contexts return `Obj(0)`. In `main`'s walk, at one call site, two
`Job`s come back. If both got the origin `New { site, made: Obj(0) }`, they
would be one object, holding `h = JsonHandler | CsvHandler` and
`k = JsonKey | CsvKey`: the mix again, one level later.

```rust
pub enum OriginKey {
    Entry(Path),
    Beyond { at: Path, attr: Name },
    New { site: SiteId, t: u8, combo: u8, made: Made },   // ← t and combo added
}
```

- `combo` is the combo index `k` at this site;
- `t` is the callee's index among the site's callees: `h.process()` with two
  handler classes is two callees at one site, and their returned objects must
  not merge either.

A site is evaluated once per walk, so `(site, t, combo, made)` always names
one object. `act` from part 03 is dropped from the key in summary mode: one
walk is one run. (Inline mode still needs it: keep it there, or set it to the
frame's activation, which is constant in a summary walk.)

### Decision 4: a strong write needs one callee and one combo

A call site with two combos is two alternatives: Python made one call, not
both. An effect applied from one combo must not replace what the other combo
might have left. So `strong_ok` (step 03, decision 4) is now
`callees.len() == 1 && combos.len() == 1`. And every combo's phase 1 reads the
same `before` (step 04, decision 3).

### Decision 5: one node per site and function, still

`main`'s `build(h)` site reaches two contexts of `build`. The renderer of step
02.03 draws one node whose children are the union of both contexts' children,
site by site. That is what it was built for. The same for `Job.go`, called on
two objects.

### What about inline mode

Inline mode walks `build` once with both handlers, and shows all four
`encode`s. CPA could be added to inline mode too, by walking the callee once
per combination. The tutorial doesn't: inline mode retires in step 09, and the
fixture names the difference instead. This is one of the places where summary
mode is *more* precise than inline mode.

### Yours

- `Combo`, `split` with `split_cap`, `Fp::Union` only for collapsed slots,
  `Stats.cpa_collapses`.
- `OriginKey::New { site, t, combo, made }`, and `t` and `k` passed down to
  `resolve`.
- `strong_ok` with combos.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/key.rs
pub fn split(&mut self, func: Definition<'db>, bound: &Bound<'db>) -> SmallVec<[Combo<'db>; 4]> {
    let relevant = relevant_params(self.sem.db, func);
    let mut combos: SmallVec<[Combo<'db>; 4]> = smallvec![Combo { slots: SmallVec::new() }];
    let mut size = 1usize;
    for (i, vals) in bound.slots.iter().enumerate() {
        let alternatives: SmallVec<[Values<'db>; 4]> = if relevant[i] && vals.len() > 1 {
            vals.iter().map(|v| smallvec![*v]).collect()
        } else {
            smallvec![vals.clone()]                     // not split: the slot as it is
        };
        size = size.saturating_mul(alternatives.len());
        if size > self.opts.limits.split_cap {
            self.stats.cpa_collapses += 1;
            return smallvec![Combo { slots: bound.slots.clone() }];   // one combo: unions in the key
        }
        combos = combos.into_iter().flat_map(|c| alternatives.iter().map(move |a| {
            let mut c = c.clone();
            c.slots.push(a.clone());
            c
        })).collect();
    }
    combos
}
```

`key` now fingerprints a `Combo` instead of a `Bound`. A collapsed combo still
has several values in a slot, and `fingerprint` makes them a `Union`, as in
part 02.

</details>

---

## Run

```rust
#[test]
fn p04_06_split() {
    check_both("04-class-summaries/python/split.py");
}
```

Then every test. Splitting can only make summaries more precise, so a tree
that changed elsewhere is worth looking at: either the fixture had a hidden
correlation (update it, with a mode-specific block, and say why), or the combo
indices aren't reaching the origins.

One **stats** assertion does change, and that is expected: in `keys.py`,
`either`'s `run(f)` is no longer one `run(a | b)` context, but `run(a)` and
`run(b)`, which already exist. Summary mode now walks 10 bodies with 6
reuses, not 11 and 6 (step 02.03's test, as step 02.04 left it): the
`run(a | b)` walk is gone, and its two inner reuses (`a`, `b`) become the two
split calls' reuses. Update the number in the same commit as this step, with a
comment saying why. The trees don't change.

Step 02.01's `contexts` assertion stays at 11: it measures inline mode, and
inline mode doesn't split (see "What about inline mode" above).

---

## Where you are

```
call_project: split → one context per combo → get, resolve, apply per combo
OriginKey::New { site, t, combo, made }
strong_ok = one callee && one combo
```

---

## Exercise

1. `def pair(a, b): return Job(a, b)` called with `a` two values and `b` two
   values. How many contexts? With `split_cap = 3`?
2. Why is "objects from different combos merge" a wrong tree and not just an
   imprecise one?
3. The parser in step 02.06 had `combine(left = num | binop)` as one context.
   With CPA, what happens instead, and does the final tree change?

<details><summary>Answers</summary>

1. Four: `(a1, b1)`, `(a1, b2)`, `(a2, b1)`, `(a2, b2)`, if both parameters are
   relevant. With `split_cap = 3`, the product (4) is over the cap: one
   context, with `Union`s for both, and one `cpa_collapses`.
2. The merged object holds `h` and `k` values that never existed together on
   any run. A call on it produces edges for those pairs, like
   `JsonHandler.process > CsvKey.encode`: an edge Python never takes, with no
   mark, which is what "wrong" means here.
3. `combine` is keyed twice, `combine(left = num)` and `combine(left = binop)`,
   and both join the group (the plan's version of the parser shows this). The
   tree doesn't change: one `combine` node at that site, with the union of
   both contexts' children.

</details>

---

→ Next: [`07-aliases.md`](07-aliases.md)
