# Answers 03 — The database

---

**1.**

- **Input** — a value the outside world sets and salsa never computes (file
  contents, project settings).
- **Query** — a `#[salsa::tracked]` function whose result is memoised together
  with the set of inputs it read.
- **Revision** — a global counter bumped whenever an input changes; it is how
  salsa knows a cached result might be stale.

**2.**

1. Keyed on content, so one keystroke is a total miss.
2. Caches one step (the parse) and nothing downstream.
3. Cannot express cross-file dependencies, so it cannot know that `main.py`'s
   analysis survives an edit to an unrelated module.

Keying on identity + revision fixes **1** directly: the key is "this file, this
revision", so the *cost* of an edit is proportional to what actually depends on
it rather than to the size of the edit. Fixes 2 and 3 come from the dependency
graph, which is the other half of the design.

**3.** Because different layers can safely share different amounts.

Two programs targeting the same Python version can share a **parse**, even if
their search paths differ — so parsing is keyed on `PythonFile` (file +
version). Two programs with equivalent resolver environments can share **module
resolution**. But **type inference** must not be shared, because the same
`import x` can resolve to different files — so inference is keyed on
`ProgramFile` (file + whole environment).

One handle would force the narrowest sharing everywhere, and you would lose the
parse cache across projects for no benefit.

**4.**

| query | handle | why |
|---|---|---|
| `source_text` | `File` | the bytes on disk do not depend on any Python setting |
| `parsed_module` | `PythonFile` | parsing depends on the target version (f-strings, PEP 695) and nothing else |
| `SemanticModel::new` | `ProgramFile` | inference depends on search paths, platform, version — the whole environment |

**5.** Because ty can **drop a parsed AST under memory pressure and re-parse it
later**. Handing out a `&ModModule` directly would pin the tree in memory for as
long as the borrow lived. `.load(db)` materialises the AST for the duration you
need it and lets ty reclaim it afterwards. This is also why you must not stash a
`ParsedModuleRef` in a long-lived struct.

---

**6.**

| query | re-runs? | why |
|---|---|---|
| `source_text(main.py)` | no | different input, untouched |
| `parsed_module(helpers.py)` | **yes** | directly reads the changed input |
| `parsed_module(main.py)` | no | parsing is per-file; imports do not affect syntax |
| `line_index(models.py)` | no | unrelated file |
| inference for `main.run` | **yes** | `run` calls `shout`, so its types depend on `helpers.py`'s contents |

That table *is* the mental model. Syntax dependencies stop at the file
boundary; semantic dependencies follow imports.

**7.** Backdating: when a query re-runs and produces a result **equal** to the
previous one, salsa marks the result as unchanged as of the new revision, so
queries that depend on it are *not* re-run.

A whitespace-only edit changes `source_text`, forcing `parsed_module` to re-run
— but the resulting AST is equal (whitespace is not in the AST), so everything
above the parse is spared. You pay for one parse, not for a cascade. It is a
good demonstration that the AST's lossiness is not only a cost.

**8.** Every subsequent query serves the pre-write content. In exercise 10 that
means: you inject IDs, write the file, and then read back the *old* text — so
your node IDs stay `None` forever and injection appears to have done nothing.

Hard to notice because nothing errors, the file on disk is visibly correct, and
a test that re-opens a fresh database passes. Only a test that does
**write-then-query on the same db instance** catches it.

---

**9.** Because a `&mut` borrow means salsa is about to change the input state
that in-flight queries are reading; their results would be inconsistent. Salsa
signals cancellation by making the running queries **panic**, and catching that
unwind at the query boundary.

That is why `panic = "abort"` is forbidden (exercise 00): it turns a routine
cancellation into a process kill.

**10.** The `didChange` handler takes `&mut db`, which cancels the in-flight
`resolve_calls`. Your handler must catch the cancellation (do not treat it as a
crash), and **retry the request against the new revision** — or return a
response telling the client the result is stale. What it must not do is report
an error to the user, because nothing went wrong.

This is a behaviour your Python driver never had to have, and it is worth
building in early rather than discovering under load.

**11.** A cheap **reference-counted handle** to the same shared storage — not a
deep copy. You can hand clones to other threads and run read-only queries in
parallel, all sharing one cache, so work done on one thread is visible as a
cache hit on another.

You cannot mutate through a clone, and you cannot hold one across a mutation:
mutation needs `&mut`, which requires that no other handle is alive.

---

**12.** A warm `parsed_module` is a hash lookup against completed work, so the
ratio is usually two to four orders of magnitude, not one. If your warm number
is only a few times faster, you are probably re-doing `.load(db)` work or
timing the wrong thing — check that you are not rebuilding the `LineIndex` or
re-walking the AST inside the timed section.

**13.** You will usually see a real but sub-linear speedup. The limiting factors
are the shared cache's synchronisation and the fact that four small files is not
much work.

The important half of the question: **it is not the same factor that limits your
Python driver.** There, the limit is the GIL — `run_in_threadpool` gives
concurrency for I/O and nothing for CPU-bound inference. Here the work genuinely
runs on four cores. Scaling the fixture up to a few hundred files makes the
difference obvious; four files does not.

**14.** Nothing about the fifth file is recomputed. Salsa is keyed on the
**dependency graph it recorded**, not on the file set or the revision counter
alone — a bumped revision does not invalidate anything by itself, it just makes
salsa *check* whether each cached result's dependencies changed.

This is the property that makes a long-lived server viable: a project with
10,000 files and one edit does approximately one file's worth of work.

---

**15.**

| thing | tracked? | reason |
|---|---|---|
| syntax node tree for `parse_file` | **yes** | pure function of one file; re-requested constantly by the editor; big win |
| MRO / base classes | already tracked by ty | you get it free — do not wrap it in your own cache |
| context-sensitive call tree | **no** | its input is a *path environment*, not a file. Keying a salsa query on an unbounded, rarely-repeating value fills the cache with entries that are never hit again |
| project-code filter | not worth it | a path prefix comparison; cheaper to compute than to look up. Cache the *project root*, not the answers |

**16.** **Untracked entry points, tracked primitives** — the entry point stays a
plain function, and incrementality comes from the salsa-cached queries it calls
(`parsed_module`, `semantic_index`, inference).

Tracking the entry point would be harmful because salsa memoises on the
*arguments*. If one argument is a call-site environment — a map of parameter
names to abstract values — then almost every call has a distinct key. You get a
cache that stores everything, hits nothing, and never releases memory: strictly
worse than no cache. `plan/03-call-tree/08` is where this is dealt with
properly, using your own memo table with a key you control.

**17.** Wrong thing to store: any `'db`-bearing value — `Type<'db>`,
`Definition<'db>`, `ProgramFile<'db>`, a `ParsedModuleRef`. They are valid for
one borrow of one revision, and the lifetime is there to stop you.

Right key: something **stable and owned** — `(file path or File, TextRange)`, or
a qualified name. Re-derive the ty values from that key on each use; deriving
them is a cache hit anyway.
