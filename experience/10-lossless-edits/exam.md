# Exam 10 — Lossless edits and ID injection

---

## Recall

**1.** Why can you not use ruff's AST to inject a docstring? Answer in terms of
a property of the tree, not "it does not have a printer".

**2.** Ruff itself depends on `libcst`. What does it use it for, and why is that
reassuring for your design?

**3.** Describe the detection/modification split, and say what fraction of
`parse_file` calls should reach libcst on a warm project.

**4.** Why is detection free?

**5.** What does `default-features = false` on `libcst` avoid?

---

## The salsa bug

**6.** Write the three-step sequence for a write, and say what breaks if you omit
step 3. Describe the symptom precisely.

**7.** Why must the sync be **batched** rather than done per file?

**8.** Design a test that fails if someone removes the sync. What must it do
that a naive test does not?

---

## Formatting

**9.** What do `CodegenState::default_newline` and `default_indent` apply to?
What do they *not* apply to?

**10.** Where do you get the right values, and what goes wrong on `crlf.py` and
`tabs.py` if you use constants instead?

**11.** `def stub(): ...` — why does "insert a line after the colon" break here,
and what does libcst let you do instead?

---

## Parity

**12.** Name the two pre-existing bugs. For each: what input triggers it, what
the wrong output is, and why you are reproducing rather than fixing it.

**13.** `raw_prefixed` in `has_docstring.py` contains `C:\new\table`. Spell out
exactly what changes when the `r` prefix is dropped, and why that is a real
problem rather than a cosmetic one.

**14.** `read_or_inject_folder_id` creates `__init__.py` when it is missing. Is
that a side effect you would design today? Does that matter?

**15.** On error, `file_folder_ids.py:22` returns a fresh random UUID with
`modified=false` — an ID that is never written anywhere. What could go wrong
downstream, and why do you reproduce it anyway?

---

## Practical

**16.** Run injection twice over all six fixtures. Report: edits made on run one,
edits on run two, libcst invocations on each. Then remove one fixture's ID by
hand and re-run — do the counts move the way you expect?

**17.** Your `parse_file` RPC writes to the user's source files as a side effect
of a *read* operation. Write the two-sentence comment you would put at the top of
that function so the next reader is not surprised. Then decide whether
`--write`-off-by-default belongs in the RPC too, not just the CLI, and justify
it.
