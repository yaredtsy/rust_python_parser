# Answers 00 — Setup

---

**1.** `ty_project`, `ty_ide` and `ty` are declared `publish = false` with
`version = "0.0.0"` **[verified]**. They have never been uploaded to crates.io
and by design never will be — they are internal crates of the ty binary. A git
dependency is the only supported way to consume them from outside the workspace.

**2.** Two reasons.

*One copy.* If `ruff_python_ast` came from crates.io (0.0.11) and
`ty_python_semantic` came from git, cargo would treat them as different sources
and could build **two** `ruff_python_ast` crates. `ast::ExprCall` from one is
not `ast::ExprCall` from the other; you would get type errors between values
that print identically.

*One truth.* One revision means `cargo doc` describes exactly the code you
linked. Mix sources and the docs you read describe a program you are not
running.

**3.** Two copies of the entire dependency subtree. Because `ty_ide::Db`
requires `ty_python_semantic::Db`, and `ty_python_semantic` at rev A is a
different crate from rev B, you get errors of the form:

```
error[E0277]: the trait bound `ProjectDatabase: ty_ide::Db` is not satisfied
note: `ProjectDatabase` implements `ty_python_semantic::Db`
      but `ty_ide` requires `ty_python_semantic::Db`
      perhaps two different versions of crate `ty_python_semantic` are being used?
```

The tell is that last line, and the fact that the two type names in the error
are **character-for-character identical**. Any time an error says "expected `X`,
found `X`", suspect duplicate crate versions and run `cargo tree -d`.

**4.** Transitively. `ty_project` depends on `salsa`, and every salsa type you
touch (`ProjectDatabase`, `File`, `ProgramFile`) is re-exported or returned
through ty's public API. You only need salsa in *your* manifest when you write
`#[salsa::tracked]` yourself — that is when the macro has to resolve `::salsa`
by name in your crate.

**5.** Salsa cancels in-flight queries by **unwinding** — it panics on purpose,
and the runtime catches it. With `panic = "abort"` the process dies instead. The
symptom is a process that vanishes under concurrent edits, with no error, on
inputs that work perfectly when tested one at a time. Nothing about the crash
points at a build profile setting, which is what makes it expensive.

---

**6.** `use ruff_db::system::OsSystem;` fails with `E0432: unresolved import`.
`ruff_db/src/system.rs:7` is `#[cfg(feature = "os")] pub use os::OsSystem;`
**[verified]**, and `ruff_db` declares no `default` feature set, so the `os`
module simply is not compiled. The confusing part is that the crate builds
fine — nothing is *broken*, the type just does not exist in your build.

**7.** `cache` gates `ruff_cache` integration; `junit` gates JUnit-XML
diagnostic output. Neither is on any path you use — you never emit ty's
diagnostics. It would bite you if you later wanted to render ty's own error
messages, at which point you add the feature and rebuild.

The general lesson matters more than this instance: **cargo features are
additive and unified across the graph, so under a workspace you often get
features for free from a sibling crate. As the root crate, nobody is enabling
them on your behalf.** That single sentence explains most "it works in their
repo, not in mine" dependency problems.

**8.** `default-features = false` turns off `zstd`, which controls how
`ty_vendored` compresses the bundled typeshed. `ty_vendored/build.rs` **[verified]**
falls back to `CompressionMethod::Deflated` if `deflate` is on, and
`CompressionMethod::Stored` if neither is — the zip is written and read by the
same build, so the stubs still resolve either way.

Neither choice is wrong. Defaults-on gives a smaller artefact; defaults-off
gives a build with no zstd C dependency, which is why ruff's WASM target wants
it. The plan's line was copied from ruff's workspace, where the `ty` binary
re-enables zstd anyway. **Leave the default on and stop thinking about it.**

---

**9.** The broken line is:

```rust
Program::get(&db).python_version(&db)
```

There is no `Program::get` at `ac201b8` **[verified]** — `Program` is a
`#[salsa::interned]` struct whose public constructor is `from_settings`. The
route that exists goes through a file:

```rust
use ty_python_semantic::Db as _;
let file = system_path_to_file(&db, some_py_path)?;
let version = db.program_file(file).python_version(&db);
```

There is a per-file `python_version_with_source(file)` on the project database
too, which additionally tells you *where* the version came from — worth finding,
because exercise 04 wants exactly that.

**10.** That the plan is a **map, not a compiler**. It was verified by reading,
by a reader who could misread; and `ac201b8` is a snapshot of a repository that
refactors aggressively. Treat every API name in the plan as a search query, not
as a fact:

```bash
cargo doc -p ty_python_core --no-deps --open
```

Ten seconds, and it is authoritative — generated from the exact revision you
depend on. Do that reflexively for the rest of the exercises, especially in 08,
where the plan itself marks the MRO accessor names `[check]` because the author
was not sure.

---

**11.** **3.14.** With no configuration and no environment, ty falls back to
`PythonVersion::latest_ty()`, which is `PY314` at this revision **[verified]**.

If you predicted "whatever Python is installed" — that is the Jedi model
(`InterpreterEnvironment()` reports the *running* interpreter). ty does not look
at your installed Python unless it finds an environment to resolve.

**12.** **3.9.** `requires-python = ">=3.9"` contributes its **lower bound**.
ty is a type checker: "I promise to work on 3.9 and up" means it must check you
against 3.9, because that is the oldest thing you claim to support.

**13.** Both are correct, for different jobs.

For a **type checker**, 3.9 is right: the user declared 3.9 compatibility, so
3.10+ syntax in their code is a real error worth reporting.

For **your driver**, 3.9 is wrong. You are a structural analyser. Your job is to
see the code that is there. A 3.12-only `type X = int` in a project declaring
`>=3.9` becomes an `UnsupportedSyntaxError`, and the function containing it
degrades or vanishes from your node tree — which shows up downstream as
"v-noc stopped seeing that file" with no visible cause.

This is why `plan/01-crates/03` recommends a **version floor** for parsing:
take `max(resolved_version, PY312)` so you always get PEP 701 f-string
tokenisation, and discard the `unsupported_syntax_errors` you do not care about.
Exercise 04 makes you implement and test that decision.

---

**14.** Empty output is the goal: it means exactly one copy of every crate, so
every `Db` trait in your graph is *the* `Db` trait. `cargo tree -d` costs a
second and is the first thing to run whenever an error names the same type
twice.

**15.** You should have seen something close to the shape in answer 3 — a trait
bound that looks satisfied, plus cargo's "perhaps two different versions" note.
Some variants of this error do **not** include that note, and those are the
genuinely hard ones. The recognition you just bought: *identical type names on
both sides of an error means duplicate crates until proven otherwise.*
