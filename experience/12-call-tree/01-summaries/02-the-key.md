# 01.02 — The key: what a call is

**Reading:** [`plan/03-call-tree/08-keys.md`](../../../plan/03-call-tree/08-keys.md).
**Fixture:** `python/sharing.py`.

---

## The context

```rust
pub struct Context<'db> {
    pub func:  Definition<'db>,
    pub args:  SmallVec<[FpId; 4]>,          // one per parameter, receiver first, defaults filled
    pub alias: SmallVec<[u8; 4]>,            // which parameters hold the same object
    pub free:  SmallVec<[(Name, FpId); 2]>,  // a closure's captured values
}
```

A context is the function plus a **fingerprint** of each argument. Two calls
with equal contexts get one summary.

## Fingerprints

A fingerprint is a caller's value reduced to what the key holds:

| value | fingerprint |
|---|---|
| an object the walk tracks | `Instance { class, attrs }`: class, plus each attribute's fingerprint, to `fp_depth` steps |
| an object at the depth limit, or one ty handed over | `Opaque(class)`: class only |
| `3`, `"x"` | `Data(int)`, `Data(str)`: literals widen to their class |
| a function, class, module | itself |
| a bound method | `Bound { func, recv }`: the receiver's fingerprint |
| a closure | `Closure { func, free }`: its captured values' fingerprints |
| nothing known | `Unknown` |

```python
p = Pipeline(JsonHandler())        # p.handler = a JsonHandler, built here
p.run()
```

`run`'s key: `self = Instance { Pipeline, attrs: [handler: Instance { JsonHandler, attrs: [] }] }`.

**Where the object was built is not in the key.** Two `Pipeline(JsonHandler())`
objects built on different lines have the same fingerprint, so they share one
`run` summary. That sharing is the point. Which object is which is kept another
way: by `alias` and by paths in the summary ([`03-the-result.md`](03-the-result.md)).

### `fp_depth`

Every step counts: an attribute, a closure's captured value, a bound method's
receiver. With `fp_depth = 2`:

```python
Service(App(Config(JsonHandler()))).run()
```

`self = Instance { Service, [app: Instance { App, [config: Opaque(Config)] }] }`.
`config`'s own attributes are not in the key. Inside `run`,
`self.app.config.handler` is unknown, so a call on it has no edge. That is a
known limit, measured before the default is set
([`plan/03-call-tree/15`](../../../plan/03-call-tree/15-checks-and-build-order.md#known-limits)).

### Parameters that don't matter

```python
def log_event(name, payload):
    logger.info(name)
```

Neither parameter reaches a call target, a receiver, a project call's argument,
a `return`, an attribute write or a nested `def`. So both are `Unknown` in the
key, and every call to `log_event` shares one summary. `relevant_params(func)`
decides this from the code alone, once per function. It must be conservative:
`x = w; x.write()` makes `w` relevant through `x`. Marking too much costs some
sharing; marking too little gives wrong trees.

### Several values: split

If `h` may be a `JsonHandler` or a `CsvHandler`, `build(h)` is **two calls**: one
context per combination of argument values. Up to `split_cap` combinations;
above it they collapse into one context whose fingerprint is a union.
[`04-one-call.md`](04-one-call.md) shows why each combination needs its own
objects.

## Starting a walk from a key

`materialize(ctx)` builds the starting env from the key, and nothing else:

| fingerprint | the walk starts with |
|---|---|
| `Instance { class, attrs }` | an object with origin `Entry(path)`, its attributes written into the env |
| `Opaque(class)` | an object with origin `Entry(path)` and no attributes |
| `Bound`, `Closure` | the method or closure, its receiver or captured values at `path + [Recv]` / `[Free(n)]` |
| `Unknown` | the parameter's annotation, if it has one ([`00-values/04`](../00-values/04-annotation-fallback.md)). Else `Unknown` |

Parameter `i` is at `Path { root: Param(i) }`; `self.handler` is at
`Param(0).handler`. Two parameters the key says are aliases share one origin.

---

## Exercise

Using `python/sharing.py`:

1. Write `run`'s key for each `.run()` call in `main`. How many `run` contexts?
2. `audit(event, sink)` writes `sink.last = event` and returns nothing. Is
   `event` relevant? Is `sink`?
3. In `deep()`, with `fp_depth = 2`, what is the fingerprint of `svc` at the
   call `svc.run()`? What starts in `run`'s env for `self.app.config`?

<details><summary>Answers</summary>

1. `a.run()` and `b.run()` both have
   `self = Instance { Pipeline, [handler: Instance { JsonHandler, [] }] }`: one
   context. `c.run()` has a `CsvHandler`: a second context. Two in total.
2. Both. `event` reaches an attribute write's value, and `sink` is the target
   of an attribute write. The write is an effect the caller must see, so both
   go in the key.
3. `Instance { Service, [app: Instance { App, [config: Opaque(Config)] }] }`.
   `run`'s env starts with `self.app.config` as an object of class `Config`
   with origin `Entry(Param(0).app.config)` and no attributes.

</details>

---

→ Next: [`03-the-result.md`](03-the-result.md)
