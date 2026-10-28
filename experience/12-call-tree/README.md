# 12 — Build the call tree, step by step

**Goal:** you write `src/interp/`, the call tree analysis, one small step at a
time. Each step adds one feature, runs one fixture, and explains the decision
the code makes at the moment you type it.

You start with the simplest analyser that works: follow every call, the way the
Jedi driver does. Then you feel why that does not scale, and replace following
with **summaries**: each function walked once per distinct input. First for
plain functions, then for classes.

---

## The order

```
12-call-tree/
├── 00-values/               the finished structures. Where you are heading. Skim it
│                            now, come back to it whenever a step names a struct
│
├── 01-functions/            INLINE MODE. Follow every call, like Jedi
│                            one call → locals → imports → following → recursion
│                            → arguments → *args → branches → returns → closures
│                            → callbacks → the wall
│
├── 02-function-summaries/   SUMMARY MODE for functions. Same trees, each body
│                            walked once per key
│                            the key → the summary → the solver and the graph
│                            → relevant parameters → recursion → closures → cuts
│
├── 03-classes/              objects, in INLINE MODE first
│                            constructing → methods and self → attribute state
│                            → two instances → writes from anywhere → static,
│                            class, property → chains, super, annotations
│
├── 04-class-summaries/      SUMMARY MODE for objects
│                            objects in keys → objects coming back → effects
│                            → resolve, then write → deep writes → split by type
│                            → aliases → recursion with objects → retiring inline mode
│
└── 05-output/               the wire format: stable IDs, the graph, the tree
                             v-noc stores, the checks, the benchmark
```

Why this order:

- **Basic first.** Inline mode is short, and every answer it gives is exact for
  the path it follows. You learn the walker (values, env, binding, branches,
  closures) without keys, summaries or recursion groups in the way.
- **Summaries when you need them.** Step `01.13` builds a fixture where inline
  mode walks the same function with the same inputs thousands of times. Part
  02 is the fix, and every decision in it answers "how do we stop repeating
  that walk without changing the tree".
- **Functions before objects.** A function value has no identity and no state,
  so the first summaries only need returns and edges. Objects add identity and
  writes, which is where most of the design's difficulty lives. They get their
  own two parts.

## Two modes, one walker

| | inline mode | summary mode |
|---|---|---|
| a call to a project function | walks the callee's body right there, with the caller's values | gets the callee's summary for its key, and applies it |
| a function reached on 1000 paths | walked 1000 times | walked once per distinct key |
| the tree | built while walking | rendered from the graph of contexts |
| built in | parts 01 and 03 | parts 02 and 04 |

Both modes share everything else: values, the env, binding, branches, member
lookup. Only "what happens at a call" and "where the tree comes from" differ.
Inline mode stays until the end of part 04. Until then it is the reference that
summary mode is checked against: on every fixture, both must give the same
tree, apart from differences each step names. Part 04's last step decides
whether it stays on as a test oracle.

---

## How a step works

Every step file has the same shape:

1. **The fixture.** A Python file in that part's `python/` folder, with the
   expected tree written in comments below the code.
2. **Predict.** Two or three questions. Write the answers down before reading
   on.
3. **Build.** Each decision gets its own block: what the code does, why, and
   the alternative it rejects. The code where the decision is made is shown.
   The routine glue is yours to write.
4. **Run.** The test that must pass, and what its output looks like when it
   does not.
5. **Where you are.** The state of the main structs after this step.
6. **Exercise**, with answers folded away.

A full reference for the step's code is folded at the end of **Build**. Write
your own first. Open the reference when you are stuck, or to compare after
your test passes.

> The reference code is written against ty at `ac201b8` but has not been
> compiled. Accessors not yet confirmed are marked **[check]**, as in the plan.
> When one is wrong, fix it in `src/sem.rs`, the only file that talks to ty.

### Fixtures only ever get stricter

Once a fixture passes, it stays in the test suite and must keep passing for the
rest of the tutorial. No later step edits an earlier fixture's expected tree.
When summary mode joins a fixture, a step tells you to add `Summary` to that
fixture's test, and the same expected tree must come out.

Where the two modes are allowed to differ, the fixture says so with a
mode-specific block (`# expect inline:` / `# expect summary:`, from step
`02.05`). Each one is explained in the step that adds it.

### The expected-tree format

```python
def helper(): pass
def main():
    helper()

# expect:
# main
#   helper
```

- `# expect:` starts a block. The first line is the entry function, and each
  further line is one tree node, indented two spaces per level.
- Names are qualified names with the fixture's own module dropped:
  `helper`, `Pipeline.run`, `make.run` (a nested def), `util.helper` (another
  module).
- Marks go in brackets after the name: `f [recursive]`, `top [incomplete]`.
- A file can hold several blocks, one per entry.

---

## Where this sits in the plan

The plan (`plan/03-call-tree/`) describes the finished design, summary mode
only. This tutorial reaches it by a different road:

| tutorial | plan build order |
|---|---|
| 01, 03 (inline mode) | not in the plan: a scaffold, and later the oracle |
| 02.01–02.03 | S1, S2, S3 (functions only) |
| 02.05–02.06 | S4 |
| 02.07 | S7 |
| 02.08 | S8 |
| 04.01–04.05 | S3 (objects) |
| 04.06 | S6 |
| 04.08 | S4 (objects) |
| 05 | S5, S9, S11 |

Read `plan/03-call-tree/03-design.md` once before starting. You will come back
to it after every part, and it will read differently each time.

---

## There is nothing to run against yet

`src/interp/` does not exist. Step `01.01` creates it. The checks are:

1. **your tests**, from the fixtures, one per step;
2. **the Jedi driver**, as ground truth for anything a step marks **[check]**:
   `/Users/yared/Documents/Programing/ide/v-noc/src/lsp/py/vnoc_lsp_python/call_resolver.py`.
   Remember the two differences on purpose: Jedi drops recursive calls
   (pylspt shows `[recursive]` leaves), and Jedi's tree is already merged
   (pylspt sends one node per call site).

---

## Done when

- [ ] every fixture in parts 01–04 passes in summary mode, and in inline mode
      where it is still kept
- [ ] you can explain each decision in the code to someone else, with the
      fixture that breaks without it
- [ ] `plan/03-call-tree/` reads like a description of code you wrote

---

→ Start: [`01-functions/README.md`](01-functions/README.md)
