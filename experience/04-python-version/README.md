# 04 — ⚠ The Python version

**Goal:** you can say, for any file, which Python version ty is using and where
that number came from — and you have seen, with your own output, a file whose
node tree changes because of it.

Short exercise. Highest ratio of "surprise per minute" in the folder.

You flagged this yourself when the plan was written: *"check the python version
carefully, most time it confuse model"*. You were right, and it is worse than a
documentation problem — the most obvious API has a **silent wrong default**.

---

## Read first

- `plan/01-crates/03-python-version.md` — read it twice, as it says
- `tutorial/07-files-and-modules.md` §"Where the Python version comes from"

---

## The mental model

### Jedi asks the interpreter. ty asks the configuration.

```
jedi                                    ty
────                                    ──
InterpreterEnvironment()                1. explicit override (--python-version)
  → sys.version_info                    2. [tool.ty.environment] python-version
  → whatever python is running          3. requires-python  → LOWER BOUND
                                        4. the resolved environment (.venv)
                                        5. fallback: PythonVersion::latest_ty()
```

These disagree, routinely, and the disagreement is a **behaviour change between
your old driver and your new one**. It is the most likely cause of "the Rust one
found fewer nodes" during parity testing, so you want to have measured it before
anyone reports it.

Item 3 is the one that surprises people. `requires-python = ">=3.9"` gives you
**3.9** — not the installed interpreter, not the latest supported. For a type
checker that is exactly right: the project promised to work on 3.9, so 3.9 is
what must be checked. For *your* driver it is wrong, because you are a
structural analyser and your job is to see the code that is there.

### The constants, and the trap

```rust
PythonVersion::lowest()         == PY37    [verified]
PythonVersion::default()        == PY310   [verified]  ⚠
PythonVersion::latest()         == PY314   [verified]
PythonVersion::latest_ty()      == PY314   [verified]
PythonVersion::latest_preview() == PY315   [verified]
```

`default()` and `latest_ty()` **disagree by four releases**, and which one you
get depends entirely on which API you call:

```rust
// ruff_python_parser/src/parser/options.rs  [verified]
impl From<Mode> for ParseOptions {
    fn from(mode: Mode) -> Self {
        Self { mode, target_version: PythonVersion::default() }   // ← 3.10
    }
}
```

So **all** of these silently target 3.10:

```rust
parse_module(source)                                   // ⚠ 3.10
parse(source, ParseOptions::from(Mode::Module))        // ⚠ 3.10
parse_unchecked_source(source, PySourceType::Python)   // ⚠ 3.10
```

And `target_version` is `pub(crate)` **[verified]**, so you cannot even see it
in a struct literal — the only way to set it is the builder:

```rust
ParseOptions::from(Mode::Module).with_target_version(version)
```

### What actually breaks

| gate | what goes wrong below it |
|---|---|
| **3.12** PEP 695 | `type X = int`, `def f[T]()`, `class C[T]` become `UnsupportedSyntaxError` |
| **3.12** PEP 701 | f-string tokenisation differs. Nested same-quote f-strings, backslashes and comments inside f-strings. **Token ranges inside f-strings move.** |
| **3.14** PEP 649 | annotations are deferred; changes how forward references are treated |
| 3.10 | `match` is fine at 3.10, not below |

**PEP 701 is the insidious one.** It does not produce an error. It produces
*different token ranges inside f-strings* — and your `call_col_pos` and
`position` come from ranges. A call inside `f"{compute(x)}"` can get the wrong
column, silently, with no diagnostic anywhere.

---

## The fixtures

```
python/
├── helpers.py ............ callees
├── pep695.py ............. 3.12+ generics and type aliases → hard errors below 3.12
├── fstrings.py ........... 3.12+ f-strings AND a plain one → range differences
├── older.py .............. match + walrus. parses fine at 3.10. the CONTROL.
├── proj-bare/ ............ no config at all      → expect the fallback
├── proj-requires39/ ...... requires-python=">=3.9" → expect 3.9
└── proj-tytoml313/ ....... ty.toml python-version="3.13" → expect 3.13
```

The three `proj-*` directories each contain the same `app.py` (a copy of
`pep695.py`). Same bytes, three configurations, three different outcomes. That is
the whole exercise in one sentence.

`older.py` is the control, and it makes a point about test corpora: it uses
version-gated syntax, it parses fine at the 3.10 default, and therefore **a
corpus of files like it can never reveal a version bug**. Most real code is like
`older.py`.

---

## Build it

### Step 1 — report the version and its source

Extend your exercise-00 binary into `pylspt-dev version`, matching the output
sketched in `plan/04-build/00-dev-cli.md`:

```
python_version = 3.9   (source: pyproject.toml requires-python ">=3.9")
search_paths   = [...]
```

The API:

```rust
use ty_python_semantic::Db as _;
db.program_file(file).python_version(&db)        // -> PythonVersion
db.python_version_with_source(file)              // -> &PythonVersionWithSource
```

`PythonVersionWithSource` has public `version` and `source` fields **[verified,
`ty_site_packages`]**, and `PythonVersionSource` is an enum with variants
including `ConfigFile`, `PyvenvCfgFile`, `InstallationDirectoryLayout`, `Cli`
and a default. You do not have that crate as a direct dependency — **you do not
need it.** You can call methods and read fields on a type you cannot name. Try
it; that is a genuinely useful Rust fact.

Run it against all three `proj-*` directories. **Predict all three first.**

### Step 2 — the warning line

`plan/04-build/00-dev-cli.md` asks for this line, and it is worth writing:

```
⚠ running interpreter is 3.12 — jedi would have used 3.12
```

Get the running interpreter's version by shelling out to `python3 --version`
(or reading it from your environment). Print the warning whenever it differs
from what ty resolved.

That single line turns your most likely parity surprise into something you see
on day one instead of during a bug report.

### Step 3 — watch a file break

Run your exercise-02 node scanner over `proj-requires39/app.py`, which resolves
to 3.9.

Predict the node count. Then run it. Then run the same file through
`proj-tytoml313/` (3.13).

Now look at `Parsed::unsupported_syntax_errors()` for the 3.9 run. **[verified]**
that accessor exists. How many errors? What did the node tree do — vanish
entirely, or degrade partially?

This is the failure mode `plan/01-crates/03` describes as "miserable to debug six
months later": *some files silently produce no nodes on one machine*. You have
now seen it on purpose, which is much cheaper.

### Step 4 — the f-string range experiment

The important one, because nothing errors.

Take `fstrings.py`. Parse the **same source text** twice, standalone, at two
targets:

```rust
let opts_310 = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY310);
let opts_312 = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY312);
let a = parse_unchecked(source, opts_310);
let b = parse_unchecked(source, opts_312);
```

For the `plain` function — a perfectly ordinary `f"value={build()} for {name}"`
— extract the `ExprCall` for `build()` and print its range under both. Compare.

Then do the same for `deep`, which has a call nested in another call's arguments
inside an f-string.

Write down what you find. Whatever the answer is, you now know it empirically
rather than by inference from a PEP, and you know which fixture to keep in your
parity suite forever.

### Step 5 — decide the parse policy, and write it down

`plan/01-crates/03` recommends a **version floor**:

```rust
// pylspt: parse as permissively as possible. The version only affects
// UnsupportedSyntaxError reporting (which we discard) and f-string tokenisation.
let parse_version = std::cmp::max(
    db.program_file(file).python_version(db),
    PythonVersion::PY312,     // floor: get PEP 701 f-string tokenisation
);
```

Implement it, or decide against it — but **write the decision down in a comment
in your source**, with the reason. This is the kind of choice that looks
arbitrary to whoever reads the code next, including you.

⚠ Note the constraint that makes this awkward: rule 1 from the plan says *never
parse a project file yourself* — go through `parsed_module(db, file)`, which is
version-wired and cached, and whose nodes the semantic layer will accept. There
is a test named `rejects_module_parsed_for_different_python_version`
**[verified, `ty_python_core/src/ast_node_ref.rs:155`]** that enforces exactly
this.

So the floor cannot be applied by parsing differently. It has to be applied by
**configuring the database differently** — which means the honest version of
this decision is "what python-version do I tell ty to use", not "how do I
parse". Work out where that setting lives (`ProjectMetadata`, before you
construct the db) and note it. Getting this ordering right is the difference
between a policy you can implement and one you cannot.

### Step 6 — log it at startup

One line, every time you initialise:

```
pylspt: project=/x/y python_version=3.12 (source: pyproject requires-python)
```

Free, and it answers the first question of every future parity investigation.

---

## Traps

- **`parse_module(src)` in a test.** It targets 3.10 and will fail on the PEP
  695 fixtures for reasons that look like your bug.
- **Assuming an error means no tree.** Ruff's parser is error-recovering: you
  usually get a partial tree *and* errors. Check what you actually got before
  concluding the file is unparseable.
- **Discarding `unsupported_syntax_errors()` silently.** Discarding them is
  correct policy for your driver; discarding them *without logging* is how you
  lose a day.
- **Testing only on ASCII, 3.9-compatible code.** See `older.py`. A corpus that
  cannot fail is not a test.
- **Believing the version is per-project.** It is resolved **per file** — that
  is why the API takes a `File`. A file in a subdirectory with its own config
  can differ.

---

## Done when

- [ ] `pylspt-dev version` prints version + source for all three `proj-*` dirs
- [ ] you predicted all three before running, and know which one you got wrong
- [ ] you have seen `pep695.py` degrade at 3.9, and counted the syntax errors
- [ ] you have compared `build()`'s range in `fstrings.py` at 3.10 vs 3.12
- [ ] the permissive-parse decision is written down, with where it must be applied
- [ ] the startup log line exists

---

→ [`exam.md`](exam.md), then [`../05-modules-and-imports/README.md`](../05-modules-and-imports/README.md)
