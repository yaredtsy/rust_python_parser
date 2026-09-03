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

## Option A — workspace member (recommended)

See [`01-crates/04`](../01-crates/04-public-vs-private-api.md) for why.

### Layout

```
pylspt/
├── vendor/ruff/          ← git submodule, pinned at ac201b8
├── crates/pylspt/        ← your code
├── Cargo.toml            ← workspace root
└── rust-toolchain.toml
```

```bash
cd /Users/yared/Documents/Programing/rust/pylspt
git submodule add https://github.com/astral-sh/ruff vendor/ruff
git -C vendor/ruff checkout ac201b8
```

### Root `Cargo.toml`

```toml
[workspace]
members = ["crates/pylspt", "vendor/ruff/crates/*"]
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

> ⚠ **`salsa` must be exactly `0.28.2` with exactly those features.**
> **[verified]** from ruff's workspace manifest. If Cargo unifies two different
> salsa versions you get `expected Db, found Db` — two distinct traits with the
> same name. Symptom: an error message where both types print identically.
> Diagnose with `cargo tree -d -p salsa` (`-d` = duplicates).

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
rpds        = "1"                      # persistent maps for Env  → 03-call-tree/05
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

## Option B — git dependency

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
