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

## Step 5 — The smoke test

Now write the Rust. Roughly twenty lines in `src/main.rs`. What it must do:

1. Take a directory path from `std::env::args()`.
2. Build an `OsSystem` for it.
3. Discover the project metadata.
4. Construct a `ProjectDatabase`.
5. Resolve one Python file inside that project to a `File`.
6. Print the Python version ty resolved for it.

### The API surface, verified at `ac201b8`

```rust
// ruff_db
pub fn system_path_to_file(db: &dyn Db, path: impl AsRef<SystemPath>) -> Result<File, FileError>;
impl OsSystem { pub fn new(cwd: impl AsRef<SystemPath>) -> Self; }
// SystemPathBuf::from("/abs/path")

// ty_project
impl ProjectMetadata {
    pub fn discover(path: &SystemPath, system: &dyn System) -> Result<ProjectMetadata, ProjectMetadataError>;
    pub fn discover_without_uv(path: &SystemPath, system: &dyn System) -> Result<..>;
}
impl ProjectDatabase {
    pub fn use_defaults<S: System + 'static + Send + Sync + RefUnwindSafe>(m: ProjectMetadata, s: S) -> Self;
    pub fn fallible<S: ...>(m: ProjectMetadata, s: S) -> anyhow::Result<Self>;
}

// ty_python_semantic::Db  — a TRAIT method. you must import the trait to call it.
fn program_file(&self, file: File) -> ProgramFile<'_>;

// ty_python_core::ProgramFile
pub fn python_version(self, db: &'db dyn Db) -> PythonVersion;
pub fn python_file(self, db: &'db dyn Db) -> PythonFile<'db>;
pub fn file(self, db: &'db dyn Db) -> File;
```

Use `use_defaults`, not `fallible`. A user's broken `pyproject.toml` must never
take your analyser down — that matches the swallow-everything posture your
Python driver already has (`plan/00-orientation/01`, quirk 13).

### ⚠ The plan's smoke test does not compile

`plan/04-build/01-wiring-cargo.md` ends with a snippet containing:

```rust
use ty_python_core::Program;
println!("python_version = {}", Program::get(&db).python_version(&db));
```

**There is no `Program::get` at `ac201b8`.** **[verified]** — `Program` is a
`#[salsa::interned]` struct in `ty_python_core/src/program.rs` whose only public
constructor is `from_settings`, and it is *obtained*, not fetched globally. The
route that exists:

```
File  --db.program_file(file)-->  ProgramFile  --.python_version(db)-->  PythonVersion
```

So you need a file before you can ask about a version. Pick any `.py` under the
project root — you are asking "what version applies *here*", which is the
honest question anyway, since ty resolves per-file.

**Finding this yourself is the actual lesson of step 5.** The plan was written
by reading the source at one moment; the source is the authority, always. When a
snippet does not compile, do not guess and do not trust the doc — look:

```bash
cargo doc -p ty_python_core --no-deps --open
```

Then search the generated page for `Program`. Ten seconds, and it describes the
exact code you linked against. Build this reflex now; you will need it
constantly from exercise 06 onward, where the plan itself marks several API
names `[check]` because the author was unsure.

### Two compiler errors you will hit, and what they mean

```
error[E0599]: no method named `program_file` found for struct `ProjectDatabase`
```
The method is on the `ty_python_semantic::Db` trait, not on the struct. Add
`use ty_python_semantic::Db as _;` — the `as _` imports the trait for method
resolution without binding a name you would then have to disambiguate against
the four other `Db` traits in scope.

```
error[E0277]: the trait bound `ProjectDatabase: ty_python_semantic::Db` ...
   expected `&dyn Db`, found `&ProjectDatabase`
```
Some functions take `&dyn Db`. Coerce explicitly: `&db as &dyn Db`, or pass
`&db` where the parameter type already forces the unsizing. This is the most
common early friction with ty's API and it stops being confusing after about
three occurrences.

---

## Step 6 — Run it against three different projects

```bash
cargo run -- /path/to/a/bare/directory          # no config at all
cargo run -- /path/to/a/project/with/.venv      # environment-derived
cargo run -- /path/to/a/project/with/pyproject.toml   # config-derived
```

**Predict each answer before you run it.** Write the three predictions down.

You will be wrong about at least one — that is exercise 04's entire subject, and
seeing the surprise now makes that exercise land harder.

---

## Folder structure — set it up now, not later

You will regret a single `main.rs`. The layout the plan asks for
(`plan/04-build/00-dev-cli.md`), adopted from day one:

```
pylspt/
├── Cargo.toml
├── rust-toolchain.toml
├── src/
│   ├── lib.rs              ← ALL analysis lives here. every exercise adds a module.
│   ├── db.rs               ← project discovery, database construction  (ex 00, 03)
│   ├── position.rs         ← LineIndex, offset ↔ line/column           (ex 01)
│   ├── nodes.rs            ← the node tree + wire types                (ex 02)
│   ├── modules.rs          ← is-this-project-code, qualified names     (ex 05, 06)
│   ├── types.rs            ← inference helpers                         (ex 07)
│   ├── mro.rs              ← base classes                              (ex 08)
│   ├── inject.rs           ← libcst ID injection                       (ex 10)
│   └── bin/
│       └── pylspt-dev.rs   ← the CLI. thin. argument parsing only.     (ex 11)
└── experience/             ← this folder
```

```toml
# Cargo.toml — add these once you create src/lib.rs
[lib]
name = "pylspt"
path = "src/lib.rs"

[[bin]]
name = "pylspt-dev"
path = "src/bin/pylspt-dev.rs"
```

**The one rule** (`plan/04-build/00-dev-cli.md`): the binary must call the same
functions the eventual server calls. The binary parses arguments and prints
JSON. It contains no analysis. If the CLI and the server can ever disagree, your
fixtures stop proving anything about what ships.

For this exercise, `src/main.rs` is fine — but do the `lib.rs` split at the
start of exercise 01, while it costs nothing.

---

## Done when

- [ ] `cargo check` is clean
- [ ] `cargo run -- <dir>` prints a Python version for three different projects
- [ ] you can state, for each, where that version came from
- [ ] `cargo tree -d` shows no duplicate `ruff_*`/`ty_*` crates
- [ ] you wrote your three predictions down before running

---

→ [`exam.md`](exam.md), then [`../01-source-and-positions/README.md`](../01-source-and-positions/README.md)
