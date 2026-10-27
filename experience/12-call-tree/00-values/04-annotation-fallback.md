# 00.04 — Falling back to the annotation

When nothing is known about a parameter, its annotation is still known. This
file is about the one place that is used, and why it is safe.

**Reading:** [`plan/03-call-tree/08-keys.md`](../../../plan/03-call-tree/08-keys.md)
§ "The annotation fallback" and § "Entry contexts".

**Fixture:** `python/annotated.py`.

---

## The gap it closes

```python
def needs_the_annotation(h: Handler):
    h.handle(1)

def run_unresolved(opaque):
    needs_the_annotation(opaque)      # `opaque` is unknown
```

`run_unresolved` passes a value nobody knows, so `needs_the_annotation`'s key
holds `Unknown` for `h`. If the walk started `h` as `Unknown`, `h.handle(1)`
would have no target. But the author already wrote the answer down: `h` is a
`Handler`.

## Where it applies

At the start of a walk, when the key is turned into the starting env
([`01-summaries/02`](../01-summaries/02-the-key.md)):

> A parameter whose fingerprint is `Unknown` starts from its annotation,
> lifted. Each instance in it gets the parameter's own `Entry` origin, so writes
> to it are still tracked. An unannotated `self` or `cls` uses the enclosing
> class.

So `h` starts as `Instance { Handler, Entry(Param(0)) }`, and `h.handle(1)` finds
`Handler.handle`.

**Entry functions** are the most common case. An entry is a function v-noc asks
about directly. Its key has every parameter `Unknown`, so every annotated
parameter starts from its annotation.

```python
def run(make: Callable[[], JsonHandler]):
    make().process()     # make starts as Ty(Callable); calling it gives ty's
                         # return type, JsonHandler → JsonHandler.process
```

## Why it's safe

**It never overrides the key.** It only fires when the key says `Unknown`. A
value the caller actually traced always wins over a declared one. Swap the
order and `w: Handler = SpecificHandler()` would report `Handler` instead of
what the caller passed: a regression, not a fallback.

**It depends only on the code.** The annotation is part of the function, not of
the caller. So the walk is still a function of its key (rule 1,
[`01-summaries/01`](../01-summaries/01-why-summaries.md)), and the summary is
still safe to reuse.

**It is counted.** An annotation is a claim, not a proof: callers in an untyped
codebase may pass something else, and signatures go stale.
`Stats.annotation_fallbacks` counts every use, so "why does this leaf say
`Handler`?" has an honest answer.

A vague annotation (`Any`, `object`, a bare `dict`) lifts to `Unknown` or to a
type with no useful members. No extra rule is needed; the fallback simply gives
nothing.

## What it is not

It is not a general "third tier" for every expression. A module global read
through ty already gets ty's answer, annotation included, because ty uses the
declaration. A local inside the walk has the value its assignment produced.
The fallback is only for a parameter the key knows nothing about.

---

## Exercise

Using `python/annotated.py`:

1. `from_untyped_dict(config: dict)` returns `config.get("handler")`. As an
   entry, what does `config` start as, and what does the return value resolve
   to?
2. `read_result()` reads the module global `result: Handler = Handler()`. Is
   that the annotation fallback?
3. `run_unresolved` calls `needs_the_annotation(opaque)`, and some other
   function calls `needs_the_annotation(LoudHandler())`. How many contexts of
   `needs_the_annotation` are there, and which edge does each produce?

<details><summary>Answers</summary>

1. `config` starts from its annotation: an instance of `dict`, which is stdlib.
   `config.get(...)` is an external call, and ty says it returns `Any`, which
   lifts to `Unknown`. The control case: nothing resolves, and nothing should.
2. No. `result` is a module global, not a parameter. The env misses, ty answers
   with the declared type `Handler`, and `lift` gives an `OPAQUE` `Handler`
   instance. `result.handle(2)` finds `Handler.handle` through ty, with no
   fallback involved.
3. Two. One with `h = Unknown` in the key, which starts `h` from the annotation
   and finds `Handler.handle`. One with `h = Instance{LoudHandler}`, which uses
   the traced value and finds `LoudHandler.handle`. The annotation never
   overrides the second.

</details>

---

With this folder's vocabulary in hand, go to the folder that ties walks
together. Do the exam first.

→ [`exam.md`](exam.md), then [`../01-summaries/README.md`](../01-summaries/README.md)
