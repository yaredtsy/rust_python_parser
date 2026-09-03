# Exam 03 — The database

---

## Recall

**1.** Define, in one sentence each: input, query, revision.

**2.** `scanner.py` caches on the full file content string. Name the three
structural problems with that, and say which one salsa fixes by keying on
identity rather than content.

**3.** Why are there three file handles (`File`, `PythonFile`, `ProgramFile`)
instead of one? Answer in terms of *what may be shared* between two programs.

**4.** Which handle does each of these take, and why is that the right one?

- `source_text`
- `parsed_module`
- `SemanticModel::new`

**5.** Why does `parsed_module` return something you must call `.load(db)` on,
instead of handing you the AST?

---

## Invalidation

**6.** You edit `helpers.py`. For each of these queries, say whether it re-runs,
and why:

- `source_text(main.py)`
- `parsed_module(helpers.py)`
- `parsed_module(main.py)`
- `line_index(models.py)`
- type inference for `run` in `main.py`

**7.** What is *backdating*, and how could it make a whitespace-only edit
cheaper than you predicted?

**8.** You write a file to disk and forget `File::sync_path`. Describe the
symptom. Why is it hard to notice in a test suite?

---

## Mutation and threads

**9.** Why does taking `&mut db` cancel in-flight queries on other threads? What
mechanism does salsa use to do the cancelling?

**10.** Your RPC layer gets a `resolve_calls` request and a `didChange`
notification at the same moment. Describe what must happen, and what your
handler must be prepared to do.

**11.** `ProjectDatabase` is `Clone`. What is a clone — a deep copy of the
cache, a reference-counted handle, or something else? What can you do with one,
and what can you not?

---

## Predict, then run

**12.** Time `parsed_module` cold and warm on `main.py`. Write both numbers and
the ratio. Was your prediction high or low?

**13.** Run your exercise-02 scanner over all four fixture files sequentially,
then over four db snapshots in parallel. Give both wall times. Is the speedup
close to 4×? If not, what is the limiting factor — and is it the same factor
that limits your Python driver?

**14.** Add a fifth file to `proj/` that imports nothing and is imported by
nothing. Edit `helpers.py`. Does anything about the fifth file get recomputed?
What does the answer tell you about what salsa is actually keyed on?

---

## Design

**15.** For each, say tracked or not tracked, with a reason:

- your syntax node tree for `parse_file`
- MRO / base classes
- the context-sensitive call tree
- your project-code filter (`is this file under the project root`)

**16.** ty's own `call_hierarchy` entry points are deliberately **not** tracked,
while everything they call is. State the principle in one sentence, and explain
why tracking the entry point would be actively harmful for a query whose input
is a call-site environment.

**17.** You want to cache your own analysis of a function across requests. What
is the wrong thing to store, and what is the right key to store instead?
