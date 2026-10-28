# 00 — The structures: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

1. Draw the structure picture from the README, top to bottom, from `Solver` to
   `AbstractValue`. Mark which structures live for the whole request and which
   only during one walk.

2. Name the three ways one value is written, where each lives, and whether each
   can name a specific object.

3. What are `ContextId` and `FpId`, and why are contexts and fingerprints
   interned rather than stored inline?

4. Give the `Fp` for each, with `fp_depth = 2`:
   (a) a `Pipeline` whose `writer` is a `JsonWriter` with no attributes;
   (b) the literal `3`; (c) a `JsonWriter` that came from a module global;
   (d) a parameter the function never uses.

5. Draw the life of a slot: every state and what moves it to the next one. Which
   states does a context with no recursion and no limits go through?

6. Write the summary of `def install(p, h): p.handler = h`. Which field of the
   walk did its effect come from?

7. When does a `Walk` exist? What happens to it when its summary is finished,
   and what is the one exception?

8. Name the five fields of `Env` and say what each answers. Why does `Env` have
   no link to the caller?

9. Name the three kinds of `OriginKey`. In `run`'s walk, what is the origin of
   `self.writer`? In `read_json`'s walk, what is the origin of the same Python
   object?

10. Follow `read_json`'s `JsonWriter` through `w = p.run(1)`: give its form after
    each of the four conversions.

---

→ [`answers.md`](answers.md)
