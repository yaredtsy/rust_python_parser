# 00 — Values: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

1. List all seven variants of `AbstractValue`. For each, give one Python
   expression that produces it.

2. `Type::Union(A | B)` and `Values` both represent "more than one
   possibility." What's the one-sentence difference in how they're meant to
   be used, and what goes wrong if you treat them the same way?

3. Why is `AbstractValue::BoundMethod`'s receiver a `ValueId` rather than a
   `Box<AbstractValue<'db>>`?

4. `def make(h): return Pipeline(h)` — one line of code — is reached
   twice from the same caller: `make(JsonHandler())` then
   `make(CsvHandler())`. Do the two `Pipeline(h)` constructions get the same
   `OriginId`? What if both calls had passed equal argument values instead?
   Justify your answer from `OriginKey`'s definition, not from intuition.

5. Write the env-first-then-ty-then-annotation resolution order as a
   three-line chain, in the style of `03-env-and-scope.md`'s two-line rule.
   Which tier is allowed to override which?

6. A function parameter `h: Handler` receives an `Unknown` value at a call
   site (the caller's own value couldn't be determined). What does the
   annotation-fallback tier do here, and what confidence should that
   produce in your stats?

7. State the `(parent frame, target_qname)` rule in one sentence. Then: `f()`
   is called three times from the same frame. How many children does the
   frame have, and what is `call_count`?

8. `is_ancestor` and the return-value in-flight memo both prevent infinite
   recursion. Give one input where using `is_ancestor` for the return-value
   case specifically produces a **wrong** answer (not just a less efficient
   one).

9. Name the four scaling brakes from `06-scaling-budget-and-memoization.md`.
   Which one is checked once globally per request rather than per branch,
   and why does that distinction matter?

10. A function's call tree is memoised as "context-independent." Later, you
    add local-assignment tracking (`01-functions/05`). Explain in 2–3
    sentences why the context-independence check must become stricter, with
    a concrete example.

---

→ [`answers.md`](answers.md)
