# Answers — final exam

Short answers, with pointers back to the exercise that covers each in full.

---

## Part 1 — The stack

**1.**

```
ruff_db::Db  →  ty_python_core::Db  →  ty_python_semantic::Db  →  ty_ide::Db
                                                                      ↓
                                                          ty_project::ProjectDatabase
```

Take the **narrowest trait you need** — `&dyn ty_python_semantic::Db`, not
`&ProjectDatabase`. It keeps your functions testable and independent of how the
database was built. *(ex 03)*

**2.**

| handle | keyed on | query |
|---|---|---|
| `File` | a path in salsa's filesystem | `source_text`, `line_index` |
| `PythonFile` | file + Python version | `parsed_module` |
| `ProgramFile` | file + full program environment | `SemanticModel::new`, `semantic_index` |

*(ex 03)*

**3.** `OsSystem` is behind `#[cfg(feature = "os")]` and `ruff_db` has no default
features. The plan did not mention it because under a vendored workspace a
sibling crate had already enabled it and cargo's feature unification handed it
over. **As the root crate, nobody enables features for you.** *(ex 00)*

**4.** Two copies of every crate in the graph; trait bounds that look satisfied
but are not; error messages where both sides print identically. Diagnose with
`cargo tree -d`. *(ex 00)*

**5.** To a `.pyi` stub in ty's **vendored typeshed** — a virtual filesystem
compiled into your binary. It has no system path, so any code assuming
`File → path` breaks there first. *(ex 05)*

---

## Part 2 — Text and trees

**6.**

```rust
line:       lc.line.get(),
column:     lc.column.to_zero_indexed(),
end_line:   end.line.get(),
end_column: end.column.to_zero_indexed(),
```
*(ex 01)*

**7.** `Utf32` — characters — implemented as `chars().count()` **[verified,
`line_index.rs:117, 210`]**. Right because parso counts code points, and Python
string indices are code points. `unicode.py` proves it: the `(` is at byte 9 and
character 8. *(ex 01)*

**8.** `b()` is index 0, `c()` is index 1. They share `line`/`column` (the start
of the whole chain, i.e. `a`) and differ in end position and `call_col_pos`.
*(ex 02)*

**9.** At the **`@`** **[verified]**. The second problem: `decorator_list` is a
field of `StmtFunctionDef`, so a visitor walking the def also walks the
decorators — putting calls inside a decorator *inside* the function it
decorates, where parso puts them in the enclosing scope. *(ex 02)*

**10.** Any three: nested defs terminate the scan (`TraversalSignal::Skip`);
lambdas dropped with their whole subtree (never emit, never walk); position
dedup (`FxHashSet<TextRange>`); no-ID callees dropped from the call tree
(docstring scan); `call_index` from chain depth. *(ex 02)*

---

## Part 3 — Meaning

**11.** CLI override → `[tool.ty.environment] python-version` → `requires-python`
**lower bound** → resolved environment → `latest_ty()` (3.14).

The surprise is the third: `>=3.9` gives you **3.9**, not the installed
interpreter. Correct for a type checker, wrong for a structural analyser.
*(ex 04)*

**12.** Flow-sensitive: the answer depends on *where* you ask. Context-sensitive:
it depends on *who called*. ty is flow-sensitive only.

```python
def emit(writer, data):
    writer.write(data)      # ty has one answer for all callers
```
*(ex 07)*

**13.** Module name + every enclosing **Function/Class** scope name + the
definition's own name, via `ancestor_scopes` — `mod.Outer.Inner.method`.

The naive answer uses `Definition::name` alone and gives `mod.method`, because
ty's name is the immediate name. Also filter scope kinds: `TypeParams`,
`Comprehension` and `Lambda` scopes must not contribute components. *(ex 06)*

**14.** `FunctionLiteral` (a resolvable callee), `BoundMethod` (callee +
receiver), `ClassLiteral` (constructor — quirk 7), `NominalInstance` (a
receiver), `Union` (fan-out), `Dynamic` (Unknown — fall back and count).
*(ex 07)*

**15.** `type_hierarchy_supertypes(db, env, ty) -> Vec<TypeHierarchyClass>`. It
returns the **direct explicit bases, one level**, with `object` special-cased —
empty for `object` itself, implicit for a class with no bases.

To match `py__mro__()` you must recurse it yourself and then **linearise (C3)**,
because recursion gives a DAG, not an order — and add `builtins.object` at the
tail. Or establish from the goldens that order is unobservable and skip C3.
*(ex 08)*

**16.** Correct because both `Handler` and `LoudHandler` really are assigned to
`self.handler` somewhere in the program. Unusable because on the path
`use_loud → build_loud → dispatch` it is one specific object.

A parameter environment is not enough: the value crossed frames **through an
object**, stored in `__init__` and read in `dispatch`. You need object identity
plus per-object attribute state. *(ex 08, and `plan/03-call-tree/06`, `/10`)*

---

## Part 4 — The gap

**17.** *Model answer.*

`outgoing_calls` answers "which functions could this line possibly call, in any
run of the program". That is the right question for an editor: when you click
"show me what this calls", you want every possibility, because you do not know
which one will happen today.

The call tree answers a different question: "when this exact function is reached
by this exact path, what actually gets called". If `emit` is called once with a
JSON writer and once with an XML writer, the editor should show you both, and
the call tree must show the JSON one under the JSON caller and the XML one under
the XML caller.

You cannot get the second answer from the first by filtering, because the
information needed to choose — which argument was passed at *this* call site —
was never part of the first question. It is not a missing feature; it is a
different computation over a different input.

**18.** Grouping vs `call_count`; no recursion; no project filter; no
builtin-by-name filter; no no-ID drop; no cycle guard; no constructor entry; no
budget — **and** path insensitivity.

Only path insensitivity is fundamental. Everything else is additive scaffolding
you build on top, which is the encouraging half of the finding. *(ex 09)*

**19.** Nearly every call would produce a distinct cache key, so the cache would
store everything and hit almost nothing. Worse than no cache because you still
pay memory, bookkeeping and dependency-validation costs on every revision — and
those entries degrade the incrementality of the queries that *do* repeat.
*(ex 07, ex 09)*

---

## Part 5 — Writing files

**20.** Because ruff's AST is lossy — no comments, no whitespace, no quote style
— so printing it back reformats the file. libcst is lossless by construction, so
untouched subtrees print byte-identically. Ruff hits the same wall and makes the
same choice in `ruff_linter/src/fix/codemods.rs` **[verified]**. *(ex 10)*

**21.** Compute → write → `File::sync_path(&mut db, &path)`. Omit the sync and
every later query serves pre-injection content: IDs stay `None` forever, the file
on disk looks correct, and nothing errors. *(ex 03, ex 10)*

**22.** Raw-prefix loss (`r"""…"""` → `"""…"""`, changing the string's meaning)
and `"""` inside content producing a malformed literal.

Reproduce because the contract is the observable output. A port that changes
behaviour cannot be verified against the thing it replaces — you lose the
ability to distinguish "I broke it" from "I improved it". Mark both
`// PARITY:`. *(ex 10)*

---

## Part 6 — Judgement

**23.** The forcing requirement is **member lookup on a receiver you chose** —
`self.handler.handle()` where the path decided what `handler` is.
`Type::static_member` is private, and every public tool takes syntax and infers
the receiver itself.

Decide at **M1**, per `plan/04-build/02`'s risk register: "deciding Option A/B
too late" is one of the two risks that sink the project. Not earlier, because
before exercises 07–08 you have no evidence about how often it actually matters.
Not later, because M6.4 is the wrong moment to discover you need a fork —
restructuring an interpreter around a different value domain mid-build is the
expensive version.

**24.** *For:* real usage surfaces parity issues faster than any test suite; the
tree's shape, IDs and `call_count` merging are all exercised; three RPC methods
are already correct and much faster.

*Against:* a context-free tree is **plausible** — right shape, right names, wrong
details — so users trust it and report nothing. The plan's own word for M5's
output is "plausible", which should read as a warning.

*Decision:* ship M1–M4 behind a flag; do **not** ship M5's `resolve_calls`. The
distinction is whether a wrong answer is visibly wrong. `parse_file` and `mro`
fail loudly; a call tree fails silently.

**25.** Log it in the divergence log, reproduce the current behaviour, mark it
`// PARITY:`, and raise it with whoever owns v-noc **after** parity is
established — not before.

Reason: until the outputs match, you cannot tell your bugs from the original's,
so a fix now costs you the ability to verify everything else. Once parity holds,
the fix is a small, reviewable diff against a known-good baseline. *(the raw-prefix
bug and the phantom folder UUID are both in this category)*

**26.** No answer key. The two most commonly rushed are **03** (people read about
salsa instead of measuring it) and **09** (people read the plan's conclusion
instead of running `outgoing_calls` themselves). Both are cheap to redo and both
change how the rest reads.

---

## Part 7

**27.** No key — they are your numbers. What they are for:

| number | decides |
|---|---|
| cold vs warm `parsed_module` | whether caching is working at all |
| sequential vs parallel scan | whether M8's parallelism is worth it for your workload |
| single-callee vs union ratio | the honest scope of `plan/03-call-tree/` |
| Python vs Rust on the same files | whether "10–100×" is real, and where the remaining time is |

If any of the four is missing, that is the next thing to measure — before
writing another line of the call tree.
