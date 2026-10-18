# 00.04 — Falling back to the type annotation

★ **This file is an addition to the base design, not a restatement of it.**
`plan/03-call-tree/` doesn't budget this explicitly — it falls out of a
decision this exercise makes deliberately: don't let `Unknown` win when a
human already wrote the answer down.

---

## The gap this closes

Recall the two-line rule from [`03-env-and-scope.md`](03-env-and-scope.md):

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>) -> Values<'db> {
    self.resolve_from_env(expr, env)
        .unwrap_or_else(|| self.resolve_from_ty(expr))
}
```

`resolve_from_ty` can itself come back with nothing useful —
`Type::Dynamic`, or a declared type too vague to act on. Today that becomes
`AbstractValue::Unknown` and the tree stops there. But look at
`python/annotated.py`'s `needs_the_annotation`:

```python
def needs_the_annotation(h: Handler):
    h.handle(1)
```

If `h` arrives as `Unknown` — because the caller's own value couldn't be
determined (`run_unresolved` in that file) — you still know something
concrete: **the parameter is annotated `Handler`.** Nobody asked you to
guess; the author already told you, in the signature, in a way `ty` itself
already parses and type-checks against. Throwing that away in favor of
`Unknown` is leaving precision on the table that cost you nothing to pick
up.

## Where this fits in the resolution order

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>, binding: Option<AnnotationSite<'db>>)
    -> Values<'db>
{
    self.resolve_from_env(expr, env)
        .or_else(|| self.resolve_from_ty_if_precise(expr))
        .unwrap_or_else(|| self.resolve_from_annotation(binding))   // ★ new tier
        .unwrap_or(smallvec![AbstractValue::Unknown])
}
```

Three tiers, in order of trust:

1. **`env`** — exact, path-specific. "This call bound `w` to *this*
   `JsonWriter`."
2. **`ty`'s inference** — exact for anything that doesn't depend on path
   (literals, constructor calls ty can see, non-ambiguous returns).
3. **★ the annotation on the binding site** — not inferred, *declared*. Lower
   confidence than (1) or (2) — the author could be lying, or the signature
   could be stale — but strictly better than giving up.

`resolve_from_ty_if_precise` is worth naming explicitly: if `ty` itself
already widened to the declared type because it *couldn't* narrow further
(a parameter with no default and no call-site info, inside `ty`'s own
flow analysis), you've already landed on tier 3's answer by a different
route — don't double up, and don't treat that as tier 2's "exact" case.

## `AnnotationSite` — where annotations actually live

A binding site, for this purpose, is one of:

| site | example | gets you |
|---|---|---|
| parameter annotation | `def f(h: Handler)` | the type for `h` inside `f`'s body |
| variable annotation | `result: Handler = compute()` | the type for `result` after this statement |
| return annotation | `def f() -> Handler: ...` | the type of `f()`'s result, when `01-functions/05`'s return-flow interpretation itself bottoms out at `Unknown` |

All three are things `ty` already parses as `ast::Expr` annotations and
already exposes an inferred `Type` for (`annotation.inferred_type(model)`).
**You are not writing a parser for this.** You are adding exactly one
`.or_else` to a chain that already has two tiers, and feeding it a `Type`
you get from a call you're already making elsewhere in the codebase.

```rust
fn resolve_from_annotation(&self, site: Option<AnnotationSite<'db>>) -> Option<Values<'db>> {
    let site = site?;
    let ty = site.annotation.inferred_type(self.model)?;
    Some(lift(self.db, self.env(), ty))   // the SAME lift() from 03-env-and-scope.md
}
```

Reusing `lift` matters: an annotation that's itself a union
(`h: Handler | None`) gets exploded into separate values exactly like a `ty`
union does, for exactly the same reason.

## Why this is safe to add, and where it isn't

Safe: it only fires where tiers 1–2 produced *nothing*, so it can only ever
add precision, never override something the environment or `ty` already
determined with more confidence.

Not free: an annotation is a claim, not a proof. `h: Handler` doesn't
guarantee the caller actually passed a `Handler` — nothing stops a caller
from passing something structurally compatible but differently typed in an
untyped codebase, or a stale annotation nobody updated. Treat this tier's
answers as **lower-confidence** in your stats (`08-termination-and-cycles.md`'s
counters are the natural place — add `annotation_fallbacks: u32`), so a
"why does my tree say this" question has an honest answer: *"the
environment and the inference both came up empty; this leaf is from the
declared annotation, not from a traced value."*

Do **not** use this tier to override `Unknown` when the annotation itself is
vague (`Any`, a bare `object`, a wide `Protocol` with no members you'd ever
resolve a call through) — `lift`'s existing `Dynamic`/`Never` arm already
turns those into `Unknown` so there's no extra code needed; just don't be
surprised when the fallback sometimes produces `Unknown` too.

## Exercise

Using `python/annotated.py`:

1. For `from_untyped_dict`, walk the resolution order for `config.get("handler")`'s
   receiver. Which tier answers, and why does this function deliberately have
   *no* fallback available?
2. For `read_result`, `result` is a module-level variable with an explicit
   annotation. Which tier answers — 1, 2, or 3? (Careful: `ty` is flow-sensitive
   within a scope and may already narrow this one for free. Decide which tier
   you'd *credit* in your stats, and why that's a judgment call rather than a
   fact.)
3. Sketch, in one sentence, what would go wrong if tier 3 ran **before**
   tier 1 instead of after it.

<details><summary>Answers</summary>

1. No tier answers with anything specific — `config: dict` carries no
   element-type annotation, `ty` can only say "a dict", and there's no
   annotation on `config.get("handler")`'s *result* to fall back to. This is
   the deliberate control case: not everything should resolve, and this file
   isn't proposing that it should.
2. Tier 2 most likely answers first, honestly — `ty` sees the literal
   assignment `result: Handler = Handler()` and can narrow `result`'s type
   at the point `read_result` reads it, same scope, same module. Tier 3
   would give the *same* answer here, which is exactly why this is a
   judgment call: crediting it to tier 2 is more accurate (ty really did
   work it out), but if your implementation order happens to ask tier 3
   first for module-level reads specifically, document that choice rather
   than silently miscounting your own stats.
3. A real, path-specific value in `env` — tier 1 — would lose to a stale or
   overly-generic declared annotation. `w: Handler = get_specific_subclass()`
   would report `Handler` instead of the *actual* subclass your interpreter
   traced, which is strictly a regression versus not having this tier at
   all. The ordering is the entire safety argument — never reverse it.

</details>

---

→ Next: [`05-frame-pathkey-and-budget.md`](05-frame-pathkey-and-budget.md)
