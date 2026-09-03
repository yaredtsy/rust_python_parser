# Answers 10 — Lossless edits and ID injection

---

**1.** Because the AST is **lossy**: comments, blank lines, quote style,
spacing and line endings are not represented. Printing it back gives you a
syntactically equivalent file, not the user's file. You would be reformatting
every file you touched — an unacceptable side effect for a tool that is supposed
to add one docstring.

A CST is lossless by construction: every token and every piece of trivia is a
node, so an unmodified subtree prints back byte-identically.

**2.** `ruff_linter/src/fix/codemods.rs` uses `libcst_native` **[verified]** for
autofixes that must preserve surrounding formatting — the same requirement you
have. Reassuring because it means this is the established answer inside the
codebase you are building on, not an exotic workaround: ruff's own authors, with
full access to their AST, still reach for a CST when they need to rewrite.

**3.** Detect with the ruff AST you already have (does every def/class have an
`ID:`?); only if something is missing do you parse with libcst, inject, codegen,
write and sync.

On a warm project, close to **0%** reach libcst — every file already has its
IDs. Injection is a first-visit and post-edit cost, not a per-request one.

**4.** Because you already walked the AST to build the node tree, and the `ID:`
lookup is the same one that fills in `BaseNode.id`. Detection adds no traversal
and no parse — it reads a value you were computing anyway.

**5.** The default features build the **PyO3 Python extension module**. You want
the Rust library only; the extension pulls in Python linkage you have no use
for. Ruff sets it false for the same reason **[verified, `Cargo.toml:132`]**.

---

**6.**

```
1. compute new content   (libcst)
2. write to disk
3. File::sync_path(&mut db, &path)
```

Omit 3 and salsa never learns the file changed, so `source_text` and
`parsed_module` keep serving the pre-injection content. Symptom: you inject
successfully, the file on disk is visibly correct, and your very next
`parse_file` still reports `id: null` for every node. Nothing errors. A test
that opens a fresh database passes.

**7.** Because `&mut db` **cancels every in-flight query on every other thread**.
Syncing per file during a request that touches thirty files means thirty
cancellation storms, and any concurrent request gets cancelled repeatedly.

Collect the injections, write them, sync once.

**8.** It must **write and then query on the same database instance**:

```
1. build db
2. parse_file(path)          → assert ids are None
3. inject + write + sync
4. parse_file(path) on THE SAME db   → assert ids are Some
```

A naive test builds a fresh db in step 4, which reads the file from disk and
passes whether or not the sync happened. The whole point is to keep the stale
cache alive across the write.

---

**9.** They apply **only to nodes you create**. Existing nodes carry their own
whitespace and print back exactly as they were.

Since the docstring statement is brand new, these two settings decide entirely
how it looks — and they are the only formatting decision you make.

**10.** From `ruff_python_codegen::Stylist`, built from the file's own tokens:
`Stylist::from_tokens(tokens, source)`, then `.line_ending()` and
`.indentation()` **[verified]**.

With constants: `crlf.py` gets a single LF line inserted into a CRLF file — git
shows a mixed-endings diff and the user's formatter or pre-commit hook fails on
their next commit. `tabs.py` gets four spaces in a tab-indented file, which in
Python is not only ugly but can be a `TabError`.

**11.** Because `def stub(): ...` has a **simple statement suite** on the same
line as the header — there is no indented block to insert into, and no newline
after the colon to insert after. Text-based insertion produces
`def stub():\n    """ID: …"""...`, which is a syntax error.

libcst represents the two forms as different suite kinds, so you can detect the
one-line case and convert it to an indented block (or handle it deliberately),
rather than discovering it as a crash on someone's real file.

---

**12.**

| bug | trigger | wrong output | why reproduce |
|---|---|---|---|
| prefix loss | any `r"""…"""` docstring | the `r` is dropped: `id_injector.py:70` emits `f'"""{content}"""'` unconditionally | byte-identical output is the gate; "fixing" it makes every diff on such a file a false positive |
| `"""` in content | a docstring containing `"""` | the rebuilt literal is malformed | same |

The general reasoning (`MEMORY.md`): the contract is the observable output. A
port that changes behaviour cannot be verified against the thing it replaces,
and you lose the ability to tell "I broke it" from "I improved it".

**13.** In a raw string, `\n` is backslash-plus-`n` — two characters. Without the
`r`, it becomes a newline, and `\t` becomes a tab. So `C:\new\table` turns into
`C:` + newline + `ew` + tab + `able`.

Real, not cosmetic: docstrings holding Windows paths or regexes (`\d+`,
`\s*`) are exactly why people write raw docstrings. The tool silently corrupts
the documentation of the code it is indexing — and the corruption is invisible
in a rendered diff unless you look at the bytes.

That is worth writing in the `// PARITY:` comment, so whoever reads it knows
this is a known defect being deliberately preserved rather than an oversight.

**14.** No — creating a file as a side effect of a *read* ("give me the folder
ID") is surprising, and it means calling the analyser on a read-only checkout
can fail.

Does it matter? **No, not for the port.** It is load-bearing for v-noc's folder
identity, so removing it breaks the consumer. It matters only insofar as you
should preserve it *knowingly* and document it, so nobody later "cleans it up".

**15.** Downstream gets a `FolderSchema/<uuid>` that exists nowhere and matches
nothing — a join key with no partner. If v-noc caches it, you get a phantom
folder that changes identity on every call.

You reproduce it because it is observable behaviour and the client may already
depend on receiving *something* rather than an error. Note it in the divergence
log as a candidate fix once parity is established — this is a good example of a
quirk worth flagging upward even though it is not yours to change today.

---

**16.** Expected: run one makes edits to all six fixtures and invokes libcst six
times; run two makes zero edits and invokes libcst **zero** times. Removing one
ID by hand should produce exactly one edit and exactly one invocation on the next
run.

If run two invokes libcst even once, your detection is not equivalent to your
injection's notion of "has an ID" — usually a docstring form the detector reads
but the injector rewrites anyway (`single_quoted` and `implicit_concat` are the
usual suspects).

**17.** Something like:

```rust
/// ⚠ SIDE EFFECT: this writes to the user's source files. If any def or class
/// lacks an `ID:` in its docstring, the file is rewritten on disk before the
/// nodes are returned (matching scanner.py:24). The IDs are the join key into
/// the rest of v-noc, so this is load-bearing, not incidental.
```

On the RPC guard: **yes, it belongs there too**, but as a *configuration*
rather than a default-off. The CLI defaults to dry-run because it is pointed at
corpora by developers; the RPC must inject, because v-noc depends on it. What
the RPC should have is an explicit `inject: bool` in its settings, defaulting to
true, so that a read-only or sandboxed deployment can turn it off without
patching code. Making the side effect *nameable* is the point — a behaviour
nobody can switch off is one nobody can reason about.
