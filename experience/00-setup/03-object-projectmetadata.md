# Object 3 — `ProjectMetadata`

The answer to "what project am I looking at, and how is it configured?"

---

## What it is

A plain settings object. It holds:

- the project's **name**
- the project's **root directory**
- the merged **options** from `pyproject.toml` / `ty.toml` / CLI overrides

That is all it is. It does **not** hold files, it does not parse Python, it does
not compute anything. Think of it as the result of reading configuration.

```
   your directory
        │
        │  ProjectMetadata::discover(...)
        ▼
   ProjectMetadata { name, root, options }
        │
        │  ProjectDatabase::use_defaults(metadata, system)     ← object 4
        ▼
   a database that can actually answer questions
```

It is the **input** to building a database. You make one, hand it over, and
mostly never touch it again.

---

## Why it exists separately from the database

Because configuration discovery is a distinct step that can **fail** or produce
surprising results, and you may want to inspect or override it before committing
to it.

The most important case for you: this is where the **Python version** gets
decided (exercise 04). If you ever implement the permissive-parse floor from
`plan/01-crates/03`, you do it by modifying options *here* — before the database
exists. Once the database is built, the version is baked in.

---

## Where it comes from

```rust
use ty_project::{ProjectMetadata, ProjectMetadataError};
```

---

## How discovery works

```rust
pub fn discover(path: &SystemPath, system: &dyn System)
    -> Result<ProjectMetadata, ProjectMetadataError>;
```

**[verified]** — and the doc comment states the search order:

> 1. The closest `pyproject.toml` with a `tool.ty` section, or `ty.toml`.
> 2. The uv workspace root, if uv integration is enabled.
> 3. The closest `pyproject.toml`.
> 4. Fallback: use `path` as the root, with default settings.

"Closest" means: start at `path` and walk **up** through its ancestors — the
`ancestors()` iterator from object 1, used for real.

Note step 4: **discovery does not fail just because there is no project.** It
falls back to treating your directory as the root. The `Result` is for *broken*
configuration (malformed TOML, invalid values), not for *absent* configuration.

### The three discovery functions

| function | uv workspace detection |
|---|---|
| `discover(path, system)` | ★ decided automatically from the environment |
| `discover_with_uv(path, system, use_uv)` | you control it |
| `discover_without_uv(path, system)` | never |

**[verified]** all three are public. Use `discover` unless you have a reason.
`discover_with_uv` shells out to the `uv` binary (via `System::which`), so on a
machine without uv, or in a test, `discover_without_uv` is faster and more
predictable.

---

## What you can do with it

**[verified]** from `ty_project/src/metadata.rs`.

### Making one

| function | use |
|---|---|
| `discover(path, system)` | ★ the normal path |
| `discover_without_uv(path, system)` | tests, fixtures, no-uv machines |
| `new(name, root)` | ★★ **build one by hand** — single-file mode |
| `from_config_file(path, system)` | point at a specific config file |

`ProjectMetadata::new(name, root)` is the one the plan does not mention and you
will need. When your CLI is handed a lone fixture file with no project around
it, you synthesise metadata instead of discovering it
(`plan/04-build/00-dev-cli.md` §"Single-file mode").

### Reading it

| method | returns |
|---|---|
| `root()` | `&SystemPath` — ★ where the project starts |
| `options()` | `&Options` — the merged configuration |
| `to_merged_options()` | `MergedOptions<'_>` |

### Changing it before you build the database

| method | meaning |
|---|---|
| `apply_override_options(options)` | ★ **highest** priority — like a CLI flag |
| `apply_fallback_options(options)` | **lowest** priority — used only if nothing else set it |
| `apply_configuration_files(...)` | merge in user-level config files |

That pair is how you implement `--python-version` on your CLI: build `Options`
with the version set, then `apply_override_options`. And it is how you would
implement the permissive-parse floor — as a *fallback* or *override*, depending
on whether you want the user's config to win.

**Rust note.** `apply_override_options(&mut self, …)` takes `&mut self`, so your
metadata binding must be `let mut metadata = …`. If you forget, the compiler
says "cannot borrow as mutable"; add `mut`.

---

## Example 1 — discover and inspect

```rust
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::ProjectMetadata;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir>");
    let system = OsSystem::new(&dir);

    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;

    println!("root = {}", metadata.root());
    Ok(())
}
```

Run it against the exercise-04 fixtures and watch the root move:

```
$ cargo run -- experience/04-python-version/python/proj-requires39
root = /Users/yared/.../python/proj-requires39

$ cargo run -- experience/04-python-version/python/proj-requires39/does/not/exist
```

Then try running it against a **subdirectory** of a project — say
`experience/05-modules-and-imports/python/proj/src/pkg`. The root that comes back
is the *project* root, not the directory you passed, because discovery walked up
and found `pyproject.toml`. That single observation is the whole behaviour of
this object.

**Rust note.** `println!("{}", metadata.root())` works without `.as_str()`
because `SystemPath` implements `Display`. When in doubt, try `{}` first; if the
compiler complains that the type "cannot be formatted with the default
formatter", use `{:?}`.

---

## Example 2 — single-file mode

```rust
use ruff_db::system::{OsSystem, SystemPath, SystemPathBuf};
use ty_project::ProjectMetadata;

/// Discover a project around `path`, or synthesise one if there is nothing.
fn metadata_for(path: &SystemPath, system: &OsSystem) -> anyhow::Result<ProjectMetadata> {
    // A file's project is discovered from its DIRECTORY, not from the file.
    let start = if system_is_file(system, path) {
        path.parent().unwrap_or(path)
    } else {
        path
    };

    match ProjectMetadata::discover(start, system) {
        Ok(metadata) => Ok(metadata),
        Err(err) => {
            // Broken config. Do not die — synthesise. (quirk 13: swallow failures)
            eprintln!("warning: project discovery failed: {err}. using defaults.");
            Ok(ProjectMetadata::new("adhoc", SystemPathBuf::from(start.as_str())))
        }
    }
}
```

Two decisions in there worth noticing:

1. **Discover from the directory**, not the file. Passing a file path works, but
   being explicit avoids surprises when the file does not exist yet.
2. **Never propagate a discovery failure.** Your Python driver swallows failures
   everywhere (`plan/00-orientation/01`, quirk 13). A malformed `pyproject.toml`
   in one project must not take your analyser down.

⚠ `system_is_file` above is a stand-in for `system.is_file(path)` — you need
`use ruff_db::system::System;` for that method. Write it properly in your
version.

---

## Exercise

**A.** Print the discovered root for all six fixture directories:

```
experience/03-the-database/python/proj
experience/04-python-version/python/proj-bare
experience/04-python-version/python/proj-requires39
experience/04-python-version/python/proj-tytoml313
experience/05-modules-and-imports/python/proj
experience/05-modules-and-imports/python/proj/src/pkg    ← predict this one first
```

The last one is the test. Write your prediction down before running.

**B.** Implement `metadata_for` properly, with the `is_file` check and the
fallback. You will use this function in every later exercise, so put it in
`src/db.rs` rather than in `main`.

**C.** Compare `discover` and `discover_without_uv` on the same directory. Do you
get the same root? Time both. (If you do not have uv installed, note what
happens — that is useful information about your fallback path.)

---

## Exam

**1.** What does `ProjectMetadata` hold? Name the three things.

**2.** What does it *not* hold? Name two things people assume it does.

**3.** Give the four-step discovery order.

**4.** `discover` returns a `Result`. Does "no `pyproject.toml` anywhere" produce
an `Err`? What does produce one?

**5.** You pass a path deep inside a project. What root comes back, and by what
mechanism?

**6.** Why does `ProjectMetadata::new` exist when `discover` already handles the
no-config case?

**7.** You want to force Python 3.12 regardless of what the project says. Which
method, and why must it happen before the database is built?

**8.** What is the difference between `apply_override_options` and
`apply_fallback_options`? Give a use for each.

---

## Answers

**1.** The project name, the project root (`SystemPathBuf`), and the merged
options from configuration files and overrides.

**2.** It does not hold **files** (no file list, no contents) and it does not
hold any **analysis or caches**. It is configuration only — the database (object
4) is what holds state.

**3.** (1) closest `pyproject.toml` with a `tool.ty` section, or `ty.toml`;
(2) the uv workspace root, if uv integration is on; (3) the closest
`pyproject.toml`; (4) fall back to the given path as root with default settings.
**[verified]** from the doc comment.

**4.** No — step 4 makes "no configuration" a success, using your path as the
root. An `Err` means **broken** configuration: malformed TOML, an unknown key,
an invalid value.

That distinction matters for your error handling: `Err` is a real user-facing
problem worth logging, not the routine case.

**5.** The **project** root — the directory containing the config file — because
discovery walks *up* through the path's ancestors looking for one. Same
`ancestors()` iterator from object 1.

**6.** Because `discover` always returns *some* project, but it also touches the
filesystem, can shell out to uv, and can fail on broken config. For a fixture
file with no project around it, `new(name, root)` is faster, deterministic and
cannot fail — which is what you want when running 37 fixtures in a test suite.

**7.** `apply_override_options`, with `Options` carrying the version. It must
happen before `ProjectDatabase::use_defaults(metadata, system)`, because the
database resolves and freezes the program settings when it is constructed. After
that, the version is part of the cache keys (`ProgramFile`, object 6) and cannot
be changed without rebuilding.

That is the real reason exercise 04 says the version is a *startup* decision,
not a per-file one.

**8.** Override = highest priority, wins over everything in the project's config
— use it for CLI flags the user explicitly passed. Fallback = lowest priority,
used only when nothing else specified a value — use it for your own defaults,
like "if this project says nothing at all, assume 3.12".

The permissive-parse floor from `plan/01-crates/03` is a genuine design question
between the two: a *fallback* respects a project that pins 3.9, an *override*
ignores it. The plan's argument (you are an analyser, not a checker) points at
override; the risk is diverging from ty's own view of the project.
