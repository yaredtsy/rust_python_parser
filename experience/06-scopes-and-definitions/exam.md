# Exam 06 — Scopes and definitions

---

## Recall

**1.** Jedi creates a `Context` per query, from a position. ty builds a
`SemanticIndex` per file, once. Name one thing each approach makes easy that the
other makes hard.

**2.** List the seven `ScopeKind` variants. Which two would a Jedi user not
expect to be scopes at all?

**3.** What is in a use-def map, and what question does it answer that a plain
"list of definitions in this scope" cannot?

**4.** What is a `Definition`? Give four different syntactic constructs that
create one.

**5.** Why must you not keep `Definition<'db>` in a long-lived struct, and what
should you keep instead?

---

## Scopes

**6.** Draw the scope tree of `outer()` in `scopes.py`. How many child scopes
does it have?

**7.** `Service.registry` is defined in the class body. Can `run` see it as a
bare name `registry`? What does that tell you about class scopes and lookup?

**8.** `[x * x for x in range(seed)]` — which scope is `x` defined in? Which
scope is `seed` read from? Why is that not the same answer?

**9.** Does a `TypeParams` scope appear between `generic` and the module scope?
What must your qualified-name builder do about it — and how did you find out?

---

## Qualified names

**10.** Give the qualified name your builder produces for each, and say which
one is the parity risk from `plan/02-mapping/03`:

- `outer`
- `inner`
- `Service.run`
- `Service.Nested.deep`
- the lambda in `outer`

**11.** `pkg.load` is defined in `pkg/core.py` and re-exported from
`pkg/__init__.py`. What does Jedi's `get_qualified_names(True)` produce? What
does yours? If they differ, which is wrong and why?

**12.** Quirk 6 dedupes children by `target_qname` and increments `call_count`.
Explain what goes wrong if the qualified name follows the import route rather
than the definition site. Use two call sites in your example.

---

## Flow

**13.** In `shadowing()`, how many definitions of `value` exist in the scope?
How many reach the `return`? Answer from your own output, then explain the gap.

**14.** For `self.dispatch(payload)` inside `run`, what did resolution give you?
Was it a single definition, several, or nothing? What does the answer suggest
about how attributes differ from plain names?

---

## The API surprise

**15.** `plan/02-mapping/01` claims `docstring_from_body` is `pub(crate)` and
reachable only under Option A. Check it. What did you find, and what does that
change about your exercise-02 code?

**16.** `Definition::docstring(db)` returns `Option<String>`. Your own extractor
returns the raw docstring text so you can scan it for `ID:`. Are these
interchangeable? Name one path where you want yours and one where you want ty's.

---

## Practical

**17.** Write `qualified_name` as a function with a signature you would actually
ship, and list its failure modes: what inputs make it return something wrong or
nothing at all? You need this list for exercise 08, where MRO entries go through
the same code.
