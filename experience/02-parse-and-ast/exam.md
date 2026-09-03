# Exam 02 — Parse and AST

---

## Recall

**1.** Ruff's AST and parso's CST differ in one property that decides which one
your ID injector must use. Name the property and the consequence.

**2.** `x=1` and `x = 1` produce identical ruff ASTs. Name two things you can
still recover about the original text, and one you cannot.

**3.** Why does `name` for a call node come from slicing the source rather than
from a field on the node?

**4.** What does `TraversalSignal::Skip` do, and which line of `parser.py` does
it replace?

---

## Call chains

**5.** For `build().render().strip()`, draw the ruff AST. Then list the three
calls with their `call_index`, and say which parts of their `position` are
shared and which differ.

**6.** `call_col_pos` is the column of `(`. Write the single expression that
gives you the byte offset of that `(`, and quote the documentation sentence that
guarantees it.

**7.** Which three expression kinds must `flatten_call_chain` descend through to
find the inner call? Give the `calls.py` function that tests each.

**8.** `wrap(build(), key=build())` — how many `CallNode`s does this produce,
and what is the `call_index` of each? Careful: this is not the same question as
"how many calls are in the chain".

---

## The traps

**9.** Where does `StmtFunctionDef.range` start for a decorated function — at
`@` or at `def`? Where does parso start? What does the difference do to your
output, and how common is it in real code?

**10.** `parser.py:126` has a special case for `async def` positions. What
happens to that special case in the ruff port, and why?

**11.** In `edges.py`, `has_lambda` contains `lambda x: log(x)`. How many nodes
should appear in your output for that function body, and why is the answer not
"one call node for `log`"?

**12.** `default_args(x=build(), *, y=log())` — those two calls are in the
*signature*, not the body. Where do they land in your tree? Where does parso put
them? (Answer from your own output; then say whether you think it is right.)

---

## Docstrings

**13.** For each of these, say whether your extractor should return an ID, and
why:

- `not_first_statement`
- `implicit_concat`
- `raw`
- `single_quotes`
- `id_like_text`

**14.** Your Python slices `[3:-3]` to unquote a docstring. Name two inputs
where that is wrong and `.value.to_str()` is right.

**15.** Quirk 4 says a callee with no `ID:` is dropped from the call tree
entirely. But `parse_file` still emits a node for it with `id: null`. Why are
these different, and what goes wrong if you apply the call-tree rule in
`parse_file`?

---

## Practical

**16.** Run your scanner over all five fixture files and count nodes by kind.
Write the counts down. Then change one thing — remove the lambda skip — and see
which count moves. That is your regression test for quirk 8.

**17.** Pick the quirk from 8–12 that your implementation currently gets wrong
(there is usually one). Write the smallest Python file that demonstrates it, and
put it in your fixture directory with an expected-output file.
