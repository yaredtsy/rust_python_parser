# Exam 05 — Modules and imports

---

## Recall

**1.** Jedi resolves imports per `Script`. ty resolves them as a query. Name the
consequence for a project of 500 files where the editor sends 50 requests.

**2.** What is a `ResolverFile`, and why does module resolution key on it rather
than on `File`?

**3.** `Module::file()` returns `Option<File>`. Give two situations where it is
`None`.

**4.** Where does `import json` resolve to? Is that file on your disk?

**5.** Name the three `SearchPath` predicates and what each one means for your
call tree.

---

## The filter

**6.** Quote `call_resolver.py`'s effective rule for "is this project code",
after accounting for the unreachable branches. Why does the dead code matter for
the port?

**7.** Name three inputs where ty's `is_first_party()` and Jedi's path-prefix
test would disagree. For each, say which answer you would rather have, and which
one you must ship.

**8.** Builtins are skipped **by name, before inference**. Give the observable
behaviour that would change if you skipped them after inference instead.

**9.** A project defines its own function called `list`. What does today's
driver do? Is that a bug? What does your port do?

---

## Predict, then run

**10.** For each file, predict the module name, then check:

| file | predicted | actual |
|---|---|---|
| `src/pkg/__init__.py` | | |
| `src/pkg/core.py` | | |
| `src/pkg/sub/deep.py` | | |

**11.** Comment out `[tool.ty.environment] root = ["src"]` and re-run step 1.
What changed, and what does that tell you about where "first-party" comes from?

**12.** Resolve every import in `core.py`. Which ones resolve to a file you can
open in your editor, and which do not? Report `definitely_not_a_real_module_xyz`
— what exactly comes back, and did anything panic?

**13.** In `entry.py`'s `main`, there are six calls. Which does your call tree
descend into, which does it skip, and for each skip, which rule fired
(builtin-by-name / not-project-code / no-ID)?

---

## Thinking ahead

**14.** `descend` is reachable by three module paths. If your call tree used the
*importing* path as identity, what would break? Give a concrete example with two
call sites.

**15.** `pkg.load` is defined in `pkg/core.py` and re-exported from
`pkg/__init__.py`. Predict the qualified name Jedi's
`get_qualified_names(True)` produces. Say which of the two files you expect to
see in it, and why.

**16.** An editable install puts a `.pth` file in `site-packages` pointing at
your source tree. A callee resolves to a path inside `site-packages` **and**
inside your project. What does the prefix test say? What does `is_first_party`
say? Which is right?

---

## Practical

**17.** Add the disagreement log from step 3 to your code, run it over a real
project (not the fixture), and report how many disagreements you got. If it is
zero, say what that does and does not prove.
