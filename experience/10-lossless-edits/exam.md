# Exam 10 — Lossless edits and ID injection

Each object has its own exam. This one is the whole exercise, and the
cross-object questions are the ones worth getting right.

---

## The shared rule

**1.** Name the one rule injection and collection must share, the function that
owns it, and the two callers.

**2.** Collection looks inside `if TYPE_CHECKING:` and injection does not.
Describe what a v-noc user sees. Now swap the two and describe that. Which is
dangerous, and why is "it errors" not the answer to either?

**3.** `shared_rule.py` has sixteen defs and several never execute. Why is that
irrelevant to both passes?

**4.** `id_from_body` is the *other* half of the shared rule. Give a concrete
input where a stricter injector-side check would corrupt the graph.

**5.** Why is a statement-only walk complete for ID injection? Name Python's one
expression-form definition and the one-line proof that it cannot hold a key.

---

## Where the key goes

**6.** `"""a""" """b"""` is one docstring. Which part do you measure the insert
offset from, and what does the other choice produce?

**7.** Why `closer_len()` and not `quote_len()`? Name the single input on which
they differ.

**8.** Inserting immediately before a closing quote cannot corrupt an escape.
Why not? Name the input you would expect to break it and what Python says about
it.

**9.** `shapes.py:single_quoted` gets a different inserted string from every
other keyed def. What, and why?

**10.** Why append the key at the **end** of a docstring rather than the start?
Name the fixture where the difference is visible and say what goes wrong.

**11.** `def stub(): ...` — write the exact inserted text and say where the
original `...` ends up.

**12.** Name two inputs for which `doc_slot` returns `None`. What does the
injector do about each, and which quirk is that?

---

## Plan and apply

**13.** Why does `plan_ids` return `Vec<Edit>` instead of applying as it goes?
Give the answer in terms of borrows, not style.

**14.** Edits are applied in descending offset order. State the invariant that
preserves, and the thing it gives you for free about nested defs.

**15.** Can two edits share an offset? Prove it, and say what the
`debug_assert` is actually testing.

**16.** The builder form of `apply` needs no ordering rule. Why not — and why
learn the rule anyway?

**17.** `new_id` is a parameter. Give the testing reason and the purity reason,
and say what a fixture diff looks like without it.

**18.** `tabs.py` gets a tab and `crlf.py` gets `\r\n`, with no style detection
code. What supplies them?

---

## Purity and the order

**19.** Give four reasons injection cannot be a `#[salsa::tracked]` function.
Rank them by how badly each one bites.

**20.** A random UUID inside a tracked query does more than produce odd IDs.
What does it do downstream, and which salsa mechanism is responsible?

**21.** Salsa can cancel a query mid-flight. Name the mechanism and say why it
rules out file writes in particular.

**22.** Write the four steps in order. What does the last one guarantee that no
amount of care in the third could?

**23.** You skip `File::sync_path`. What is broken, and why is it hard to
notice?

**24.** The stale-range rule is a compile error, not a convention. Name the two
signatures that make it one, and the loophole that gets around it.

**25.** Why batch the writes and sync once rather than per file?

---

## Parity and the wire

**26.** `plan/02-mapping/02` says to reproduce two pre-existing bugs. Name them,
say what input triggers each, and say why this exercise does **not** reproduce
them.

**27.** `raw_prefixed` contains `C:\new\table`. Spell out what the Python driver
does to that string and why it is a behaviour change, not a formatting one.

**28.** Which two fixtures can no longer be byte-compared against the Python
driver, and what do you do about the other seven?

**29.** `read_or_inject_folder_id` creates `__init__.py` when it is missing.
Which sync function do you need afterwards, and why that one?

**30.** A module has a docstring and a key but is not a def. Which of the four
objects still applies to it, and which does not?

**31.** The four-step order means you *could* inject before answering, so v-noc
would never see `id: null`. Give the argument for and the argument against, and
say what you would look at before deciding.

---

## Practical

**32.** Inject into all ten fixtures twice. Report: edits on run one, edits on
run two, and for each fixture whether it still imports. Any non-zero in the
second column is a reader/writer mismatch — find it.

**33.** `idempotent.py` takes exactly two edits. Name all five defs, what the
reader finds in each, and whether it is edited. Then say which row you disagree
with and what you would change.

**34.** Run the CLI over a directory with `--write` off and prove nothing
changed. Then point it outside the project with `--write` on and show the
refusal.

**35.** Pick the object you are least confident in and write the smallest Python
file that would catch you getting it wrong. Add it to `python/` with its
expected edit plan.
