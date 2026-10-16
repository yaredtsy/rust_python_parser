# 10 — Lossless edits and ID injection

**Goal:** you can put a UUID into every def and class in a file by **inserting
bytes**, not by rewriting it; the second run changes nothing; and the outline you
hand to v-noc is always computed from the text *after* the write.

This is the port of `id_injector.py` (185 lines) and `file_folder_ids.py` (82).
It is the only part of the driver that **writes to the user's source files**, so
it is the part where a bug is expensive — and the only part that cannot live
inside salsa.

---

## How this exercise is organised

Four objects, one file each, then the assembly. Same shape as exercise 02:
lesson → API → worked examples → exercise → its own exam → answers.

| | file | object | what it gives you |
|---|---|---|---|
| 1 | [`01-object-def.md`](01-object-def.md) | `Def`, `child_defs` | ★★ **the one rule for which defs exist** — shared with collection |
| 2 | [`02-object-slot.md`](02-object-slot.md) | `DocSlot`, `SlotKind` | where the ID goes, in all eleven shapes |
| 3 | [`03-object-edit.md`](03-object-edit.md) | `Edit`, `plan_ids`, `apply` | plan then apply, bottom-up, with injectable IDs |
| 4 | [`04-object-purity.md`](04-object-purity.md) | `File::sync`, the four-step order | ⚠ why this is **not** a salsa query |
| 5 | [`05-putting-it-together.md`](05-putting-it-together.md) | — | the two RPCs, `FileID`/`FolderID`, the parity table |

Then [`exam.md`](exam.md) for the whole exercise.

**Objects 1 and 4 are the core.** Object 1 is the thing two passes must agree
on; object 4 is the reason there are two passes. Objects 2 and 3 are the shapes
and the mechanics, and they are where your time will actually go.

---

## Read first

- `plan/02-mapping/02-id-injection.md` — the whole chapter, including the
  libcst design this exercise replaces (object 5 has the comparison)
- `plan/00-orientation/01` §"Side effects" — three RPCs write to disk
- `experience/02-parse-and-ast/06-object-docstrings.md` — the **read** side. You
  cannot write a key you cannot read back, and the reader is looser than you
  think (object 2's `prose_mentions_id`)
- `experience/02-parse-and-ast/08-staged-pipeline.md` — where this pass sits in
  the pipeline, and the `definitions` query it shares a rule with

---

## The mental model, in one page

### Injection is its own pass. It must share one thing with collection.

```
                  child_defs(body)          ← ★ ONE rule, two callers
                   ┌──────┴───────┐
                   ▼              ▼
          outline(body)      plan_ids(body, source, new_id)
          (in salsa:          (outside salsa: random, writes)
           pure, cached)             │
                   ▲                 ▼
                   │            apply → write → File::sync
                   └─────────── then collect again, on the new text
```

### Why injection cannot be a salsa query

**1. Tracked queries must be pure.** Same input, same output, no effects
outside themselves. Injection breaks both halves: it writes files, and it mints
*random* UUIDs, so two runs on one file disagree. Inside `definitions` that is
not merely untidy — salsa compares `old == new` to decide whether anything
downstream must re-run
(`salsa-macros/src/tracked_fn.rs:103-115` **[verified]**), so a random result
is never equal, every rerun invalidates everything beneath it, and the same def
can be handed two different IDs on two reads of the same file.

**2. A query can be cancelled mid-flight.** Salsa cancels in-progress queries
when another thread takes `&mut db`, by unwinding
(`ruff_db::panic`, `salsa::Cancelled`). A pure query loses nothing when that
happens. A query that was half-way through writing a file loses the file.

**3. It needs data the outline does not.** To insert, you need the closing
quotes' offset, the quote style, the body's indentation, and whether the body
shares a line with the `def`. Putting that in `DefNode` makes every cached
result bigger and makes every `old == new` compare fields nobody else reads.

**4. It runs at a different time.** Collection runs on every read of the
outline. Injection runs once per file, only when a key is missing — and it
shifts every range after the insert, so it has to finish *before* you compute
the outline you send to the graph.

A second pass costs **no second parse**: both read the same salsa-cached
`parsed_module`, and a statement-only walk is cheap.

### Why the "which defs exist" rule must be shared

Give the two passes their own walks and they can disagree. Suppose collection
looks inside `if TYPE_CHECKING:` and injection does not:

> That def appears in the outline with `id: null`, forever. It never becomes a
> graph node. Nothing reports why. The file on disk looks fine, the outline
> looks fine, and the def is simply invisible to v-noc.

The failure is silent in both directions and permanent in one. So the rule lives
in **one function** — `child_defs` (object 1) — and both passes call it.
`python/shared_rule.py` is sixteen defs in sixteen statement positions, and it
exists to fail loudly when the two walks drift apart.

---

## The fixtures

```
python/
├── shared_rule.py ...... ★ 16 defs in if/try/else/finally/with/for/while/match,
│                         nested in a function and in a class. The agreement test.
├── shapes.py ........... ★ 14 defs, NO ids: every insert shape in one file
├── decorated.py ........ 5 decorated defs, no docstrings: body[0] starts at the `@`
├── idempotent.py ....... looks done and is not, looks undone and is not
├── no_docstring.py ..... 7 defs, nothing has a docstring: each needs one created
├── has_docstring.py .... 7 defs, 5 unkeyed. The parity fixture: r"""…""", """ in content
├── crlf.py ............. CRLF line endings (verified: 9 CRs)
├── tabs.py ............. tab indentation (verified: 7 tabs)
├── nonascii.py ......... accented identifiers, emoji docstring, coding line
└── minimal.py .......... `pass`, `...`, `def stub(): ...`
```

| fixture | what it decides |
|---|---|
| `shared_rule.py` | do collection and injection find the *same* sixteen defs |
| `decorated.py` | `body[0].start()` is the `@`, so where does a new docstring go |
| `shapes.py` | does every insert shape parse afterwards, and read back |
| `idempotent.py` | is "needs an ID?" exactly the reader's answer |
| `minimal.py` | one-line bodies — `def stub(): ...` is not a block |
| `crlf.py`, `tabs.py` | does the inserted text inherit the file's style |
| `nonascii.py` | offsets are **bytes**; an emoji before the insert point moves it |
| `has_docstring.py` | the two Python bugs you now *diverge* from (object 5) |

---

## Build it

Each step has a fixture that fails first.

1. **`child_defs`, and nothing else.** Print the sixteen names of
   `shared_rule.py` with their nesting. Then make `definitions` (exercise 02,
   file 8) call it too, and diff the two def lists. Object 1.
2. **`DocSlot` for every def in `shapes.py`.** Print the offset, the kind, and
   the byte the offset points at. Fourteen rows, no `unwrap`. Object 2.
3. **`plan_ids` with a counter, not a UUID.** `id-1`, `id-2`, … so the output is
   the same on every run and can be snapshotted. Object 3.
4. **`apply`, bottom-up.** Write to a *copy*, then `python3 -c "import shapes"`.
   Then apply ascending on purpose and watch where the second insert lands.
5. **The sync.** Inject, write, `File::sync`, re-query. Then comment out the
   sync and watch the IDs stay `None` with the file visibly correct on disk.
   Object 4.
6. **Idempotency.** Run twice over all ten fixtures: run two makes zero edits.
   `idempotent.py` is the one that catches a reader/writer mismatch.
7. **`FileID` and `FolderID`.** Module docstrings, and `__init__.py` creation.
   Object 5.
8. **The guards.** `--write` off by default, and never write outside the
   project. Object 5.

---

## Done when

- [ ] `child_defs` is the only place either pass decides what a def is
- [ ] collection and injection agree on all 16 defs of `shared_rule.py`
- [ ] every def in `shapes.py` gets a key, and the file still imports
- [ ] `crlf.py` gains CRLF-terminated inserts, not LF ones
- [ ] a multi-line docstring gains no trailing-whitespace line
- [ ] `decorated.py`'s class docstring lands above the `@`, not below it
- [ ] the `ends_with_colon` reader divergence is written down
- [ ] `plan_ids` takes its ID source as a parameter, and tests pass a counter
- [ ] edits are applied bottom-up (or by a builder that needs no order)
- [ ] the outline sent to the graph is computed **after** the write, every time
- [ ] run two over all ten fixtures makes zero edits
- [ ] `prose_mentions_id` is skipped, and you have written down whether that is
      correct
- [ ] you have seen the missing-`sync` failure with your own eyes
- [ ] `--write` defaults to off, and injection refuses to leave the project
- [ ] the divergence from `id_injector.py` is written down with its two fixtures

---

→ Start: [`01-object-def.md`](01-object-def.md)
