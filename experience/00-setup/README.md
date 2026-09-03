# 00 — Setup: a crate that compiles against ty

**Goal:** `cargo run` prints the Python version ty resolved for a real project.

That is the whole exercise. It sounds trivial. It is not — the crates you are
linking against are unpublished internal crates of a fast-moving compiler, half
the API you will read about does not exist at your revision, and one missing
feature flag removes the type you need to open a file.

Getting this right now means every error message you see from exercise 01 onward
is about *your* code.

**No Python files in this exercise.** Nothing to analyse yet.

---

## How this exercise is organised

**Part 1 — this file.** Toolchain, `Cargo.toml`, the build. Mechanical. You have
probably already done it.

**Part 2 — nine separate files.** One per object you have to understand, in the
order you meet them. Each teaches one thing, shows worked examples, gives you an
exercise, and has its own small exam. The index is at
[Part 2](#part-2--the-objects-this-is-the-rest-of-the-exercise) below.

If you are lost, you are probably in Part 2 territory and should go read the
object files rather than pushing on from here.

---

## Getting started — the commands you run

Everything here is plain cargo. No submodules, no vendoring, no cloning ruff.
Copy-paste, in order:

```bash
# 1. Rust toolchain. Your current default is 1.94.1; ruff/ty need 1.96+.
#    Ruff itself pins 1.98.0, so match it.
rustup toolchain install 1.98.0

# 2. In the pylspt directory, pin it so cargo picks it automatically:
cd /Users/yared/Documents/Programing/rust/pylspt
printf '[toolchain]\nchannel = "1.98.0"\n' > rust-toolchain.toml

# 3. Confirm cargo now uses it (must print 1.98.0):
cargo --version

# 4. Edit Cargo.toml — the dependency block is in step 2 below. Then:
cargo check          # first run: downloads + builds. 5-10 min. walk away.
```

That is the entire setup. Steps 1–3 are yours to run; the rest of this file
explains *why* each dependency line is what it is, which is the part worth
knowing.

> **Looking things up without cloning ruff.** You will constantly want to check
> whether a function exists at your pinned revision. Two ways, neither needing a
> checkout:
>
> ```bash
> cargo doc -p ty_python_semantic --no-deps --open   # docs for exactly what you linked
> ```
>
> and `docs.rs/ruff_python_ast`, `docs.rs/ruff_source_file` for the published
> `ruff_*` crates. The `ty_*` crates are not on docs.rs — `cargo doc` is how you
> read those. **`cargo doc` is the authority**, because it is generated from the
> revision you actually depend on.

---

## Read first

- `plan/04-build/01-wiring-cargo.md` — the wiring chapter. **Read it knowing
  that you are taking its Option B, not Option A**, and that some of its
  specifics change as a result. This exercise tells you exactly which.
- `plan/01-crates/04-public-vs-private-api.md` — the consequence of Option B.
- `tutorial/11-reading-the-source.md` §"Building and checking".

---

## The decision, stated plainly

The plan recommends **Option A**: vendor Ruff as a submodule, make your crate a
workspace member, gain access to `pub(crate)` internals.

**You are doing Option B**: git dependencies, pinned revision, public API only.

| | Option A (plan's pick) | **Option B (yours)** |
|---|---|---|
| Setup | submodule + workspace surgery | ten lines of TOML |
| Cold build | full workspace, 5–10 min | your dep subtree, 4–8 min |
| Rebuild after `git pull` on ruff | rebase your patch | change one string |
| `pub(crate)` access | yes | **no** |
| Disk | a second copy of ruff | cargo's git cache |

For learning, B is strictly better: no fork to maintain, no rebase, and the
public API is more than enough for all twelve exercises. The moment where B
actually costs you something is exercise 08 (attribute lookup on a receiver *you*
choose), and when you get there you will understand the trade-off from evidence
instead of from a table. That is the right time to revisit it.

> **This is a real decision, not a shortcut.** `plan/01-crates/04` says "do not
> start with B" — and it is right *about building the interpreter*. It is not
> talking about learning the API. Note the difference and move on.

---

## Step 1 — Pin the toolchain

Create `rust-toolchain.toml` next to your `Cargo.toml`:

```toml
[toolchain]
channel = "1.98.0"
```

**[verified]** — that is exactly what `ruff/rust-toolchain.toml` pins at
`ac201b8`. ty uses recent language features; on an older toolchain you get
compile errors inside *their* code, which is a demoralising place to start
debugging.

Rustup will download the toolchain the first time you run cargo in this
directory. Let it. If you would rather do it up front:

```bash
rustup toolchain install 1.98.0
```

⚠ **This step is not optional on this machine.** Your default toolchain today is
**1.94.1** **[verified]**, and every ruff/ty crate declares `rust-version = "1.96"`.
Without the pin, `cargo check` fails during *resolution* — before compiling a
single line — with a wall of:

```
error: rustc 1.94.1 is not supported by the following packages:
  ruff_db@0.0.11 requires rustc 1.96
  ty_python_semantic@0.0.11 requires rustc 1.96
  ...  (about 25 more)
Either upgrade rustc or select compatible dependency versions with
`cargo update <name>@<current-ver> --precise <compatible-ver>`
```

Ignore the suggestion in that last line. There is no older version of these
crates that does what you need; the fix is the toolchain, not the dependencies.

---

## Step 2 — The dependencies

Replace your empty `[dependencies]` with this. Every line is deliberate; the
notes below explain the four that are not obvious.

```toml
[package]
name = "pylspt"
version = "0.1.0"
edition = "2024"

[dependencies]
# --- ruff/ty, all from ONE pinned revision ---
ruff_db            = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8", features = ["os"] }
ruff_python_ast    = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ruff_python_parser = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ruff_python_stdlib = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ruff_source_file   = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ruff_text_size     = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_ide             = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_module_resolver = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_project         = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_python_core     = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_python_semantic = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }

# --- yours ---
anyhow     = "1"
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
```

### The four non-obvious lines

**1. `ruff_db` needs `features = ["os"]`.**

`OsSystem` — the type that lets ty read your actual filesystem — is behind that
feature. **[verified]** `ruff_db/src/system.rs:7` is `#[cfg(feature = "os")]`,
and `ruff_db` declares no `default` feature set, so without this line you get:

```
error[E0432]: unresolved import `ruff_db::system::OsSystem`
```

ty's own CLI does exactly this: `ruff_db = { workspace = true, features = ["os", "cache", "junit"] }`
**[verified, `crates/ty/Cargo.toml:21`]**. You need `os`; the other two are for
its diagnostics output, not for you.

This is the single most likely thing to stop you, and the plan does not mention
it — because under Option A some other workspace member had already turned the
feature on, and cargo's feature unification handed it to you for free. Feature
unification does not cross the workspace boundary the same way when *you* are the
top-level crate. **Own your features now that you are the root.**

**2. Do *not* copy the plan's `default-features = false` on `ty_project`.**

`ty_project`'s default is `["zstd"]` **[verified]**, which controls how the
bundled typeshed zip is compressed. Turning it off is not fatal — `ty_vendored`'s
`build.rs` falls back to `CompressionMethod::Stored` **[verified,
`ty_vendored/build.rs:50`]**, so the stubs still load, just from a bigger
artefact. Leave the default on. The plan's `default-features = false` was copied
from ruff's own workspace, where a sibling crate re-enables it anyway.

**3. `salsa` is not in your `Cargo.toml`.**

The plan tells you to pin `salsa = "0.28.2"` with an exact feature list. That
advice applies when you write your own `#[salsa::tracked]` queries. You do not,
in any of these exercises. Every salsa type you touch arrives through ty's
public API.

Leaving it out removes the nastiest failure mode in this whole setup: two
different salsa versions in one graph, producing `expected Db, found Db` — an
error where both types print identically. When you eventually do add it, take
the version *and the exact feature list* from ruff's workspace manifest, and
verify with `cargo tree -d -p salsa` (`-d` = show duplicates; the correct answer
is "no duplicates found").

**4. Every line has the same `rev`.**

Mixed revisions means cargo builds two copies of `ruff_db`, and
`ty_python_semantic@rev1::Db` is a genuinely different trait from
`ty_python_semantic@rev2::Db`. Same name, same printout, incompatible. When you
bump the pin, bump every line at once — a `sed` over the manifest, not a manual
edit.

---

## Step 3 — Build profiles

Add to `Cargo.toml`:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
```

Your code stays fast to compile and debuggable; the dependency wall (which you
will never step through) gets optimised once and cached forever. Without this,
ty is slow enough at `-O0` that your test loop becomes unpleasant.

> ⚠ **Never set `panic = "abort"`.** Salsa uses unwinding for query cancellation
> and cycle recovery — a routine cancellation becomes a process kill. The
> symptom is random crashes under concurrent edits, and it is miserable to
> diagnose. See `plan/01-crates/02-the-salsa-db.md`.

---

## Step 4 — First build

```bash
cargo fetch          # clones ruff — a big repo, be patient the first time
cargo check          # then wait
```

The clone alone is a few hundred MB of git history into `~/.cargo/git/db/`, and
on a slow link it can take longer than the compile. It happens **once** — every
later project pinning the same repo reuses that cache.

Expect **4–8 minutes cold** for the compile after that. Afterwards, changes to
your own code are 5–20 seconds with `cargo check`.

Two things that make this less painful:

```bash
# If the initial clone stalls or is very slow, use your system git (better at
# large repos, and picks up your proxy/SSH config):
export CARGO_NET_GIT_FETCH_WITH_CLI=true
```

```bash
# Watch what it is doing rather than staring at a spinner:
cargo check --timings     # writes target/cargo-timings/*.html — open it
```

That timings HTML is worth one look. It shows you the shape of what you just
linked against: `ty_python_semantic` and `ty_vendored` dominate, and you will
recognise the crate names from `plan/01-crates/01-crate-map.md`.

**Use `cargo check`, not `cargo build`, as your inner loop** for the whole of
this folder. Only exercises that actually run something need a binary.

---

## Part 2 — the objects (this is the rest of the exercise)

Steps 1–4 got you a crate that compiles. Everything from here is about the
**seven objects** you need in order to open one Python file and ask ty a
question about it.

One file per object. Each file is the same shape:

```
what it is  →  why it exists  →  what you can do with it  →  examples
            →  exercise  →  its own exam  →  answers at the bottom
```

Read them in order. Each one uses the one before it, and each one is small
enough to finish in a sitting. **Do not skip to file 08** — it assumes all seven.

| | file | object | one-line summary |
|---|---|---|---|
| 1 | [`01-object-systempath.md`](01-object-systempath.md) | `SystemPath`, `SystemPathBuf` | a path, guaranteed to be UTF-8 |
| 2 | [`02-object-system.md`](02-object-system.md) | `System`, `OsSystem` | the filesystem, abstracted |
| 3 | [`03-object-projectmetadata.md`](03-object-projectmetadata.md) | `ProjectMetadata` | where the project is + its config |
| 4 | [`04-object-projectdatabase.md`](04-object-projectdatabase.md) | `ProjectDatabase` | ★ the database. ty's memory |
| 5 | [`05-object-file.md`](05-object-file.md) | `File` | a handle for one path |
| 6 | [`06-object-programfile.md`](06-object-programfile.md) | `ProgramFile` | that file + its program context |
| 7 | [`07-object-pythonversion.md`](07-object-pythonversion.md) | `PythonVersion` | two integers, one booby trap |
| 8 | [`08-putting-it-together.md`](08-putting-it-together.md) | — | the whole smoke test, line by line |
| 9 | [`09-project-layout.md`](09-project-layout.md) | — | lib/bin split. do this before exercise 01 |

Then [`exam.md`](exam.md), which covers the whole exercise including the
manifest and toolchain parts above.

---

## How they fit together

By the end of file 08 you will have typed this, and understood every line:

```rust
let system   = OsSystem::new(&dir);                          // object 2
let metadata = ProjectMetadata::discover(path, &system)?;    // object 3
let db       = ProjectDatabase::use_defaults(metadata, system);  // object 4
let file     = system_path_to_file(&db, path)?;              // object 5
let pf       = db.program_file(file);                        // object 6
println!("{}", pf.python_version(&db));                      // object 7
```

Six lines. Seven objects. That is the entire deliverable of exercise 00 — and
every later exercise starts from exactly these lines.

---

## Two things to know before you start

**1. You will meet the same error three times.**

```
error[E0599]: no method named `X` found for struct `Y`
```

…when `X` is right there in the documentation. It always means the same thing: a
**trait is not imported**. You will hit it with `System` (object 2), with `Db`
(object 4), and later with `Ranged` and `HasType`. After the third time it
becomes instant recognition. Object 8 has the full error catalogue.

**2. The plan's smoke test does not compile.**

`plan/04-build/01-wiring-cargo.md` ends with:

```rust
use ty_python_core::Program;
println!("python_version = {}", Program::get(&db).python_version(&db));
```

**There is no `Program::get` at `ac201b8`** **[verified]**. `Program` is a
salsa-interned struct obtained *from a file*, not a global you fetch. Object 7
gives the route that works.

Mentioned here so you do not waste an afternoon on it — and as a warning about
the plan generally. It was written by reading the source once; the source is the
authority. When something does not compile:

```bash
cargo doc -p ty_python_core --no-deps --open
```

Ten seconds, and it describes the exact revision you linked against. Build that
reflex now; from exercise 06 onward the plan marks several API names `[check]`
precisely because the author was unsure.

---

## Done when

- [ ] `cargo check` is clean
- [ ] `cargo tree -d` shows no duplicate `ruff_*`/`ty_*` crates
- [ ] you have worked through all seven object files and their exams
- [ ] the smoke test from file 08 prints a version for six different projects
- [ ] you predicted each version before running, and know which one you got wrong
- [ ] the crate is split into `lib.rs` + `src/bin/` per file 09
- [ ] `cargo test` runs one real test

---

→ Start: [`01-object-systempath.md`](01-object-systempath.md)
