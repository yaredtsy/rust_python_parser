# Exam 09 — The IDE layer

---

## Reading

**1.** Write the two-line prologue that `outgoing_calls` starts with, from
memory.

**2.** How does `outgoing_calls` turn an offset into "the function I am asking
about"? Name the function it uses.

**3.** `OutgoingCall.from_ranges` is a `Vec<TextRange>`. What design decision
does that encode, and which of your own quirks is it nearly identical to?

**4.** Name three things `outgoing_calls` deliberately does not do that your
call tree must.

**5.** `CallHierarchyItem.selection_range` is documented as "the stateless key
when the LSP client re-sends this item". What does that tell you about how the
LSP call hierarchy is meant to be driven, and why that shape does not fit your
RPC?

---

## The comparison

**6.** For each entry point, give the reason `outgoing_calls`' answer differs
from what your tree needs. Six different reasons:

| entry | reason |
|---|---|
| `twice` | |
| `recurse` | |
| `calls_undocumented` | |
| `diamond` | |
| `constructs` | |
| `emit` | |

**7.** `diamond` reaches `leaf` through `left` and through `right`. How many
`leaf` nodes does your tree contain, and how many would a call *graph* contain?
Which quirk is this, and why was it chosen?

**8.** `recurse` calls itself. Explain why the ancestor guard (quirk 5) is not
the same as a global visited set, and give an input where the two produce
different trees.

---

## The gap

**9.** For `writer.write(data)` inside `emit`, how many `OutgoingCall`s came
back? Is any of them wrong?

**10.** Is there an argument you could pass to `outgoing_calls` that would give
you only the one you need? Answer yes or no, and give the reason in terms of
*what question the function answers*.

**11.** Suppose you had the ty source in your own workspace and could edit it
freely. Sketch what you would have to change to make `outgoing_calls`
path-sensitive — and say what that would do to salsa's cache.

**12.** After doing steps 1–4, read `plan/03-call-tree/02`. What did it say that
you had not already worked out? What had you worked out that it does not say?

---

## Reuse

**13.** List what you will take from `outgoing_calls.rs` and what you will
build yourself. Be specific — name functions, not themes.

**14.** Compare `document_symbols`' output to your exercise-02 node tree for
`tree.py`. Name one thing it includes that you drop, and one thing you include
that it does not.

**15.** `incoming_calls` answers "who calls me". Your driver never asks that.
Name a v-noc feature that would want it, and say whether it could be served by
your call tree instead.

---

## Practical

**16.** Time `outgoing_calls` on a file, cold and warm. Compare to the timings
you took in exercise 03. Is call resolution dominated by parsing, by inference,
or by the traversal?

**17.** Write the function signature for *your* recursive call-tree entry point
— the one `plan/03-call-tree/03` will make you implement. Compare it to
`outgoing_calls`' signature and list every parameter that differs. That diff is
the project.
