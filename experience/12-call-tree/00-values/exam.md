# 00 — Values: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

1. List the `AbstractValue` variants. For each, give one Python expression (or
   situation) that produces it.

2. `Type::Union(A | B)` and `Values` both mean "more than one possibility". What
   is the difference in how they're used, and what goes wrong if you treat them
   the same?

3. What is the difference between an empty `Values` and `[Unknown]`?

4. Why is `BoundMethod`'s receiver a `ValueId` rather than a
   `Box<AbstractValue>`?

5. Name the three kinds of origin. For each, say where the object came from.

6. One Python object is built in `main` and passed to `run(self)`. Give its
   origin in `main`'s walk and in `run`'s walk. Why are they different?

7. Why can an `OPAQUE` instance never hold attribute state? Give the bug it
   would cause.

8. `Env` has no link to the caller. Then how does `inner` in
   `outer → middle → inner` know what `y` is?

9. When does the annotation fallback fire, and why can it never make a traced
   value worse?

10. `if c: w = JsonWriter() else: w = CsvWriter(); w.write()` — what does `w`
    hold at `w.write()`, and why isn't `c` looked at? Then:
    `w = JsonWriter(); w.write(); w = CsvWriter(); w.write()` — what does each
    `w.write()` see?

---

→ [`answers.md`](answers.md)
