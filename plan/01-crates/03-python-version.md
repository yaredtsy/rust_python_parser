# 01.03 — ⚠ The Python version

> You flagged this: *"check the python version carefully, most time it confuse
> model"*. You were right, and it is worse than a documentation problem — there
> is a **silent wrong default** in the most obvious API.

Everything here is **[verified]** from
`ruff_python_ast/src/python_version.rs` and
`ruff_python_parser/src/parser/options.rs` @ `ac201b8`.

---

## The constants

```rust
// ruff_python_ast::PythonVersion
PythonVersion::lowest()        == PY37     // minimum supported
PythonVersion::default()       == PY310    // ⚠ the Default impl
PythonVersion::latest()        == PY314    // latest stable, for ruff
PythonVersion::latest_ty()     == PY314    // what ty defaults to
PythonVersion::latest_preview() == PY315
```

Note that `default()` (3.10) and `latest_ty()` (3.14) **disagree by four
releases**. Which one you get depends entirely on which API you call.

## The trap

```rust
// ruff_python_parser/src/parser/options.rs  [verified]
impl From<Mode> for ParseOptions {
    fn from(mode: Mode) -> Self {
        Self { mode, target_version: PythonVersion::default() }   // ← 3.10
    }
}
impl From<PySourceType> for ParseOptions {
    fn from(source_type: PySourceType) -> Self {
        Self { mode: source_type.as_mode(), target_version: PythonVersion::default() }  // ← 3.10
    }
}
```

So all of these silently target **Python 3.10**:

```rust
parse_module(source)                                    // ⚠ 3.10
parse(source, ParseOptions::from(Mode::Module))         // ⚠ 3.10
parse_unchecked_source(source, PySourceType::Python)    // ⚠ 3.10
```

And the field is `pub(crate)`, so you cannot see it in the struct literal —
you must go through the builder:

```rust
ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313)
```

`with_target_version` and `target_version()` are the only public accessors.

---

## What actually breaks when the version is wrong

| Version gate | Method | What goes wrong at 3.10 |
|---|---|---|
| **3.12** PEP 695 | — | `type X = int`, `def f[T](x: T)`, `class C[T]` are reported as `UnsupportedSyntaxError` |
| **3.12** PEP 701 | `supports_pep_701()` | f-string tokenisation differs. Nested same-quote f-strings, backslashes and comments inside f-strings parse differently. **Token ranges inside f-strings move.** |
| **3.14** PEP 649 | `defers_annotations()` | annotation evaluation semantics change; affects how ty treats forward references |
| **3.10** | — | `match` statements are fine at 3.10 (3.10 introduced them), but *not* below |
| **3.13** | `free_threaded_build_available()` | irrelevant to you |

The insidious one is **PEP 701**. It does not produce an error — it produces
*different token ranges inside f-strings*. Your `call_col_pos` and `position`
fields come from ranges. A call inside an f-string (`f"{compute(x)}"`) can get
the wrong column, silently, on the wrong target version.

---

## Where ty gets the version from

Resolution order, highest priority first (from `ty_project::metadata`
**[verified]**, plus `PythonVersionSource` in `ty_site_packages`):

1. Explicit CLI/API override (`--python-version`)
2. `[tool.ty.environment] python-version` in `ty.toml` / `pyproject.toml`
3. `requires-python` in `pyproject.toml` → **lower bound is used**
4. The resolved Python environment (`.venv`, uv workspace) — its actual interpreter version
5. Fallback: `PythonVersion::latest_ty()` == **3.14**

> Note #3: `requires-python = ">=3.9"` gives you **3.9**, not the installed
> interpreter. A project that declares broad compatibility but uses `match`
> statements in a 3.12-only code path will produce syntax errors. This is
> correct behaviour for a type checker and *wrong* for your use case, where you
> want to parse whatever is actually there.

Read it back with **[verified]**:

```rust
use ty_python_core::Program;
let version = Program::get(db).python_version(db);
```

---

## Rules for this project

**1. Never call `ruff_python_parser` directly for a project file.**
Go through `parsed_module(db, python_file)`. It is version-wired and cached.
Direct parsing bypasses both, and `AstNodeRef` will reject the resulting nodes
(there is a test literally named
`rejects_module_parsed_for_different_python_version` **[verified]**).

**2. When you must parse standalone** — e.g. `parse_file` receives `content`
from the client that is not yet on disk — always pass the version explicitly:

```rust
let version = Program::get(db).python_version(db);
let parsed = ruff_python_parser::parse_unchecked(
    &content,
    ParseOptions::from(Mode::Module).with_target_version(version),
);
```

Never let `Default` decide.

**3. Prefer maximum permissiveness over strictness.**
Your driver is a structural analyser, not a linter. It should parse 3.14 syntax
even in a project declaring `requires-python = ">=3.8"`, because the goal is to
*see the code*, not to validate it. Consider:

```rust
// pylspt: parse as permissively as possible; version only affects
// UnsupportedSyntaxError reporting (which we discard) and f-string tokenisation.
let parse_version = std::cmp::max(
    Program::get(db).python_version(db),
    PythonVersion::PY312,   // floor: get PEP 701 f-string tokenisation
);
```

**Decide this deliberately and write the decision down.** The failure mode —
"some files silently produce no nodes on one machine" — is miserable to debug
six months later. `Parsed::unsupported_syntax_errors()` is the signal to log
when it happens.

**4. Log the resolved version at `initialize` time.** One line:

```
pylspt: project=/x/y python_version=3.12 (source: pyproject requires-python)
```

Jedi took the version from `InterpreterEnvironment()` — the *running*
interpreter. ty takes it from *configuration*. **These will disagree**, and
that disagreement is a behaviour change between your old and new driver. It is
the most likely cause of "the Rust one found fewer nodes" during parity
testing. → [`04-build/03-transport-and-parity.md`](../04-build/03-transport-and-parity.md)

---

## Checklist

- [ ] Never `parse_module(src)` without `.with_target_version(...)`
- [ ] Prefer `parsed_module(db, file)` over any direct parse
- [ ] Log resolved version + its source at startup
- [ ] Log `unsupported_syntax_errors()` per file at debug level
- [ ] Decide + document the permissive-parse policy
- [ ] Parity-test a file using 3.12 PEP 695 syntax and one with nested f-strings

---

→ Next: [`04-public-vs-private-api.md`](04-public-vs-private-api.md)
