# Exam 07 — Types and inference

The most important exam in the folder. Answer 6, 7 and 12 in writing, properly —
they are the ones you will reuse.

---

## Recall

**1.** Jedi's `Value` and ty's `Type` answer different questions. State both
questions in one sentence each.

**2.** Define flow-sensitive and context-sensitive, then say which one ty is,
and give the one-line code example that separates them.

**3.** Why can ty not be context-sensitive? Give the *architectural* reason, in
terms of what its cache is keyed on.

**4.** Name the six `Type` variants that matter to your call tree, and what each
one means for resolving a callee.

**5.** What does `Dynamic`/`Unknown` mean, and what must your interpreter do
with it?

---

## The gap

**6.** For `emit(writer, data)` in `context.py`, ty says `writer.write` is both
`JsonWriter.write` and `XmlWriter.write`.

- Is that answer wrong?
- Which one belongs under `run_json → emit` in your call tree?
- What input would you need, at the moment of resolution, to pick it?

Write this as a paragraph, not bullets. It is the argument you will make to
anyone who asks why the port is not "just call `outgoing_calls`".

**7.** `context.py` deliberately calls `emit` from two **different** functions
rather than twice from one. Why does that matter? (Hint: frame identity is
`(parent, qname)`.) What would the call tree look like if both calls were in one
function?

**8.** ty's inference is cached per scope. If you tried to make it
context-sensitive by adding the call-site environment to the cache key, describe
what would happen to the cache — and why that is worse than no cache.

---

## Reading types

**9.** For each `flow.py` function, give the type you observed:

| function | expression | type |
|---|---|---|
| `narrowing` | `value` before the guard | |
| `narrowing` | `value` after the guard | |
| `reassigned` | `thing` at each of the three lines | |
| `branched` | `thing` at the return | |
| `unannotated` | `param` | |
| `literal_types` | `n`, `s`, `b` | |
| `calls_returning` | `cache.get("k")` | |

**10.** Does ty say `int` or `Literal[42]` for `n = 42`? Where would each be the
more useful answer for your interpreter?

**11.** Two separate `Cache()` expressions produce the same `Type`. Why is that
a problem for a call tree, and which chapter of the plan deals with it?

---

## The wall

**12.** Write the signature of the function you wanted in step 6, and the
signature of `static_member_type_for_attribute`. Explain in one sentence why the
second cannot serve the first.

**13.** `Type` has roughly ten `pub fn` and seventeen `pub(crate) fn`. Is that a
mistake by ty's authors, or a decision? Argue for the decision, then say what it
means for your Option A/B/C choice.

**14.** You are on Option B (git dependency, public API only). Which of the
three call-tree needs below can you still meet, and which cannot?

- resolving a plain function call
- resolving `obj.method()` where ty can infer `obj`
- resolving `obj.method()` where *you* chose `obj` on this path

---

## Measure

**15.** On a real project, what fraction of callees resolve to a single
`FunctionLiteral` or `BoundMethod`? What fraction are unions or `Unknown`?
Report the numbers and the project size.

**16.** Of the `Unknown`s you found, sample five and classify each: missing
annotation, dynamic construct, ty inference gap, or your own bug in asking. What
is the dominant cause?

**17.** Based on 15 and 16: if you built the context-free call tree only (M5 in
`plan/04-build/02-milestones.md`) and shipped it, what fraction of your tree
would be correct? Is that a useful intermediate product or a misleading one?
