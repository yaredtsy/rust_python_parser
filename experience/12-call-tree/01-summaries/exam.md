# 01 — Summaries: exam

Closed book. Write your answers, then check [`answers.md`](answers.md).

1. Why is walking the tree path by path not an option for a fast port? What does
   work become with summaries?

2. State the three rules. For each, give the bug it prevents.

3. `Pipeline(JsonHandler()).run()` on line 10 and `Pipeline(JsonHandler()).run()`
   on line 20. One `run` context or two? Why is that safe, given they are two
   different objects?

4. `def log(msg, obj): logger.info(msg)`. What is `log`'s key for
   `log("x", JsonHandler())` and for `log("y", CsvHandler())`?

5. Write the summary of `def install(p, h): p.handler = h`. Then of
   `def maybe(p, h): if c: p.handler = h`. What does the caller do differently?

6. `old = b.replace(CsvHandler())` where `replace` returns the old `self.h` and
   writes the new one. Explain the two phases, and the wrong answer you get if
   they run in the other order.

7. `h` may be a `JsonHandler` or a `CsvHandler`, and `build(h)` returns
   `Job(h, h.key())`. Why does each returned `Job` need an origin with the
   combination index in it?

8. A context was truncated by `max_stack`. Why isn't its result stored, and what
   happens next time it's reached?

9. What does no-reuse mode check, and what doesn't it?

10. `main` calls `emit` twice, with two writers. How many nodes does pylspt send,
    how many does v-noc save, and what is `call_count`? Why do fixtures avoid
    this shape?

---

→ [`answers.md`](answers.md)
