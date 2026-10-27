# 04.01 — Wiring Cargo

The mechanical setup. Do this first; a build that resolves is a prerequisite for
everything else, and dependency skew here produces error messages that look like
logic bugs.

---

## Current state

```toml
# /Users/yared/Documents/Programing/rust/pylspt/Cargo.toml   [verified]
[package]
name = "pylspt"
version = "0.1.0"
edition = "2024"

[dependencies]
```

Empty. Good — nothing to undo.

## Toolchain

**[verified]** ruff @ `ac201b8` requires:

```toml
# rust-toolchain.toml — create this in pylspt/
[toolchain]
channel = "1.98.0"
```

- workspace `edition = "2024"`, `rust-version = "1.96"`
- ruff's own `rust-toolchain.toml` pins `1.98.0`

Your `edition = "2024"` already matches. **Pin the same channel** — ty uses
recent language features and a mismatched toolchain gives you compile errors in
*their* code, which is a confusing place to start debugging.

---

## Option A — workspace member ★ DECIDED, direction reversed 2026-10-17

See [`01-crates/04`](../01-crates/04-public-vs-private-api.md) for the current
DECIDED section — that is the source of truth for the layout now; what follows
in this file is partly stale (written for the fork-wholesale shape this
reverses) and not yet fully rewritten to match. Read the layout from there,
not from the diagram and warning this replaces.

> ⚠ **This section used to say "Fork; do not vendor"**, warning against
> nesting ruff inside `pylspt/vendor/ruff` as a submodule. That warning is
> itself now reversed — `01-crates/04`'s DECIDED section explains why
> (`pylspt` is the repo of record; a full ruff fork makes it one crate among
> someone else's fifty). What's actually at `pylspt/vendor/` today is neither
> of the two things that sentence considered: not a submodule (a plain
> directory copy of each needed crate, so a rebase is "re-copy the files and
> re-apply the one `vnoc.rs` diff", not a submodule bump), and not "all of
> ruff" (a computed closure of ~26 crates — whatever `ty_python_semantic`
> actually depends on — not the full ~50-crate workspace). The wrapper
> discipline (one wrapper file, public types only, add on demand) is
> unchanged by any of this; only the directory nesting direction and the trim
> are new.

### Layout — `pylspt` is the repo; a trimmed closure lives at `vendor/`

```
pylspt/                     ← the repo. workspace root AND the pylspt package.
├── Cargo.toml              [workspace] members = ["vendor/*"]
├── src/                    ← your crate, all your logic
└── vendor/                 ← ty_python_semantic's real dependency closure,
    ├── ruff_db/              computed by walking [dependencies] sections,
    ├── ty_python_semantic/   not hand-picked. Unchanged except:
    │   └── src/types.rs        + `pub mod vnoc;`
    │   └── src/types/vnoc.rs   ← NEW: wrappers only
    └── ... (~24 more crates)
```

Add a crate, and `members = ["vendor/*"]` already covers it — nothing else to
configure.

### Root `Cargo.toml` — ruff's, with `crates/*` already covering you

```toml
[workspace]
members = ["crates/*"]        # ← already matches crates/pylspt. nothing to add.
resolver = "2"

[workspace.package]
edition = "2024"
rust-version = "1.96"

# Inherit ruff's dependency versions verbatim so there is exactly one
# salsa, one rustc-hash, one smallvec in the graph.
[workspace.dependencies]
# --- ruff/ty (path deps into the submodule) ---
ruff_db              = { path = "vendor/ruff/crates/ruff_db", default-features = false }
ruff_python_ast      = { path = "vendor/ruff/crates/ruff_python_ast" }
ruff_python_parser   = { path = "vendor/ruff/crates/ruff_python_parser" }
ruff_python_stdlib   = { path = "vendor/ruff/crates/ruff_python_stdlib" }
ruff_source_file     = { path = "vendor/ruff/crates/ruff_source_file" }
ruff_text_size       = { path = "vendor/ruff/crates/ruff_text_size" }
ty_ide               = { path = "vendor/ruff/crates/ty_ide" }
ty_module_resolver   = { path = "vendor/ruff/crates/ty_module_resolver" }
ty_project           = { path = "vendor/ruff/crates/ty_project", default-features = false }
ty_python_core       = { path = "vendor/ruff/crates/ty_python_core" }
ty_python_semantic   = { path = "vendor/ruff/crates/ty_python_semantic" }

# --- shared, versions taken from ruff's workspace [verified] ---
salsa = { version = "0.28.2", default-features = false, features = [
    "compact_str", "macros", "salsa_unstable", "inventory",
] }
rustc-hash = "2.0.0"
smallvec   = { version = "1.13.2", features = ["union", "const_generics", "const_new"] }
indexmap   = "2.6.0"
camino     = "1.1.7"
serde      = { version = "1.0.197", features = ["derive"] }
uuid       = "1.6.1"

# Lossless CST for ID injection. Ruff already pins this exact version.
# KEEP default-features = false — the defaults build the PyO3 extension module.
libcst     = { version = "1.8.4", default-features = false }
```

> Under Option A you inherit ruff's `libcst` entry automatically — write
> `libcst = { workspace = true }` in your crate, exactly as `ruff_linter` does
> **[verified]**. That guarantees one copy in the tree.

> ✅ **`salsa` is now a solved problem.** As a workspace member you write
> `salsa = { workspace = true }` and inherit ruff's entry — exactly
> `0.28.2`, `default-features = false`, features `compact_str`, `macros`,
> `salsa_unstable`, `inventory` **[verified, ruff `Cargo.toml:165`]**. There is
> no second version to unify and nothing to keep in sync by hand.
>
> This is the whole reason rule 6 of the fork discipline exists. Outside the
> workspace you must match that line yourself, and two different salsa versions
> in one graph give you `expected Db, found Db` — two distinct traits with the
> same name, in an error message where both types print identically. Diagnose
> with `cargo tree -d -p salsa` (`-d` = duplicates) if it ever recurs.

### `crates/pylspt/Cargo.toml`

```toml
[package]
name = "pylspt"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
ruff_db.workspace = true
ruff_python_ast.workspace = true
ruff_python_parser.workspace = true
ruff_python_stdlib.workspace = true
ruff_source_file.workspace = true
libcst.workspace = true
ruff_text_size.workspace = true
ty_ide.workspace = true
ty_module_resolver.workspace = true
ty_project.workspace = true
ty_python_core.workspace = true
ty_python_semantic.workspace = true
salsa.workspace = true
rustc-hash.workspace = true
smallvec.workspace = true
serde.workspace = true
uuid.workspace = true

# yours
anyhow      = "1"
serde_json  = "1"
rpds        = "1"                      # persistent maps for Env  → 03-call-tree/05-the-walk
tracing     = "0.1"
tracing-subscriber = "0.3"

[dev-dependencies]
insta = "1"                            # snapshot tests, same as ruff uses
```

### Build profile

```toml
# root Cargo.toml
[profile.release]
lto = "thin"
codegen-units = 1
panic = "abort"        # ⚠ NO — see below

[profile.dev]
opt-level = 1          # ty is unusably slow in a fully-unoptimised build
[profile.dev.package."*"]
opt-level = 3          # optimise deps even in dev builds
```

> ⚠ **Do not set `panic = "abort"`.** Salsa uses **unwinding** for query
> cancellation and for cycle recovery. `panic = "abort"` turns a routine
> cancellation into a process kill. This will look like random crashes under
> concurrent edits.

`opt-level = 1` for dev + `opt-level = 3` for deps is the standard ruff-adjacent
setup; without it the type checker is slow enough to make your test loop painful.

### Expected build times

- Cold: **5–10 minutes** (54 crates + typeshed).
- Incremental (your crate only): **10–30 seconds**.
- `cargo check`: **~5 seconds**.

Use `cargo check` in your inner loop. Consider `sccache` or a
`CARGO_TARGET_DIR` shared with ruff so the vendored crates build once.

---

> **`04-local-ty-checkout.md` is deleted.** It described a git-ignored
> `.cargo/config.toml` `[patch]` pointing `pylspt`'s committed `git = "…"`
> dependency lines at a local ruff checkout, for fast edit/rebuild on
> `vnoc.rs` without a commit-and-rev-bump round trip. Moot now: `pylspt`
> doesn't have `git = "…"` lines any more — `vendor/ty_python_semantic` is a
> real, local, already-checked-out copy, so editing `vendor/.../vnoc.rs` and
> rebuilding already *is* the fast loop that file was trying to approximate.

## Option B — git dependency ← *what this repo did until the fork*

This is how `pylspt/Cargo.toml` looks today, and it is what the fork replaces.
Keep it documented: it is the fallback if the fork ever becomes untenable, and
it is still the right shape for a throwaway spike against a *different* ruff
revision.

```toml
[dependencies]
ty_python_semantic = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_project         = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ty_ide             = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
ruff_db            = { git = "https://github.com/astral-sh/ruff", rev = "ac201b8" }
# ...
salsa = { version = "0.28.2", default-features = false, features = [
    "compact_str", "macros", "salsa_unstable", "inventory",
] }
```

Works because Cargo allows git deps on unpublished crates. But:
- Every crate needs its own `git`+`rev` line (no path shorthand).
- You **must** replicate the exact `salsa` feature set or the `Db` traits differ.
- `default-features = false` on `ruff_db` and `ty_project` matters
  **[verified]** — that's how ruff's own workspace declares them.
- No access to `pub(crate)`. → [`01-crates/04`](../01-crates/04-public-vs-private-api.md)

---

## First build — smoke test

Write this before anything else. If it prints, your wiring is correct and every
later error is your own code.

```rust
use ruff_db::system::{OsSystem, SystemPathBuf};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::Program;

fn main() -> anyhow::Result<()> {
    let root = SystemPathBuf::from(std::env::args().nth(1).expect("usage: pylspt <dir>"));
    let system = OsSystem::new(&root);
    let metadata = ProjectMetadata::discover(&root, &system)?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    // ★ the version line from 01-crates/03 — log this for real, at startup
    println!("python_version = {}", Program::get(&db).python_version(&db));
    Ok(())
}
```

Run it against your own project and against a `.venv`-having project. If the
version differs from what Jedi's `InterpreterEnvironment()` would report, you've
already found your first parity difference — before writing any analysis.

---

## Pinning and upgrades

Record the pin where someone will see it:

```toml
# vendor/ruff pinned at ac201b8
#   ruff 0.16.5 · ty crates 0.0.11 · toolchain 1.98.0 · 2026-09-02
# Upgrade procedure: plan/04-build/01-wiring-cargo.md#upgrading
```

### Upgrading

1. `git -C vendor/ruff fetch && git -C vendor/ruff checkout <new-rev>`
2. `cargo check` — expect breakage in `ide_support` signatures and `Type` variants.
3. Rebase your visibility patch (should be one commit, <100 lines).
4. Run the parity suite from [`03-transport-and-parity.md`](03-transport-and-parity.md).
5. Re-check the resolved Python version — defaults have moved before
   (`latest_ty` is 3.14 today) and will move again.

Do this on a schedule (quarterly), not reactively. A 6-month-old pin is a
painful rebase; a 3-month-old one is an afternoon.

---

→ Next: [`02-milestones.md`](02-milestones.md)
→ Also: [`00-dev-cli.md`](00-dev-cli.md) — the `version` command belongs in this
  milestone, and is the fastest way to prove the wiring works.
