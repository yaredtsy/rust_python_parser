# 09 — The IDE layer, and where it stops

**Goal:** you have run ty's own call-hierarchy machinery, you can describe
exactly what it gives you, and you have an input in your hand where it is
correct and useless for your purposes.

This exercise is mostly **reading someone else's code**, which is a skill this
port needs more than it needs any single API.

> `ty_ide` ships a `call_hierarchy` module. It will look like the answer.
> **It is not.** — `plan/README.md`

You are here to find out precisely *how* it is not, empirically, so that the
70% of the plan you have not read yet makes sense.

---

## Read first

- `tutorial/11-reading-the-source.md` — how to navigate 54 crates
- `plan/03-call-tree/02-why-ty-alone-cannot.md` — **read it after step 3**, not
  before. The exercise is more useful if you find the gap yourself first.

---

## What `ty_ide` gives you

**[verified]** exports, from `ty_ide/src/lib.rs`:

```rust
pub use call_hierarchy::{CallHierarchyItem, prepare_call_hierarchy};
pub use call_hierarchy::outgoing_calls::{OutgoingCall, outgoing_calls};
pub use call_hierarchy::incoming_calls::{IncomingCall, incoming_calls};
pub use goto::{goto_declaration, goto_definition, goto_type_definition};
pub use symbols::{FlatSymbols, HierarchicalSymbols, SymbolId, SymbolInfo, SymbolKind};
pub use document_symbols::document_symbols;
pub use find_references::find_references;
pub use hover::hover;
pub use type_hierarchy::{…};
// …and about twenty more
```

The two signatures that matter:

```rust
pub fn prepare_call_hierarchy(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize)
    -> Option<Vec<CallHierarchyItem>>;

pub fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize)
    -> Vec<OutgoingCall>;

pub struct CallHierarchyItem {
    pub name: Name,
    pub kind: SymbolKind,
    pub detail: Option<String>,      // containing module
    pub file: File,
    pub full_range: TextRange,
    pub selection_range: TextRange,  // the stateless key the LSP client re-sends
}

pub struct OutgoingCall {
    pub to: CallHierarchyItem,
    pub from_ranges: Vec<TextRange>,   // ← note: a Vec. calls are GROUPED by callee
}
```

Read those five fields of `OutgoingCall` again. Three properties of the design
are visible right there, before you run anything:

1. **It takes an offset, not a path.** The only input is "where is the cursor".
   There is nowhere to put "and I got here from `run_json`".
2. **It returns one level.** No recursion. The LSP client drives the tree by
   asking again with a new offset — which is why `selection_range` is documented
   as "the stateless key the client re-sends".
3. **It groups by callee.** `from_ranges` is a `Vec` because two call sites to
   the same callee collapse into one `OutgoingCall`.

Property 3 is interesting, because it is *almost* your quirk 6 (`add_child`
dedupes by qname and increments `call_count`). Same idea, arrived at
independently, for the same reason: a hierarchy view wants one row per callee.

---

## Build it

### Step 1 — read `outgoing_calls.rs` before writing anything

797 lines. `plan/05-reference/api-cheatsheet.md` says read it first, and that is
right.

You do not have it checked out — read it on GitHub at the pinned revision:
`github.com/astral-sh/ruff/blob/ac201b8/crates/ty_ide/src/call_hierarchy/outgoing_calls.rs`,
or generate docs with `cargo doc -p ty_ide --no-deps --open` for the shapes.

Read for these five things specifically, and write down where each lives:

1. **The prologue.** `parsed_module(db, file.python_file(db)).load(db)` then
   `SemanticModel::new(db, file)`. Every analysis starts this way — yours too.
2. **How it finds the starting definition.** `find_goto_target(&model, &module,
   offset)`, then definitions from the target.
3. **`OutgoingCallsFinder`** — the AST visitor. Compare its structure to the
   scanner you wrote in exercise 02. What does it do that yours does not?
4. **How a callee is resolved** — where does it go from an `ExprCall` to a
   definition? That function is the one you will keep reaching for.
5. **What it does NOT do** — no recursion, no environment, no filtering by
   project, no budget.

Point 3 is the payoff. You have already written a scope-walking visitor; reading
a mature one written by people who do this full-time will teach you more per
minute than any tutorial.

### Step 2 — run it

Wire a command:

```
pylspt-dev outgoing <file> <line>:<col>
```

Convert line/column to an offset (exercise 01, in reverse — `LineIndex::offset`
exists), call `outgoing_calls`, print each `to.name`, `to.detail`, `to.file` and
the `from_ranges` count.

Run it on every function in `python/tree.py`. Then on exercise 07's
`context.py` and exercise 08's `attributes.py`.

### Step 3 — the comparison table

For each entry point, compare what `outgoing_calls` returns against what your
call tree must produce. Fill this in from real output:

| entry | `outgoing_calls` gives | your tree must give | same? |
|---|---|---|---|
| `twice` | | `leaf` once, `call_count: 1` | |
| `recurse` | | `recurse` once, then stop (ancestor guard) | |
| `ping` | | `pong` → `ping` → stop | |
| `calls_undocumented` | | `leaf` only — `no_id_callee` dropped | |
| `diamond` | | `left`→`leaf` **and** `right`→`leaf`: two `leaf` nodes | |
| `constructs` | | `Constructed` with `ClassSchema/…` id, descend into `__init__` | |
| `emit` (context.py) | | depends on the caller | |
| `dispatch` (attributes.py) | | depends on the constructor path | |

Six of these differ, and they differ for **six different reasons**. Name each
reason. That list is your specification for what you are building on top of ty,
and it is more concrete than any prose description of it.

### Step 4 — the one that matters

Take `context.py`. Call `outgoing_calls` at the offset of `emit`.

Look at what comes back for `writer.write(data)`.

Now answer:

- How many `OutgoingCall`s did you get for that one call site?
- Is either of them wrong?
- Under `run_json → emit`, which one belongs in your tree?
- Is there **any** argument you could pass to `outgoing_calls` that would give
  you only that one?

The last question is the point of the exercise. The answer is no, and the reason
is not that the function is missing a parameter — it is that the function is
answering "what could this call reach", which is a different question from "what
does this call reach on this path".

**Now** read `plan/03-call-tree/02-why-ty-alone-cannot.md`.

### Step 5 — steal the good parts

You are not using `outgoing_calls`, but you *are* using its skeleton. List the
pieces you will reuse:

- the two-line prologue
- `find_goto_target` for offset → definition (your RPC takes positions too)
- the callee-resolution step from step 1, point 4
- `CallHierarchyItem::from_definition`'s approach to building a display item
  from a definition — you need the same thing for `target_qname`/`target_id`

And the pieces you will not:

- the traversal (you need project filtering, budget, recursion)
- the grouping (yours is by qname with `call_count`)
- the entry-point shape (yours is recursive and environment-carrying)

Write both lists down. When you start `plan/03-call-tree/03`, this is the
inventory of what already exists.

### Step 6 — the rest of `ty_ide`, quickly

Spend twenty minutes running the others against your fixtures. You are not going
to use most of them, but knowing they exist prevents you from writing them:

| function | try it on | why you care |
|---|---|---|
| `goto_definition` | any call site | the resolution you keep reimplementing |
| `document_symbols` | `tree.py` | compare to your exercise-02 node tree — very close to `parse_file`'s output |
| `find_references` | `leaf` | the reverse direction; `incoming_calls` builds on it |
| `hover` | any expression | a fast way to see ty's own rendering of a type |
| `incoming_calls` | `leaf` | what "who calls me" looks like, for contrast |

`document_symbols` is the one worth a real look. It produces a hierarchical
symbol tree from a file — the same shape as your `parse_file` nodes. Compare
them field by field and note where they differ: what does it include that you
drop, and what do you include that it does not?

---

## Traps

- **Concluding ty_ide is badly designed.** It is a correct implementation of the
  LSP call-hierarchy protocol, which is stateless, path-free and client-driven
  by design. Your requirement is unusual, not its shortcoming.
- **Trying to bolt an environment onto `outgoing_calls`.** Even with the source
  in front of you, the resolution it performs runs through `infer`, which is
  cached per scope. There is no seam.
- **Copying its traversal.** It has no project filter, no budget, no ID lookup
  and no cycle guard, because it never recurses. Yours needs all five.
- **Skipping the reading.** Step 1 is the exercise. Steps 2–4 confirm what you
  read.

---

## Done when

- [ ] you can name the five things in `outgoing_calls.rs` from step 1, by location
- [ ] `pylspt-dev outgoing` works on a file + position
- [ ] the step-3 table is filled in with real output
- [ ] you named six distinct reasons your tree differs
- [ ] you can state, in one sentence, why no argument to `outgoing_calls` helps
- [ ] you have a written list of what to steal and what to build
- [ ] you compared `document_symbols` to your own node tree

---

→ [`exam.md`](exam.md), then [`../10-lossless-edits/README.md`](../10-lossless-edits/README.md)
