# Object 7 — `PythonVersion`

Two small integers with a booby trap attached.

---

## What it is

```rust
pub struct PythonVersion {
    pub major: u8,
    pub minor: u8,
}
```

**[verified, `ruff_python_ast/src/python_version.rs:9`]**. That is the entire
type. Both fields are public, it is `Copy`, `Ord` (so you can compare versions
with `<`, `>`, `max`), and it implements `Display` so `{}` prints `3.12`.

Note where it lives: **`ruff_python_ast`**, the AST crate — not the parser crate,
where you would expect it. That surprises everyone once.

---

## Why you care, right now

Because the version decides:

- whether a file **parses at all** (`type X = int` needs 3.12)
- how f-strings are **tokenised** — which moves the column numbers your wire
  format reports
- which typeshed stubs apply

Exercise 04 is dedicated to this. Here you need just enough to print it and to
avoid the trap.

---

## The constants

```rust
PythonVersion::lowest()         // PY37   — minimum supported
PythonVersion::default()        // PY310  ⚠
PythonVersion::latest()         // PY314
PythonVersion::latest_ty()      // PY314  — what ty falls back to
PythonVersion::latest_preview() // PY315
PythonVersion::PY312            // …and one constant per version
```

**[verified]**. Look at those two middle lines again:

```
default()   == 3.10
latest_ty() == 3.14
```

**They disagree by four releases.** And `default()` is what you get *implicitly*,
from anything that does not ask you:

```rust
// ruff_python_parser/src/parser/options.rs  [verified]
impl From<Mode> for ParseOptions {
    fn from(mode: Mode) -> Self {
        Self { mode, target_version: PythonVersion::default() }   // ← 3.10
    }
}
```

So every one of these silently targets 3.10:

```rust
parse_module(source)                                   // ⚠
parse(source, ParseOptions::from(Mode::Module))        // ⚠
parse_unchecked_source(source, PySourceType::Python)   // ⚠
```

The rule that keeps you safe is simple, and it is the same rule for a different
reason (caching): **never parse a project file yourself — go through
`parsed_module(db, python_file)`**, which is version-wired and cached. When you
genuinely must parse standalone (the editor sent you unsaved text), set the
version explicitly:

```rust
ParseOptions::from(Mode::Module).with_target_version(version)
```

`with_target_version` is the only public setter — the field is `pub(crate)`
**[verified]**, so you cannot even write it in a struct literal.

---

## Where the version comes from

Priority order, highest first:

1. an explicit override (a CLI flag you implement)
2. `[tool.ty.environment] python-version` in `pyproject.toml`, or `[environment]`
   in `ty.toml`
3. `requires-python` in `pyproject.toml` — **the lower bound**
4. the resolved Python environment (`.venv`, uv workspace)
5. fallback: `latest_ty()` == **3.14**

Item 3 is the one that surprises people: `requires-python = ">=3.9"` gives you
**3.9**, not your installed interpreter.

Compare with Jedi, which used `InterpreterEnvironment()` — the *running*
interpreter. **These disagree**, and that disagreement is a behaviour change
between your old driver and your new one. Exercise 04 makes you measure it.

---

## Reading it back

```rust
use ty_python_semantic::Db as _;

let version = db.program_file(file).python_version(&db);        // ★ PythonVersion
let with_source = db.python_version_with_source(file);          // ★ + where it came from
```

**[verified]** both are public. The second returns a `&PythonVersionWithSource`
with public fields:

```rust
pub struct PythonVersionWithSource {
    pub version: PythonVersion,
    pub source: PythonVersionSource,   // ConfigFile, PyvenvCfgFile, Cli, …
}
```

⚠ **There is no `Program::get(db)`.** `plan/04-build/01-wiring-cargo.md`'s smoke
test uses it and it does not exist at `ac201b8` **[verified]** — `Program` is a
salsa-interned struct you obtain *from* a file, not a global you fetch. Going
through `db.program_file(file)` is the route that works.

**Rust note — using a type you cannot name.** `PythonVersionSource` lives in
`ty_site_packages`, which is not in your `Cargo.toml`. You still can:

```rust
println!("{:?}", db.python_version_with_source(file).source);
```

Calling methods and reading public fields does not require naming the type. You
would only need the dependency to write `let s: PythonVersionSource = …` or to
`match` on its variants. Worth knowing — it saves you from adding dependencies
you do not need.

---

## What you can do with it

| item | notes |
|---|---|
| `.major`, `.minor` | public `u8` fields |
| `{}` formatting | `Display` → `3.12` |
| `==`, `<`, `>`, `.max()`, `.min()` | `Ord` — versions compare correctly |
| `supports_pep_701()` | **[verified]** — is f-string tokenisation the new kind |
| `defers_annotations()` | **[verified]** — PEP 649, 3.14+ |
| `PythonVersion::PY312` etc. | one constant per release |

---

## Example 1 — print the version and its source

```rust
use ruff_db::files::system_path_to_file;
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let system = OsSystem::new(&dir);
    let metadata = ProjectMetadata::discover(SystemPath::new(&dir), &system)?;
    let db = ProjectDatabase::use_defaults(metadata, system);

    let file = system_path_to_file(&db, SystemPath::new(&file_arg))?;
    let program_file = db.program_file(file);

    let version = program_file.python_version(&db);
    let with_source = db.python_version_with_source(file);

    println!("python_version = {version}");
    println!("source         = {:?}", with_source.source);
    println!("pep 701 f-strings: {}", version.supports_pep_701());

    Ok(())
}
```

Run it on the three exercise-04 fixtures and **predict each first**:

```bash
cargo run -- experience/04-python-version/python/proj-bare          .../proj-bare/app.py
cargo run -- experience/04-python-version/python/proj-requires39    .../proj-requires39/app.py
cargo run -- experience/04-python-version/python/proj-tytoml313     .../proj-tytoml313/app.py
```

You will get one of them wrong. That is exercise 04's entire subject, and being
surprised now is the point.

---

## Example 2 — the warning line worth writing

```rust
// What ty resolved:
let ty_version = program_file.python_version(&db);

// What jedi would have used: the RUNNING interpreter.
let output = std::process::Command::new("python3").arg("--version").output();
if let Ok(out) = output {
    let text = String::from_utf8_lossy(&out.stdout);   // "Python 3.12.2\n"
    if let Some(running) = text.split_whitespace().nth(1) {
        // crude compare: "3.12.2" starts with "3.12"
        if !running.starts_with(&ty_version.to_string()) {
            eprintln!(
                "⚠ running interpreter is {running} — jedi would have used that, \
                 ty resolved {ty_version}"
            );
        }
    }
}
```

`plan/04-build/00-dev-cli.md` asks for this line, and it is worth the ten
minutes: it turns the most likely parity surprise into something you see on day
one rather than in a bug report six weeks later.

---

## Example 3 — the permissive floor (a decision, not code to copy)

```rust
// pylspt: parse as permissively as we can. The version affects
// UnsupportedSyntaxError reporting (which we discard) and f-string tokenisation.
let parse_version = std::cmp::max(
    db.program_file(file).python_version(&db),
    PythonVersion::PY312,          // floor: get PEP 701 f-string tokenisation
);
```

`std::cmp::max` works because `PythonVersion` is `Ord`.

⚠ **But you cannot apply it here.** Remember rule 1: you must go through
`parsed_module(db, file)`, so the version is whatever the *database* was built
with. To actually apply a floor you set it on `ProjectMetadata` before building
the database (object 3, `apply_override_options`).

So this snippet is the *decision*, and object 3 is where it gets *implemented*.
Exercise 04 makes you sort that out properly — it is the difference between a
policy you can ship and one you cannot.

---

## Exercise

**A.** Run example 1 on all three fixture projects. Fill in a table of
prediction / actual / source. Note which one you got wrong and why.

**B.** Add example 2's warning to your binary.

**C.** Print `PythonVersion::default()`, `latest_ty()`, `lowest()` and
`latest()`. Then write, in one sentence, why the existence of `default()` is a
hazard rather than a convenience.

**D.** For each fixture project, print `version.supports_pep_701()`. Which
projects get the modern f-string tokeniser? Connect that to the columns you will
report in exercise 02 — which of these projects could give you wrong
`call_col_pos` values inside f-strings?

---

## Exam

**1.** Which crate is `PythonVersion` in? Why is that surprising?

**2.** What are `default()` and `latest_ty()`, and why is the gap dangerous?

**3.** Name three parser entry points that silently target 3.10, and the method
that fixes them.

**4.** Why can you not set `target_version` in a struct literal?

**5.** Give the five version sources in priority order. Which one surprises
people?

**6.** Where did Jedi get its version? Name a concrete project layout where ty
and Jedi disagree.

**7.** `Program::get(db).python_version(db)` appears in the plan. What is wrong
with it, and what do you write instead?

**8.** `PythonVersionSource` is in a crate you do not depend on. What can you
still do with it, and what can you not?

**9.** You want a parse floor of 3.12. Why can you not apply it at the parse
call, and where does it go instead?

---

## Answers

**1.** `ruff_python_ast` — the **AST** crate, not the parser crate. Surprising
because the version's most visible effect is on parsing. It lives there because
AST nodes themselves are version-dependent (PEP 695 type-parameter nodes only
exist at 3.12+), so the AST crate is the lowest layer that needs the concept.

**2.** `default()` is **3.10**, `latest_ty()` is **3.14** **[verified]**.
Dangerous because `default()` is what you get *implicitly* — from
`ParseOptions::from(...)`, from `..Default::default()` — so the version depends
on which entry point you happened to call, and nothing in the signature warns
you.

**3.** `parse_module(source)`, `parse(source, ParseOptions::from(Mode::Module))`,
`parse_unchecked_source(source, PySourceType::Python)`. Fix with
`.with_target_version(version)`.

**4.** The field is `pub(crate)` **[verified]**, so it is not nameable outside
the parser crate. The builder is the only public route — which at least means
every version choice is greppable.

**5.** (1) explicit override, (2) `[tool.ty.environment] python-version`,
(3) `requires-python` **lower bound**, (4) the resolved environment/`.venv`,
(5) fallback `latest_ty()` = 3.14.

Number 3 surprises people: `>=3.9` yields 3.9, not the installed interpreter.

**6.** `InterpreterEnvironment()` — the running interpreter. Concrete
disagreement: a library with `requires-python = ">=3.8"` (very common) analysed
on a machine running 3.12. Jedi says 3.12 and parses everything; ty says 3.8 and
reports 3.9+ syntax as unsupported.

**7.** There is no `Program::get` at `ac201b8` **[verified]** — `Program` is a
salsa-interned struct obtained from a file, not a global. Write:

```rust
db.program_file(file).python_version(&db)
```

**8.** You can call methods on it and read its public fields — e.g.
`println!("{:?}", …source)` — because none of that requires naming the type. You
cannot write it in a type annotation, `match` on its variants, or store it in a
struct field, without adding the dependency.

**9.** Because you must go through `parsed_module(db, python_file)` (rule 1: the
cache and the version wiring both live there, and `AstNodeRef` rejects nodes
from a differently-versioned parse). The version is therefore fixed when the
**database** is built, so the floor goes on `ProjectMetadata` via
`apply_override_options` *before* `ProjectDatabase::use_defaults`.

The consequence worth remembering: it is a **startup** decision, not a per-file
one.
