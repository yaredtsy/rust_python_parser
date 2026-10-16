# Exam 02 — Parse and AST

> ⚠ **Questions 5–8 and 12 are history questions now.** They describe the
> one-tree scanner: `flatten_call_chain`, `call_index`, `call_col_pos`, and a
> single traversal that collects defs and calls together. Object 5 part 4 §4
> drops the two numeric fields (one node per `ExprCall` makes the range the
> handle), and [file 8](08-staged-pipeline.md) stops collecting calls in the
> first traversal at all.
>
> Answer them anyway — knowing what parso counted is how you read the Python
> driver — but answer the **Staging** section at the end as well, and treat it
> as the one that describes what you ship.

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

---

## Staging

See [`08-staged-pipeline.md`](08-staged-pipeline.md); its own exam goes deeper.

**18.** Name the three stages, and for each one say what it is the *only* thing
that does.

**19.** Stage 1 uses `StatementVisitor`, which never enters an expression. Name
the one definition form that is an expression, and explain why ignoring it loses
no ID.

**20.** Stage 2 applies its inserts in descending offset order. What does that
buy, and what does it give you for free about a def and the defs nested inside
it?

**21.** Why must stage 2 end by re-running stage 1 rather than adjusting the
ranges it already has? Name the thing in Rust that stops you getting this wrong.

**22.** The ID index maps an ID to a **`Vec`** of locations. What real-world
action produces two locations for one ID, and which earlier bug in this exercise
is the single-valued version of?

**23.** Stage 3 never receives a list of calls. Say where each of these rules
from object 5 ends up: the lambda skip, the decorator override, and
"default arguments stay with the function".
