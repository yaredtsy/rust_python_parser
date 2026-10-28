# 00.09 — The four conversions: how a value crosses a call

You've now seen all three ways to write a value: `AbstractValue` in a walk, `Fp`
in a key, `SVal` in a summary. A call is four conversions between them, two in
the caller and two in the callee.

**Reading:** [`plan/03-call-tree/08-keys.md`](../../../plan/03-call-tree/08-keys.md),
[`09-summaries.md`](../../../plan/03-call-tree/09-summaries.md),
[`10-one-call.md`](../../../plan/03-call-tree/10-one-call.md).

---

## The picture

```
        CALLER'S WALK                                     CALLEE'S WALK
   AbstractValue (origin o…)                          AbstractValue (origin Entry…)
          │                                                     ▲
          │ 1. fingerprint      ┌──────────────┐  2. materialize│
          └────────────────────►│ Fp, in the   │────────────────┘
                                │ Context      │
                                └──────────────┘
                                                                │
          ┌─────────────────────────────────────────────────────┘
          │                     ┌──────────────┐
          │      3. finish      │ SVal, in the │      4. resolve
          └────────────────────►│ summary      │────────────────────┐
                                └──────────────┘                    ▼
                                                    CALLER'S WALK, AbstractValue (origin o…)
```

| # | conversion | from → to | where | what survives |
|---|---|---|---|---|
| 1 | **fingerprint** | `AbstractValue` → `Fp` | caller, at the call, reading the snapshot `before` | class and attributes, to `fp_depth`. **Not** which object |
| 2 | **materialize** | `Fp` → `AbstractValue` | callee, at the start of its walk | a fresh object per input, named by its path: `Entry(Param(i)…)` |
| 3 | **finish** (`to_sval`) | `AbstractValue` → `SVal` | callee, at the end of its walk | input objects as `Ref(path)`, built objects as `Obj(j)`, closures with their captured values |
| 4 | **resolve** | `SVal` → `AbstractValue` | caller, right after the call, reading `before` | `Ref(path)` becomes the caller's own object at that path; `Obj(j)` a new origin for this site |

Two things to notice:

- **The fingerprint loses identity on purpose.** That's what lets two callers
  with equal-looking objects share one summary.
- **The path carries it back.** The callee never knew which object it got, but it
  knows where: `Param(0).writer`. The caller follows that path in its own state
  and gets its own object back.

Then, after step 4, the caller writes the effects (phase 2). Step 4 must read
`before`, not the state after the effects, or a callee that returns an old value
while writing a new one (`old = box.replace(new)`) hands back the new one
([`01-summaries/04`](../01-summaries/04-one-call.md)).

---

## The running example: one object through all four

`read_json`'s `JsonWriter`, through `w = p.run(1)`:

**1. fingerprint** (in `read_json`, at `p.run(1)`):

```
self: Instance{Pipeline, o1}
      before.attrs[(o1, writer)] = [Instance{JsonWriter, o0}]
   →  Instance{Pipeline, [writer: Instance{JsonWriter, []}]}        fp#2
```

`o0` and `o1` are gone. Any other `Pipeline` holding any other `JsonWriter` gives
the same fp#2.

**2. materialize** (start of `run`'s walk, ctx#3):

```
fp#2 at Param(0)  →  self = Instance{Pipeline, e0 = Entry(Param(0))}
                     attrs[(e0, writer)] = [Instance{JsonWriter, e1 = Entry(Param(0).writer)}]
```

**3. finish** (end of `run`'s walk):

```
return self.writer = [Instance{JsonWriter, e1}]
   e1 is Entry(Param(0).writer)  →  returns: [Ref(Param(0).writer)]
```

**4. resolve** (back in `read_json`, reading `before`):

```
Ref(Param(0).writer)
   Param(0)  → the combo's argument 0 = Instance{Pipeline, o1}
   .writer   → before.attrs[(o1, writer)] = [Instance{JsonWriter, o0}]
   →  w = [Instance{JsonWriter, o0}]
```

`o0` went in, `o0` came out. Identity crossed the call through a path, never
through the key.

## The same, for a built object and an effect

**A built object.** `__init__` doesn't return one, but a factory does:

```python
def make(): return JsonWriter()
```

`make`'s walk builds `New{site}`; finish writes `returns: [Obj(0)]`,
`objects: [NewObject{JsonWriter}]`. The caller resolves `Obj(0)` to a new origin
of its own, `New{site of make(), t: 0, combo: 0, made: Obj(0)}`.

**An effect.** In `read_json`, `Pipeline(JsonWriter())` gets `__init__`'s
summary, `Effect { target: Param(0), attr: writer, value: [Ref(Param(1))] }`.
Step 4 resolves the target `Param(0)` to `o1` (the new `Pipeline`) and the value
`Ref(Param(1))` to `o0`. Phase 2 then writes `attrs[(o1, writer)] = [o0]`.

---

## Exercise

1. `read_csv` calls `.run(3)` on its `Pipeline(CsvWriter())`. Write all four
   conversions for its `CsvWriter`.
2. Why can't step 1 keep the origin, so step 4 isn't needed?
3. Which step would break if `finish` wrote `Instance{JsonWriter, e1}` into the
   summary as is?

<details><summary>Answers</summary>

1. (1) `Instance{Pipeline, [writer: Instance{CsvWriter, []}]}` = fp#6.
   (2) In ctx#7's walk: `self = Entry(Param(0))`, `self.writer = Entry(Param(0).writer)`.
   (3) `returns: [Ref(Param(0).writer)]`, the same as ctx#3's.
   (4) In `read_csv`: `Param(0)` → its `Pipeline` → `.writer` → its `CsvWriter`.
   The result is discarded, since `read_csv` doesn't assign it.
2. Then no two calls with different objects could share a key, so nothing would
   be reused, and the analysis would be the exponential walk again. And the
   origin is walk-local: it means nothing in the callee's walk.
3. Step 4, and every caller after the first: `e1` is an id from `run`'s walk.
   In `read_json`'s walk it names nothing, or the wrong object. That is why the
   summary's value type (`SVal`, `PlainVal`) can't hold an origin at all.

</details>

---

→ Next: [`10-the-output.md`](10-the-output.md)
