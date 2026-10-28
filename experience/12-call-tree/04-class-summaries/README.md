# 04 — Class summaries

**Goal:** summary mode gives part 03's trees. Each method walked once per key,
with objects crossing calls in both directions: into a key, back out as a
return, and as **effects**, the writes a call made on objects its caller can
see.

In inline mode, an object was one origin in one walk, and a write in a callee
was simply there for the caller. In summary mode, the callee is walked once,
alone, from its key, and its summary is reused by callers it has never seen.
So a summary can't say "object 7". It says "the object you passed as parameter
0", "its `.handler`", "a new object I built". That is this whole part.

---

## The steps

| | step | you build | the decision |
|---|---|---|---|
| 1 | [`01-objects-in-keys.md`](01-objects-in-keys.md) | `Fp::Instance`, `Opaque`, `fp_depth`, `Entry` origins | an object is keyed by its class and attributes, to a fixed depth. Where it was built is not in the key |
| 2 | [`02-objects-coming-back.md`](02-objects-coming-back.md) | `SVal::Ref`, `SVal::Obj`, `NewObject` | a returned object is "your object at this path" or "a new one I built", never a fingerprint |
| 3 | [`03-effects.md`](03-effects.md) | `Effect`, `written`, apply | a write to an input object is reported relative to the inputs, and applied by the caller. `__init__` is no exception |
| 4 | [`04-resolve-then-write.md`](04-resolve-then-write.md) | phase 1 / phase 2 | every value of a summary is read from the state before the call, then the writes happen |
| 5 | [`05-deep-writes.md`](05-deep-writes.md) | paths past the key, `max_path`, `Beyond` | an input object the key didn't hold is still named by its path |
| 6 | [`06-split-by-type.md`](06-split-by-type.md) | CPA, combo origins | one call with several argument types is several contexts, and their objects never mix |
| 7 | [`07-aliases.md`](07-aliases.md) | the alias vector | two parameters holding one object are one object in the walk |
| 8 | [`08-recursion-with-objects.md`](08-recursion-with-objects.md) | `Member`, `Write`, the identity rule | an object doesn't keep its identity around a cycle |
| 9 | [`09-retiring-inline-mode.md`](09-retiring-inline-mode.md) | the final gate | every fixture, both modes, every difference named |

**Steps 1, 2, 3 and 4 are the core.** Together they are "an object crosses a
call", and the four together make part 03's fixtures pass in summary mode at
step 3.

---

## The fourth form of a value

Part 02 had two forms of a value: in a walk (`AbstractValue`) and in a key or
summary (`Fp`, `SVal`), and three conversions between them. Objects make all
four conversions matter, because an object's identity is walk-local:

```
caller's walk              key                callee's walk              summary
 Instance{origin 3}  ──fingerprint──►  Instance{class, attrs} ──materialize──►  Instance{Entry(Param(0))}
                                                                                      │ finish
 Instance{origin 3}  ◄────────────────── resolve ───────────────────────────  Ref(Param(0))
```

`00-values/09-the-four-conversions.md` draws this with one object followed all
the way round. Read it at step 2.

---

## What you add

```
Fp      += Instance { class, attrs }, Opaque(class), Bound { func, recv }
Context += alias
OriginKey = Entry(Path) | Beyond { at, attr } | New { site, t, combo, made }
SVal    += Ref(Path), Obj(u16), Bound { func, recv }
FunctionResult += effects: [Effect], objects: [NewObject]
Env     += written: (OriginId, Name) → Must | May
DeferredOp += Member, Write
```

---

## Read first

- `plan/03-call-tree/08-keys.md`, `09-summaries.md`, `10-one-call.md`: this
  part implements them for objects
- `00-values/09-the-four-conversions.md` and `07-origins-and-ids.md`

---

## Done when

- [ ] every fixture of parts 01–04 passes in summary mode, with and without
      reuse
- [ ] every difference from inline mode is a named, mode-specific block
- [ ] you can explain why phase 1 reads the snapshot before phase 2 writes,
      with `Box.replace`

---

→ Start: [`01-objects-in-keys.md`](01-objects-in-keys.md)
