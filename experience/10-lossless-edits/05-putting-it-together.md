# 05 — Putting it together: the injector

Four objects, one module, and the three RPCs that write to disk.

---

## The module

```
src/
├── ids/
│   ├── mod.rs        Def, child_defs                     ← object 1 (shared!)
│   ├── slot.rs       DocSlot, SlotKind, doc_slot          ← object 2
│   ├── edit.rs       Edit, plan_ids, apply, line_prefix   ← object 3
│   └── inject.rs     the four steps, the guards           ← object 4
└── defs.rs           definitions(db, file) -> Vec<DefNode>
                      ← exercise 02 file 8; CALLS ids::child_defs
```

`child_defs` lives in `ids/mod.rs` and `defs.rs` depends on it, not the other
way round. That direction matters: the shared rule is the lower-level thing, and
putting it in the salsa module would invite someone to make it a query.

**One test guards the whole arrangement:**

```rust
#[test]
fn collection_and_injection_see_the_same_defs() {
    for fixture in FIXTURES {                       // all ten
        let from_outline = flatten_names(&outline(body));
        let from_plan = plan_names(body);           // child_defs, same recursion
        assert_eq!(from_outline, from_plan, "{fixture}");
    }
}
```

If that test ever has to be deleted to make something else pass, the thing it
was protecting has already broken.

---

## The three writing RPCs

`plan/00-orientation/01` §"Side effects":

| RPC | writes | key | prefix |
|---|---|---|---|
| `parse_file` | the file, if any def lacks a key (`scanner.py:24`) | `ID` | `FunctionSchema/` · `ClassSchema/` |
| `read_or_inject_file_id` | the **module** docstring | `FileID` | `FileSchema/` |
| `read_or_inject_folder_id` | `__init__.py`, **creating it if absent** | `FolderID` | `FolderSchema/` |

### The module is the one def-like thing `Def` cannot hold

A module has a docstring and a key, and it is not a `Stmt`. Two consequences:

- `doc_slot` works on it unchanged — it takes a `&[Stmt]`, and
  `module.syntax().body` is one. Only `child_defs` and `Def` do not apply.
- **Two injectors can want the same docstring.** `parse_file` injects `ID:` into
  defs; `read_or_inject_file_id` injects `FileID:` into the module docstring.
  They never collide today because the module is not a def — but if you ever
  decide the module should also carry an `ID:`, both keys land in one literal at
  one offset, and `plan_ids`' no-two-edits-share-an-offset invariant (object 3)
  stops being true. Keep them separate, or merge them into one planner.

`idempotent.py`'s module docstring carries `FileID` and **no** `ID` — verified
with the Python driver's own regex, the reader finds `{'FileID'}` only. That is
why the lookup is key-exact: a `FileID` must not satisfy an `ID` check, and
exercise 02's `multiple_keys` fixture is the same rule from the read side.

### `__init__.py` creation, and why `sync_path`

`read_or_inject_folder_id` creates the file when it is missing. That is
load-bearing for v-noc's folder identity, and it is the one case where the file
did not exist a moment ago — so it needs `File::sync_path` (which invalidates the
containing **directory listing**) and not `File::sync` (which does not):
`files.rs:456` vs `:478` **[verified]**. Use `sync` for a file you overwrote,
`sync_path` for a file you created.

### The error behaviour to reproduce

On any failure, `file_folder_ids.py:22` returns a **fresh random UUID with
`modified: false`** — a throwaway that is never persisted, so the next call mints
another one. It is odd. Reproduce it: it is observable behaviour at the wire, and
quirk 13 says a partial answer beats an error.

---

## ⚠ DECIDED — insertion, not libcst

`plan/02-mapping/02-id-injection.md` designs this chapter around the `libcst`
crate: parse a CST, insert a docstring node, `codegen` the module back, and take
newline and indent from `ruff_python_codegen::Stylist`. That is a reasonable
design and this exercise does not use it. The comparison, stated plainly:

| | libcst codegen | insert at an offset |
|---|---|---|
| bytes outside the docstring | re-emitted (preserved, but re-emitted) | **provably untouched** |
| `r"""…"""` prefix | `id_injector.py:70` emits `f'"""{content}"""'` — **drops the `r`**, changing what the string means | preserved: the prefix is at the other end of the literal from `at` |
| `"""` inside the content | rebuilt literal is **broken** | impossible; nothing is rebuilt |
| quote style, indent | from `Stylist`, or you mismatch the file | inherited from bytes you did not touch (`line_prefix`) |
| line endings | from `Stylist::line_ending()` | ⚠ **yours to get right** — the inserted newlines are ones you wrote, so detect with `find_newline` (object 3) |
| one-line bodies (`def f(): ...`) | a CST `Suite` shape question | one extra `\n` in the inserted text |
| new dependencies | `libcst` 1.8.4 + `ruff_python_codegen` | none but `uuid` |
| byte-parity with the Python driver | reproduces both bugs | **diverges on exactly those two inputs** |

The last row is the trade, and it is a real cost. `plan/02-mapping/02` says
"reproduce them, do not fix them" about the prefix loss and the broken rebuild —
a defensible rule for *output* parity, and the wrong rule here, because this
output is **the user's source file** and the Python's version of it is lossy.
Reproducing a bug that turns `r"""C:\new\table"""` into a string containing a
newline and a tab is not parity, it is damage.

So: take the insertion, and put the divergence where the parity gate can see it.
`plan/04-build/02-milestones.md`'s byte-identical test **cannot** cover
`has_docstring.py:raw_prefixed` or `has_docstring.py:contains_triple_quotes`,
because the two implementations disagree there by design. Those two fixtures are
the divergence; everything else in the ten must still match byte-for-byte.

`shapes.py:raw_prefixed` is the same shape with no key, so it is the one that
exercises the *insert* path rather than the comparison.

---

## ⚠ Still open: inject before reporting?

The four-step order (object 4) ends with a collect, so the outline is always
post-write. That means the tool *could* inject before it ever answers, and v-noc
would never see `id: null` at all.

Tempting, and not obviously right:

| | v-noc sees | cost |
|---|---|---|
| report `id: null`, inject next call ← today | a def with no key, once | v-noc must tolerate `null`, and probably already has logic that does |
| inject, then report ← possible now | every def keyed, always | every `parse_file` on a new file writes before answering; a read-only-looking RPC is now a write, always |

Exercise 02 object 6 is emphatic that `parse_file` must emit `id: null` rather
than dropping the node, because `null` is what *triggers* injection downstream —
so something in v-noc reads that field and acts on it. **Check what, before
changing when injection runs.** It is the same interface question as the
`parse_file` call-node decision in exercise 02 file 8, and it has the same
answer shape: look at the consumer first.

---

## Verify against the fixtures

For each, predict then check. "Still imports" means
`python3 -c "import <name>"` after injection.

| fixture | expected |
|---|---|
| `shared_rule.py` | 16 edits, 16 defs; collection and injection agree exactly |
| `decorated.py` | 5 edits; `Decorated`'s class docstring lands **above** `@property`, which is the only legal spot |
| **all ten** | 61 edits, 66 defs, 5 already keyed |
| `shapes.py` | 14 edits, one per def; still imports; all 14 keys read back in Rust — and `ends_with_colon` does **not** read back in Python (object 2) |
| `idempotent.py` | **2** edits — `has_other_keys` and `Mixed.needs_one`. Not `has_id`, not `Mixed`, and **not** `prose_mentions_id` |
| `no_docstring.py` | 7 edits — including `Gamma.Nested.deep`, three levels down |
| `has_docstring.py` | 5 edits; `already_has_id` and `Documented` untouched; `raw_prefixed` keeps its `r` ← **divergence** |
| `minimal.py` | 4 edits, of which **one** — `stub` — is the one-liner shape |
| `crlf.py` | 3 edits; inserted text uses `\r\n`; `file crlf.py` still says CRLF |
| `tabs.py` | 3 edits; inserted text indents with a **tab** |
| `nonascii.py` | 2 edits; the coding line survives, the emoji docstring is intact, and the key lands after it |
| any of them, run twice | run two: **0 edits** |

`idempotent.py`'s row is the one to argue about before you run it. Three of its
four defs look done and are not, or look undone and are not:

| def | reader finds | edit? |
|---|---|---|
| `has_id` | `ID` → a real uuid | no |
| `prose_mentions_id` | `ID` → `'nothing-real,'` | **no** — the prose *is* a key |
| `has_other_keys` | `Owner`, `Status`, no `ID` | yes |
| `Mixed` | `ID` → a real uuid | no |
| `Mixed.needs_one` | nothing | yes |

All five rows verified against `re.findall(r"(\S+)\s*:\s*(\S+)", doc)` — the
Python driver's own regex. `prose_mentions_id` is not a bug in your injector; it
is the reader's rule, shared by both passes (object 1), applied to an input
nobody intended. Write down whether you think that is correct, and what you
would change if not — the answer changes `_extract_metadata`, not the injector.

---

## Snapshot it

```
pylspt-dev inject --ids=counter --dry-run python/ --bless
```

With the counter from object 3 the output is identical on every run, so the
snapshot is reviewable as a `git diff`. Snapshot the **edit plan** (offset, kind,
text) and not just the resulting file: a plan diff tells you *which* decision
changed, where a file diff only tells you that the bytes moved.

⚠ As in exercise 02: generate the first comparison from the **Python driver**
where the two agree, and hand-check the two divergence fixtures. Snapshotting
your own output as the baseline snapshots your own bugs.

---

## Done when

- [ ] `child_defs` is the only place either pass decides what a def is, and one
      test asserts the two agree on all ten fixtures
- [ ] every def in `shapes.py` is keyed, the file imports, and all 13 keys read
      back through `id_from_body`
- [ ] `idempotent.py` takes exactly 2 edits, and you can say why for each of the
      five rows
- [ ] `crlf.py` and `tabs.py` keep their style; `nonascii.py` keeps its bytes
- [ ] run two over all ten fixtures makes **0** edits
- [ ] you have seen the missing-`sync` failure with your own eyes
- [ ] `--write` defaults off, and the injector refuses to write outside the
      project or into a file that did not parse
- [ ] `FileID` and `FolderID` work, including creating `__init__.py` with
      `sync_path`
- [ ] the libcst divergence is written down, with `raw_prefixed` and
      `contains_triple_quotes` marked as not byte-comparable
- [ ] the inject-before-report question has an answer, taken from what v-noc
      actually does with `id: null`

---

## What you have now

The write half of the join key. Exercise 02 object 6 reads `ID:`; this exercise
puts it there, in a file you can still import, with a second run that changes
nothing — and it does it without rewriting a single byte it did not mean to.

Combined with exercise 02 file 8, the pipeline is: **one cached parse, one
shared rule, two cheap walks, one write.** Everything the graph sees is computed
after the write.

→ [`exam.md`](exam.md), then [`../11-capstone/README.md`](../11-capstone/README.md)
