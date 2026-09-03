# Answers 04 — The Python version

---

**1.**

1. Explicit override (`--python-version`, or the setting you pass in)
2. `[tool.ty.environment] python-version` in `pyproject.toml`, or `[environment]`
   in `ty.toml`
3. `requires-python` in `pyproject.toml` — **the lower bound**
4. The resolved Python environment (`.venv`, uv workspace) — its interpreter version
5. Fallback: `PythonVersion::latest_ty()` == **3.14** **[verified]**

**2.** Jedi uses `InterpreterEnvironment()`: the *running* interpreter's
`sys.version_info`.

The most likely disagreement: a project with `requires-python = ">=3.8"` (very
common — libraries declare broad support) analysed by a driver running on 3.12.
Jedi says 3.12, ty says 3.8. Every 3.9+ construct in the code then parses under
Jedi and errors under ty.

**3.** `default()` is **PY310**; `latest_ty()` is **PY314** **[verified,
`python_version.rs:59-76, 100`]**. Four releases apart.

Dangerous because `Default` is what you get *implicitly* — from
`ParseOptions::from(...)`, from `..Default::default()`, from any API that does
not ask you. So the version you get depends on which entry point you happened to
call, and nothing in the signature tells you.

**4.**

```rust
parse_module(source)                                   // ⚠ 3.10
parse(source, ParseOptions::from(Mode::Module))        // ⚠ 3.10
parse_unchecked_source(source, PySourceType::Python)   // ⚠ 3.10
```

Fix: `.with_target_version(version)` — the only public setter **[verified]**.

**5.** `target_version` is `pub(crate)` **[verified,
`ruff_python_parser/src/parser/options.rs:26`]**. You cannot name it from
outside the crate, so the builder is the only route. Arguably this is the API
protecting you: you must make an explicit call to change it, so the mistake is
at least greppable.

---

**6.**

| construct | at 3.10 |
|---|---|
| `type Alias = int \| str` | `UnsupportedSyntaxError` (PEP 695, 3.12) |
| `def f[T](x: T) -> T` | `UnsupportedSyntaxError` (PEP 695, 3.12) |
| `match command:` | **fine** — `match` landed in 3.10 |
| `f"outer {f"inner"}"` | `UnsupportedSyntaxError` (PEP 701, 3.12) |
| `f"value={build()}"` | **parses fine, but may tokenise differently** |

The last one is on the list precisely because it looks safe. PEP 701 rewrote
f-string tokenisation, so the token boundaries *inside* the braces can differ
between targets even for f-strings that are legal in both. Your `position` and
`call_col_pos` are derived from ranges, so a difference there is a difference in
your output — with no error, no warning, and no failing test unless you wrote
one.

**7.** PEP 695 fails **loudly**: you get `UnsupportedSyntaxError`, a degraded or
empty tree, and something visibly wrong. You will find it in an afternoon.

PEP 701 fails **quietly**: everything parses, every node exists, and a handful
of columns are off by a few characters inside f-strings. Your driver reports
them confidently. Nothing distinguishes it from a correct run except a
byte-level diff against the Python driver.

The general rule this teaches: rank version risks by how they fail, not by how
big the feature is.

**8.** ty uses **3.9**.

*Correct*, because that is what a type checker owes the user — the project
declared it supports 3.9, so code must be checked against 3.9, and 3.10+ syntax
in it is a genuine bug.

*Wrong for you*, because you are not checking anything. You are building a
structural map of code that demonstrably runs on 3.12. Reporting "this file has
syntax errors" for code that works is not a useful answer to any question v-noc
asks.

Same number, two jobs, opposite verdicts. That is why the policy decision in
step 5 exists at all.

---

**9.**

| project | version | source |
|---|---|---|
| `proj-bare/` | **3.14** | fallback (`latest_ty()`) — unless a `.venv` or interpreter is discovered nearby, in which case that wins |
| `proj-requires39/` | **3.9** | `requires-python` lower bound |
| `proj-tytoml313/` | **3.13** | `ty.toml` `[environment] python-version` |

If `proj-bare` gave you something other than 3.14, discovery found an
environment — that is worth investigating, because it means "no config" does not
mean "no input", and your fixtures are not as isolated as they look.

**10.** `proj-tytoml313/app.py` at 3.13 parses cleanly: full node tree, zero
unsupported-syntax errors.

`proj-requires39/app.py` at 3.9 produces several `UnsupportedSyntaxError`s
(`type Alias`, `def generic_fn[T]`, `class Box[T]`) and a **partial** tree —
ruff's parser is error-recovering, so you still get most nodes back.

That partial recovery is a mixed blessing. It is why the plan insists your
driver "must never hard-fail on a broken file", and also why the failure is
subtle: you get *some* nodes, so nothing looks broken; you just get fewer than
Jedi did.

**11.** Report what you measured. Two outcomes, both informative:

- **Identical ranges** — for this particular construct the two tokenisers agree.
  That does *not* mean PEP 701 is irrelevant: check `deep` (a call nested inside
  another call's arguments, inside an f-string) and `multiline_expression`,
  which is 3.12-only syntax and therefore cannot even be compared.
- **Different ranges** — you have found the silent bug in your own fixture, and
  it goes into the parity suite permanently.

The reasoning to take away: a single passing comparison generalises to nothing
here. The claim you can defend is "these constructs agree at these two
versions", and it is only worth anything for constructs you actually tested.

---

**12.**

*For:* your job is to see the code that exists, and the highest version parses
the most code — 3.12+ additionally gives you the modern f-string tokenisation,
which is what your position numbers depend on.

*Against:* you would then diverge from ty's own view of the project, so a file
that ty considers erroneous is one you happily analyse — and any inference you
later ask ty for is computed under *ty's* version, not yours. Two versions in
one pipeline is its own kind of confusion.

**13.** The floor must be applied to **the database's configuration**, not to a
parse call — you set the python-version in `ProjectMetadata` *before*
constructing the `ProjectDatabase`, so that `parsed_module` (which is what you
must use) is wired to the version you chose.

The consequence is a scheduling one: **it is a startup decision, not a per-file
one.** You cannot decide "parse this file permissively" halfway through a
request. Discovering that at exercise 11, after building around the assumption
that you can pick a version per parse, means rewriting your initialisation path.

**14.** It protects the invariant that a semantic node reference belongs to the
same parse the semantic layer is reasoning about. `AstNodeRef` is keyed on
`(file, python_version)` **[verified]**, so nodes from a differently-versioned
parse are rejected rather than silently mixed.

Without it, you could hand ty a node from your own 3.12 parse while its semantic
index was built from a 3.9 parse. Ranges would not correspond, and you would get
wrong types attributed to the wrong expressions — the worst failure mode
available, because everything still "works".

**15.** *Correct to discard:* you are not a linter. A syntax error your parser
recovered from does not stop you from producing a useful structural map, and
your existing driver swallows failures everywhere (quirk 13). Returning errors
to v-noc for files it can already open is a regression.

*Must still log:* it is the only signal distinguishing "this file has no
functions" from "this file's functions did not parse". Without the log, the
version misconfiguration in answer 8 is invisible.

---

**16.**

```
pylspt: project=/…/proj-requires39 python_version=3.9 (source: pyproject requires-python ">=3.9")
⚠ running interpreter is 3.12 — jedi would have used 3.12
```

The second line should fire on startup, once, not per file — and it should say
what Jedi *would* have done, because the reader's mental model is the old
driver.

**17.** A file using PEP 695 syntax, asserted on the **node count** (or on the
presence of a specific function's node), not on "did it parse". A
`parse_module(source)` regression targets 3.10, so `def generic_fn[T]` fails and
the node disappears — an assertion on the node's existence fails loudly, while
an assertion on "no error was returned" passes, because ruff recovers.

Pick assertions by asking which wrong implementation they reject. That is the
same principle as exercise 01's answer 17, and it is the single most
transferable habit in this folder.
