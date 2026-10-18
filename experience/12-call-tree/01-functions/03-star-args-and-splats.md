# 01.03 — `*args`, `**kwargs`, and call-site splats

The one place this design explicitly chooses to lose precision — and the
rule for doing it safely instead of guessing.

**Fixture:** `python/varargs.py`. **Reading:**
[`plan/03-call-tree/05-binding-arguments.md`](../../../plan/03-call-tree/05-binding-arguments.md)
§"3. `*args`/`**kwargs`" and §"4. Star-args at the call site".

---

## Parameter-side: `def takes_splat(a, *rest, **opts)`

Your Jedi driver delegates this to `TreeArguments`, which actually does
model it. Three options, in increasing effort, and the guidance is explicit:

- **(a) Bind `rest`/`opts` to `Unknown`.** Calls made *through* `*args` lose
  precision; the function **body is still walked**, and ordinary parameters
  (`a` here) still bind normally. **Start here, and usually stop here** —
  measure before doing more.
- (b) Bind `*args` to a `Values` of the leftover positionals, so
  `args[0].run()` could resolve if constant subscripts are also modelled.
- (c) Full splat modelling. Not worth it.

```rust
for param in params_iter {
    if param.is_vararg() { locals.insert_mut(param.name(), smallvec![AbstractValue::Unknown]); continue }
    if param.is_kwarg()  { locals.insert_mut(param.name(), smallvec![AbstractValue::Unknown]); continue }
    // ... ordinary binding for everything before these
}
```

## Call-site: `takes_splat(*items)`

This is the sharper case, and the rule is different in kind, not just
degree. Once a call site splats an iterable of **unknown length** into the
arguments, there is no sound way to say which parameter gets which value —
`items` could have 0 elements or 50.

> **Bind ALL remaining parameters to `Unknown` rather than guessing an
> alignment.** A wrong alignment produces a *confidently wrong* tree — one
> that looks precise and isn't — which is strictly worse than an admittedly
> imprecise one. This is the same principle as the decorator rule in
> [`07-curry-partial-and-decorators.md`](07-curry-partial-and-decorators.md):
> when you can't be right, be visibly unsure rather than silently wrong.

## Exercise

Using `python/varargs.py`:

1. `call_normally()` calls `takes_splat(Handler(), 1, 2, extra=True)`.
   Write the locals map `bind_parameters` produces.
2. `call_with_call_site_splat(items)` calls `takes_splat(*items)`. Does `a`
   (the one parameter with no star) bind to `Unknown` too, even though in
   principle `items[0]` might be knowable? What does the rule above say,
   and why does it say that even for `a`?
3. In both cases, does `a.go()` inside `takes_splat`'s body get walked at
   all? What differs between the two calls is the *binding*, not whether
   the walk happens — explain why that distinction matters for the tree's
   shape.

<details><summary>Answers</summary>

1. `{ a → Instance{Handler, O1}, rest → Unknown, opts → Unknown }` — `a`
   binds normally from the first positional; the splat parameters both take
   the Unknown shortcut regardless of what was actually passed through
   them.
2. Yes — **all** parameters, including `a`, become `Unknown`. The call-site
   splat rule is about not *guessing an alignment at all*, not about
   salvaging the parameters you can. Even though `items[0]` conceptually
   corresponds to `a`, your interpreter doesn't model indexing into an
   arbitrary iterable (`00-values/04`'s "what you deliberately do not
   model" list), so claiming to know `items[0]` would itself be a guess —
   exactly the "confidently wrong" case the rule exists to prevent.
3. Yes in both cases — `walk_body` doesn't care what `a` is bound to; it
   just resolves whatever call sites exist in `takes_splat`'s body against
   whatever `env` it was given. In case 2, `a.go()` still appears as a
   node in the tree, but its *leaf* resolves to nothing informative — the
   receiver is `Unknown`, so `member()` has nothing to look attributes up
   on. The call-tree *shape* survives; only the *precision* at that one
   leaf degrades. That's the intended trade-off: lose information, not
   structure.

</details>

---

→ Next: [`04-closures-and-nested-functions.md`](04-closures-and-nested-functions.md)
