# Exam 00 — Setup

Write your answers down before opening `answers.md`. Answers you only *thought*
do not count; the gap between what you meant and what you can state is the part
worth finding.

---

## Recall

**1.** Why can you not write `cargo add ty_project` and be done?

**2.** `ruff_python_ast` *is* on crates.io. Why do you still take it from git at
the same revision as the rest?

**3.** What breaks if two of your `git` dependency lines carry different `rev`
values? Describe the error message you would see, not just "it breaks".

**4.** You did not put `salsa` in your `Cargo.toml`, yet your program constructs
a salsa database and runs salsa queries. How?

**5.** What does `panic = "abort"` do to a salsa-based program, and why is the
symptom hard to diagnose?

---

## The feature flag

**6.** Without `features = ["os"]` on `ruff_db`, which import fails, and what
exactly is the compiler complaining about? (Name the file and the `#[cfg]` if
you can.)

**7.** ty's own CLI asks for `["os", "cache", "junit"]`. You ask for `["os"]`
only. Under what circumstance would that difference bite you, and why does it
not bite you today?

**8.** The plan tells you to write `default-features = false` on `ty_project`.
You did not. What is the actual consequence of each choice — and is either one
*wrong*?

---

## The one that costs you an afternoon

**9.** `plan/04-build/01-wiring-cargo.md` contains a smoke test that does not
compile at `ac201b8`. Without looking: what is the broken line, and what is the
route that does work?

**10.** The plan marks that snippet as coming from a `[verified]` reading of the
source. It is still wrong. What does that tell you about how to use the plan for
the remaining eleven exercises?

---

## Predict, then check

**11.** Run your binary against a directory containing **only** a `main.py` — no
`pyproject.toml`, no `.venv`, no `ty.toml`. Predict the version it prints.
Then run it.

**12.** Now add a `pyproject.toml` containing exactly:

```toml
[project]
name = "demo"
requires-python = ">=3.9"
```

Predict the version. Then run it.

**13.** You are running on a machine whose interpreter is Python 3.12. Jedi
would report 3.12 for both of the cases above. ty reports something else for at
least one. **Which behaviour is correct for a type checker, and which is correct
for your driver?** These are not the same answer — that is the point.

---

## Practical

**14.** Make `cargo tree -d` output part of your workflow. Run it. Paste the
result into your notes. If it is empty, say in one sentence why that is the
outcome you wanted.

**15.** Break your build on purpose: change `rev` on the `ty_ide` line only, to
any other commit hash from ruff's history. Run `cargo check`. Read the error.
Restore it. Write down what the error looked like — you want to recognise it
instantly six months from now.

*(#15 costs you one rebuild. It is the cheapest possible way to buy that
recognition, and this class of error is genuinely confusing when it arrives
unannounced.)*
