# Exam 04 — The Python version

---

## Recall

**1.** Give the five sources ty consults for a Python version, in priority
order.

**2.** Where does Jedi get its version? Name the single most likely way the two
disagree on a real project.

**3.** What are `PythonVersion::default()` and `PythonVersion::latest_ty()`?
Why is it dangerous that they differ?

**4.** Name three parser entry points that silently target 3.10, and the one
method that fixes it.

**5.** Why can you not just write `ParseOptions { mode, target_version }` as a
struct literal?

---

## Consequences

**6.** For each, say what happens at a 3.10 target:

- `type Alias = int | str`
- `def f[T](x: T) -> T`
- `match command:`
- `f"outer {f"inner"}"`
- `f"value={build()}"`

The last one is the interesting entry. Explain why it is on this list at all.

**7.** Why is PEP 701 described as "insidious" while PEP 695 is merely
"breaking"? Answer in terms of what your driver observes.

**8.** `requires-python = ">=3.9"` on a project whose CI runs 3.12. Which
version does ty use, and why is that both correct and wrong at the same time?

---

## Predict, then run

**9.** For each fixture project, predict the version and the source, then run
`pylspt-dev version`:

| project | predicted version | predicted source | actual |
|---|---|---|---|
| `proj-bare/` | | | |
| `proj-requires39/` | | | |
| `proj-tytoml313/` | | | |

**10.** Run your node scanner over `proj-requires39/app.py` and
`proj-tytoml313/app.py` — the same bytes. Report both node counts and the
`unsupported_syntax_errors()` count for each. Did the 3.9 run produce *no* tree
or a *partial* tree? What does that tell you about ruff's parser?

**11.** Parse `fstrings.py` at 3.10 and 3.12 and compare the range of `build()`
inside `plain`. Report both ranges. If they are identical, does that mean PEP
701 does not matter for your driver? Justify your answer with a second fixture.

---

## Policy

**12.** The plan recommends `max(resolved_version, PY312)` as a parse floor.
State the argument for it in one sentence, and the argument against it in one
sentence.

**13.** Rule 1 says never parse a project file yourself — always go through
`parsed_module(db, file)`. Rule 3 says parse permissively with a version floor.
These pull in opposite directions. Where must the floor actually be applied, and
what does that mean for when you have to make the decision?

**14.** What is `rejects_module_parsed_for_different_python_version` protecting
against? What would go wrong if it did not exist and you parsed a file yourself
at a different version?

**15.** Your driver discards `unsupported_syntax_errors()`. Give the reason that
is correct policy, and the reason you must still log them.

---

## Practical

**16.** Write the startup log line your driver will emit, filled in for
`proj-requires39/`. Then write the one-line warning that fires when ty's
resolved version differs from the running interpreter.

**17.** Add one fixture to your parity suite that would fail if someone
"simplified" your parse call back to `parse_module(source)`. It must fail
loudly, not subtly.
