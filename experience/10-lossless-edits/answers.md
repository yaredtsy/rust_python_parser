# Answers 10 — Lossless edits and ID injection

Each object file has its own answers for its own exam; these are for
[`exam.md`](exam.md).

---

## The shared rule

**1.** "Which defs exist in this scope." `child_defs` owns it
([object 1](01-object-def.md)). The callers are `outline`/`definitions`
(collection, inside salsa) and `plan_ids` (injection, outside it).

**2.** Collection sees it and injection does not: the def sits in the outline
with `id: null` **forever**, never becomes a graph node, and emits no error.
Injection sees it and collection does not: an unread key appears in a docstring
— untidy, harmless. The first is dangerous because the only symptom is absence,
and absence is not something you can grep for. "It errors" is not an answer to
either because nothing is in an invalid state — both halves are internally
consistent, they just disagree.

**3.** Because both passes are **syntactic**. `child_defs` reads the AST; it has
no idea which branch runs. A def inside `case _:` exists in the file, needs a
key in the file, and gets read back from the file. Execution never comes into
it.

**4.** `idempotent.py:prose_mentions_id`, whose docstring says
"The text says ID: nothing-real" and has no real key. The reader's regex finds
`ID` → `'nothing-real,'`, so collection considers it keyed. A stricter injector
— "is there a line beginning `ID:`?" — would inject a second key. The reader
takes the **last** pair for a key, so which value the graph gets would then
depend on where the new key landed relative to the prose.

**5.** Because no `def` or `class` can appear inside an expression. The one
expression-form definition is `lambda`, and `(lambda: "x").__doc__` is `None` —
a string in a lambda body is its return value, not a docstring. So no key can
live in a lambda, and ignoring expressions misses nothing that matters.

---

## Where the key goes

**6.** The **last** part: it holds the closing quotes. Measuring from the first
puts the offset before the second part's opener, so the insert lands in the gap
between two literals and yields either a syntax error or a third silently
concatenated string. `shapes.py:implicit_concat` is the fixture.

**7.** They differ on exactly one input: an **unclosed** string, where
`closer_len()` is zero because there is no closer. `quote_len()` would put the
offset three bytes inside the content — in a file that does not parse, which is
the file you least want to rewrite.

**8.** Because Python forbids a string literal from ending in an odd number of
backslashes: the last one would escape the closing quote. The input you would
fear is `r"""abc\"""`, and Python rejects it — *unterminated triple-quoted
string literal*. The legal form is `"""abc\\"""`, where the pair is an escaped
backslash and inserting after it changes nothing.
(`shapes.py:ends_with_backslash`, whose `__doc__` really does end in `\`.)

**9.** ` ID: u` on the same line, instead of `\n\n{indent}ID: u\n{indent}`. A
newline inside a single-quoted string is a syntax error. The reader does not
care: `(\S+)\s*:\s*(\S+)` finds a key mid-line perfectly well.

**10.** Because the reader builds a dict, so for a repeated key the **last**
pair wins. Append and your key wins; insert at the front and prose that happens
to contain `ID:` outranks you. `idempotent.py:prose_mentions_id` and
`docstrings.py:id_like_text` are the two fixtures, and the failure is a graph
node keyed on `'nothing-real,'`.

**11.** `"\n{indent}\"\"\"ID: u\"\"\"\n{indent}"` at the offset of `...`. The
leading newline ends the header line; the docstring takes the next line; the
trailing newline-plus-indent opens a line for the original statement, which is
still at that offset and so follows the docstring. Verified: `__doc__` reads
back and the function still works.

**12.** An **empty body** — `parse_body` returns `Suite::new()` on error
(`ruff_python_parser/src/parser/statement.rs:3117`), so `def f():` at EOF has no
statements — and an **unclosed docstring**. In both cases the def is skipped: no
edit, `id: null` this run, a key as soon as the file parses. That is quirk 13 —
swallow it, never panic, and never rewrite a file you could not read.

---

## Plan and apply

**13.** Because applying needs `&mut db` afterwards and planning holds
`&'a Stmt` borrowed from the parse. `Vec<Edit>` is owned (`TextSize` +
`String`), so it outlives the AST borrow. The two-phase split is forced by the
borrow checker, not chosen for neatness.

**14.** An insert shifts every byte after it, so going from the end backwards
leaves every not-yet-used offset valid. For free: **children are written before
parents**, because a def's slot is at the top of its body and therefore strictly
before any nested def's.

**15.** No. Every offset is inside a distinct def's body and after that def's own
header, so a parent's offset is strictly less than its child's and siblings live
in different bodies. The `debug_assert` is therefore not testing the sort — it
is testing **object 1**, since the only way to get two edits at one offset is for
`child_defs` to yield the same def twice.

**16.** Because the builder never mutates the source: it copies spans out of an
immutable `&str`, so no offset can go stale. Learn the rule anyway — any
in-place edit, in a test or a throwaway script, needs it, and it is what makes
the plan/apply split obviously correct.

**17.** Testing: a counter makes fixture output byte-identical run to run, so it
can be snapshotted and reviewed as a diff. Purity: it concentrates all the
nondeterminism in one closure, leaving `plan_ids` pure *given* `new_id`. Without
it, the diff for "what did injection do to `shapes.py`" is thirty-six random hex
digits of noise per def, and `insta` fails on its second run.

**18.** `line_prefix`, which slices the real bytes between `body[0]`'s line start
and `body[0]` and reuses them verbatim. The file supplies its own indentation;
there is nothing to detect. (Line endings likewise come from the surrounding
bytes for an in-literal insert — the only inserted newline you choose is in the
one-liner and new-statement shapes, which is where a CRLF file needs you to
write `\r\n`.)

---

## Purity and the order

**19.** In order of how badly each bites:

1. **Random output.** Salsa compares `old == new`; a random result is never
   equal, so everything downstream re-invalidates forever and one def can get
   two IDs.
2. **Cancellation.** `&mut db` unwinds in-flight queries; a half-finished
   `fs::write` loses the user's file.
3. **Wrong time.** Injection shifts ranges, so it must finish before the outline
   is computed; collection runs on every read.
4. **Wrong data.** The slot fields make every cached `DefNode` bigger and join
   `old == new`, so cosmetic edits start invalidating the outline.

**20.** It makes `values_equal` always false
(`salsa-macros/src/tracked_fn.rs:103-115`), so every dependent query re-runs on
every read, permanently — and the same def can be handed two different IDs on
two reads of one unchanged file, with the graph keeping whichever came last.

**21.** Taking `&mut db` cancels other threads' in-flight queries by throwing
`Cancelled::PendingWrite` through `panic::resume_unwind`
(`salsa/src/cancelled.rs`, `storage.rs:160` `cancel_others`). A pure query that
is unwound loses nothing and is re-run; a write that is unwound leaves a
truncated file, and no structuring makes a write safe against interruption at an
arbitrary instruction.

**22.** Parse (cached) → `plan_ids` → apply, write, `File::sync` →
`definitions`. The last step guarantees the outline handed to the graph is
computed from the **post-write** text. Care in step three could stop *you*
reusing a stale range; step four removes the code path on which one exists.

**23.** Every later query serves the pre-injection source: IDs stay `None`,
injection looks like a no-op, and nothing errors. It is hard to notice because
the one place you would look — the file on disk — shows the correct text.

**24.** `definitions` is `#[salsa::tracked(returns(ref))]`, so its result borrows
the db immutably; `File::sync` / `sync_path` take `&mut dyn Db`. The two borrows
cannot coexist. The loophole is cloning the outline first, which compiles and
whose ranges are stale — clone names if you must, never ranges.

**25.** Because `&mut db` cancels every in-flight query on every other thread.
One sync per file multiplies that by the file count, so a first pass over a
project becomes a storm of cancelled and re-run work. One request, one write
phase, one sync (`File::sync_all_recursive` for the many-paths case).

---

## Parity and the wire

**26.** (a) **Prefix loss** — `id_injector.py:70` emits `f'"""{content}"""'`
unconditionally, so an `r` prefix is dropped; triggered by any raw docstring
(`has_docstring.py:raw_prefixed`). (b) **`"""` in content** — the rebuilt
literal is broken; triggered by `has_docstring.py:contains_triple_quotes`. This
exercise does not reproduce them because it never rebuilds the literal: it
inserts bytes before the closer, and the prefix is at the other end. Reproducing
them would mean deliberately corrupting the user's source, and the parity rule
("match the output") was written about JSON, not about the files on disk.

**27.** The driver re-emits the docstring without its `r`, so `\n` and `\t` stop
being two characters each and become a real newline and a real tab. The string's
**value** changes — a path becomes a path with control characters in it, and a
regex stops matching. That is why it is not a formatting question: the program
means something different afterwards.

**28.** `has_docstring.py:raw_prefixed` and
`has_docstring.py:contains_triple_quotes`. For the other seven, keep the
byte-identical gate and generate the baseline from the **Python driver**; for
these two, hand-check and record the divergence in
`plan/04-build/02-milestones.md` so the gate's gap is deliberate rather than
discovered.

**29.** `File::sync_path` (`files.rs:456`), because it also invalidates the
containing **directory listing** — the file did not exist a moment ago, so the
db's view of the directory is wrong too. `File::sync` (`:478`) refreshes a known
file and is the right one after an overwrite.

**30.** Objects 2, 3 and 4 all apply unchanged: `doc_slot` takes a `&[Stmt]`, and
`module.syntax().body` is one; the edit, the ordering and the sync are
identical. Object 1 does not — a module is not a `Stmt`, so `Def` cannot hold it
and `child_defs` never yields it. It is the one def-like thing that needs its
own entry point.

**31.** **For:** v-noc never sees `id: null`, so no consumer needs to handle it,
and every answer is immediately joinable. **Against:** a read-shaped RPC becomes
an unconditional writer, so a hover or an outline request can rewrite the user's
file — and the `--write` guard that protects corpus runs now has to be off in
normal operation. What to look at first: **what v-noc does with `id: null`
today**. Exercise 02 object 6 says `null` is what *triggers* injection
downstream, so something reads that field and acts on it; change the timing and
that logic may fire never, or always.

---

## Practical

**32.** The shape of the answer: ten rows, second column all zeros, third
column all "imports". A non-zero second number means your writer produced
something your reader does not accept — start at `id_from_body`, not at the
injector, and check the single-quoted and implicit-concat shapes first, since
those are the two whose inserted text differs from the common case.

**33.** `has_id` → `ID` → a real uuid → no edit. `prose_mentions_id` → `ID` →
`'nothing-real,'` → **no edit**. `has_other_keys` → `Owner`, `Status`, no `ID` →
edit. `Mixed` → `ID` → a real uuid → no edit. `Mixed.needs_one` → nothing →
edit. Two edits.

The row to argue about is `prose_mentions_id`. It is not a bug in the injector —
it is the reader's rule applied to an input nobody intended. If you want it
edited, the change belongs in `_extract_metadata` (require the key at a line
start, say), it applies to **both** passes because they share the reader, and it
is a parity break with the Python driver that you would have to record.

**34.** The first run must produce a report and a zero-byte diff — `git status`
clean, or an `md5` of the tree unchanged. The second must refuse before opening
the file, and the refusal must come from inside the injector rather than the
CLI, so that a future caller cannot bypass it.

**35.** No fixed answer. A good one is small, has exactly one reason to fail, and
comes with its expected edit plan rather than its expected output — a plan diff
tells you which decision changed; a file diff only tells you the bytes moved.
