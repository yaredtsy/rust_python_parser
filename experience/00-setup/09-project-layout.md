# 09 — How to lay out the crate

Ten minutes now, or a 1500-line `main.rs` in three weeks. Your choice.

---

## The problem

Right now everything is in `src/main.rs`. That is correct for a smoke test and
wrong for anything else, for one specific reason:

**A binary crate cannot be tested from outside, and cannot be reused.**

`cargo test` can test code inside `main.rs` (with `#[cfg(test)] mod tests`), but
nothing else can *call* it — not another binary, not an integration test in
`tests/`, not the JSON-RPC server you will write in exercise 11's follow-up.

---

## The shape

```
pylspt/
├── Cargo.toml
├── rust-toolchain.toml
├── src/
│   ├── lib.rs              ← ALL analysis lives here
│   ├── db.rs               ← project discovery, database, open()   (ex 00, 03, 04)
│   ├── position.rs         ← offsets ↔ line/column                 (ex 01)
│   ├── nodes.rs            ← the node tree + wire types            (ex 02)
│   ├── modules.rs          ← is-project-code, qualified names      (ex 05, 06)
│   ├── types.rs            ← inference helpers                     (ex 07)
│   ├── mro.rs              ← base classes                         (ex 08)
│   ├── inject.rs           ← libcst ID injection                   (ex 10)
│   └── bin/
│       └── pylspt-dev.rs   ← the CLI: arguments and printing ONLY  (ex 11)
├── tests/                  ← integration tests, later
└── experience/             ← this folder
```

And in `Cargo.toml`:

```toml
[lib]
name = "pylspt"
path = "src/lib.rs"

[[bin]]
name = "pylspt-dev"
path = "src/bin/pylspt-dev.rs"
```

**Rust note.** A package can have one library and many binaries. Binaries in
`src/bin/*.rs` are found automatically, but declaring them explicitly makes the
intent obvious. Each binary depends on your library by name — inside
`pylspt-dev.rs` you write `use pylspt::db::open_project;`, exactly as an
external user would.

---

## `lib.rs` — the module list

```rust
pub mod db;
pub mod position;
pub mod nodes;
// …one line per file, added as you write it
```

**Rust note — modules are not automatic.** Creating `src/position.rs` does not
make it part of your crate. You must declare `pub mod position;` in `lib.rs`, or
the file is dead weight the compiler never reads. If you create a file and
nothing you write in it seems to exist, this is why.

`pub` makes the module visible outside the crate — which your binaries count as.
Without it, `pylspt-dev.rs` cannot see anything.

---

## The one rule

From `plan/04-build/00-dev-cli.md`:

> **The CLI must call the exact same functions the RPC layer calls.**
>
> Not a reimplementation, not a "simplified path". If the CLI and the server can
> disagree, your fixtures stop proving anything about what ships.

So:

```
src/lib.rs and its modules   →  analysis. no printing, no argument parsing,
                                no process::exit, no eprintln for control flow.

src/bin/*.rs                 →  read arguments, call the library, print.
                                that is all.
```

A useful test of whether you have it right: **could you delete every binary and
still have a working library?** If the answer is no, logic has leaked into the
CLI.

---

## Where the smoke test goes

Your working program from file 08 splits like this.

**`src/db.rs`:**

```rust
use ruff_db::files::{File, system_path_to_file};
use ruff_db::system::{OsSystem, SystemPath, SystemPathBuf};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::ProgramFile;
use ty_python_semantic::Db as _;

/// Build a database for the project containing `dir`.
pub fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase> {
    let system = OsSystem::new(dir);
    let metadata = match ProjectMetadata::discover(dir, &system) {
        Ok(m) => m,
        Err(err) => {
            // Broken config must never take the analyser down (quirk 13).
            tracing::warn!("project discovery failed: {err}; using defaults");
            ProjectMetadata::new("adhoc", SystemPathBuf::from(dir.as_str()))
        }
    };
    Ok(ProjectDatabase::use_defaults(metadata, system))
}

/// Open `path`, returning both handles.
pub fn open<'db>(
    db: &'db ProjectDatabase,
    path: &SystemPath,
) -> anyhow::Result<(File, ProgramFile<'db>)> {
    let file = system_path_to_file(db, path)?;
    Ok((file, db.program_file(file)))
}
```

**`src/lib.rs`:**

```rust
pub mod db;
```

**`src/bin/pylspt-dev.rs`:**

```rust
use pylspt::db::{open, open_project};
use ruff_db::system::SystemPath;
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: pylspt-dev <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: pylspt-dev <dir> <file>");

    let db = open_project(SystemPath::new(&dir))?;
    let (file, program_file) = open(&db, SystemPath::new(&file_arg))?;

    println!("python_version = {}", program_file.python_version(&db));
    println!("version source = {:?}", db.python_version_with_source(file).source);
    Ok(())
}
```

Notice what happened: the binary has **no ty logic left**. It reads two
arguments, calls two library functions, prints. That is the target shape for
every command you add.

---

## A note on `tracing` instead of `println!`

`open_project` above uses `tracing::warn!`. That is deliberate.

`println!` writes to stdout, which is where your **JSON output** goes. A stray
`println!` in analysis code corrupts the output your fixtures diff against — and
it will take you an embarrassingly long time to find.

Rules that keep this straight:

- **stdout** — the JSON result, and nothing else. Only binaries write to it.
- **stderr** — diagnostics, warnings, traces. `tracing::warn!`, `eprintln!`.
- analysis code in `lib.rs` uses `tracing`, never `println!`.

You already have `tracing` in your dependency list. Add
`tracing-subscriber` and one line in each binary to see the output:

```rust
tracing_subscriber::fmt().with_writer(std::io::stderr).init();
```

---

## Do it now

**A.** Create `src/lib.rs`, `src/db.rs`, `src/bin/pylspt-dev.rs` and the
`[lib]` / `[[bin]]` sections. Move your smoke test into the shape above. Delete
`src/main.rs` (or keep it as a scratch pad — but nothing important lives there).

**B.** Confirm `cargo run --bin pylspt-dev -- <dir> <file>` still works.

**C.** Add `tracing-subscriber` and initialise it in the binary. Make
`open_project` log the discovered root at `info` level, and confirm it appears on
stderr and **not** in stdout:

```bash
cargo run --bin pylspt-dev -- <dir> <file> 2>/dev/null    # only the results
cargo run --bin pylspt-dev -- <dir> <file> 1>/dev/null    # only the logs
```

**D.** Write your first test. In `src/db.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_requires_python_39() {
        // point at experience/04-python-version/python/proj-requires39
        // assert the resolved version is 3.9
    }
}
```

Run `cargo test`. This is the test that would have caught the version surprise
from exercise 04 before you ever noticed it by hand — and it is only possible
because the logic is in a library.

---

## Exam

**1.** Why can a binary-only crate not be tested from `tests/` or reused by a
second binary?

**2.** You create `src/position.rs` and nothing in it seems to exist. What did
you forget?

**3.** What goes in `lib.rs` and its modules, and what goes in `src/bin/*.rs`?
Give the one-sentence test for whether you have it right.

**4.** Why must the CLI and the RPC server share the same functions rather than
each doing its own thing?

**5.** Why is a `println!` inside analysis code a bug rather than a style
problem?

**6.** Which stream carries results, which carries diagnostics, and how would you
check that you got it right from the shell?

**7.** `open` returns `ProgramFile<'db>` and takes `&'db ProjectDatabase`. What
would go wrong if the signature had no lifetimes?

**8.** Your binary says `use pylspt::db::open_project;`. What must be true of
the module and the function for that to compile?

---

## Answers

**1.** Because a binary has no public API — nothing links against it. Integration
tests in `tests/` and other binaries can only `use` items from a **library**
target. `#[cfg(test)]` unit tests inside `main.rs` work, but they are the only
kind you get.

**2.** `pub mod position;` in `lib.rs`. Rust does not discover files; modules are
declared. A `.rs` file nobody declares is never compiled.

**3.** `lib.rs` and its modules hold all analysis — everything that computes an
answer. `src/bin/*.rs` reads arguments, calls the library, prints, and exits.

The test: **could you delete every binary and still have a complete, usable
library?** If not, logic has leaked into the CLI.

**4.** Because the CLI is how you generate and check fixtures. If the two paths
can diverge, then a passing fixture says nothing about the server that actually
ships — you would be testing a program nobody runs.

**5.** Because stdout carries the JSON your fixtures diff against. A `println!`
buried in analysis code injects text into that stream, corrupting output for
some inputs and not others, with no indication of where it came from. It is a
correctness bug in the output format.

**6.** stdout = results, stderr = diagnostics. Check with `2>/dev/null` (should
leave clean JSON) and `1>/dev/null` (should leave only logs).

**7.** `ProgramFile` borrows from the database, so without lifetimes the compiler
cannot know the returned value must not outlive the borrow — it would refuse to
compile (`missing lifetime specifier`). Tying both to `'db` states the true
relationship: the handle is valid exactly as long as your borrow of the database.

**8.** The module must be `pub mod db;` in `lib.rs`, and the function must be
`pub fn open_project`. Both, or the binary cannot see it — a binary is an
external consumer of your library, with no special access.
