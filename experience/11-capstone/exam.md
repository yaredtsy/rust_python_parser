# Final exam — all eleven exercises

Closed book. About an hour. If you cannot answer something, note it and move on
— the gaps are the point, and they tell you which exercise to revisit.

---

## Part 1 — The stack (00, 03, 05)

**1.** Draw the `Db` trait stack from `ruff_db::Db` up to `ProjectDatabase`, and
say which trait your analysis functions should take.

**2.** You have `File`, `PythonFile` and `ProgramFile`. For each, say what it is
keyed on and give one query that takes it.

**3.** Why is `ruff_db = { features = ["os"] }` necessary, and why did the plan
not mention it?

**4.** Name three things that break if two of your git dependencies use
different revisions.

**5.** Where does `import json` resolve to, and what is unusual about that file?

---

## Part 2 — Text and trees (01, 02)

**6.** Give the four expressions converting a `LineColumn` pair into your wire
format's `position`.

**7.** Which encoding does `line_column` use? Why is that the right one, and
which fixture proves it?

**8.** For `a.b().c()`, give each call's `call_index` and say which parts of
their positions are shared.

**9.** Where does `StmtFunctionDef.range` start for a decorated function? Name
the *second* problem decorators cause, beyond the range.

**10.** Name three quirks from `plan/00-orientation/01` that your scanner
implements, and the mechanism for each.

---

## Part 3 — Meaning (04, 06, 07, 08)

**11.** Give ty's five version sources in priority order, and the one that
surprises people.

**12.** What is the difference between flow-sensitive and context-sensitive?
Which is ty, and what is the one-line example that separates them?

**13.** Build the qualified name for a method of a class nested inside another
class. What is the naive answer, and what makes it wrong?

**14.** Name the six `Type` variants that matter for callee resolution, and what
each means.

**15.** `iter_mro` is private. What is the public route to base classes, what
does it actually return, and what must you add to match `py__mro__()`?

**16.** For `self.handler.handle(payload)`, why is ty's answer correct and
unusable? What would a per-path answer require that a parameter environment
alone does not provide?

---

## Part 4 — The gap (07, 09)

**17.** State, in one paragraph and without jargon, why `ty_ide::outgoing_calls`
cannot be the basis of your call tree. Assume your reader knows Python and
nothing about ty.

**18.** Six behaviours separate `outgoing_calls` from your call tree. Name them.
Which one is fundamental and which five are additive?

**19.** If you added a call-site environment to salsa's inference cache key,
what would happen? Why is that worse than having no cache?

---

## Part 5 — Writing files (10)

**20.** Why does ID injection need libcst when you already have a parsed AST?

**21.** Give the three-step write sequence and the symptom of omitting the third.

**22.** Name the two pre-existing bugs you reproduced, and the argument for
reproducing them.

---

## Part 6 — Judgement

These have no single right answer. Answer them as decisions, with reasons.

**23.** You are on Option B (git deps, public API). Name the one requirement
that will force a decision about Option A, and say at which milestone you should
make that decision — and why not earlier or later.

**24.** Your M5 context-free call tree produces a plausible tree with the right
shape and wrong details. Would you ship it behind a flag? Argue both sides, then
decide.

**25.** You find a genuine bug in the Python driver's behaviour — one that loses
information users would want. What do you do, and when?

**26.** Which of the eleven exercises taught you the most, and which one do you
now think you rushed? Go back and finish that one.

---

## Part 7 — Practical

**27.** From your own measurements: cold vs warm `parsed_module`; sequential vs
parallel scan; single-callee vs union/`Unknown` ratio; Python driver vs your CLI
on the same files.

Four numbers. They are the entire evidence base for the rest of the project, and
none of them appears in the plan — because only you can measure them.
