# Exam 08 — Classes, MRO and attributes

---

## Recall

**1.** What does `mro_resolver.py` do per class today, and why is that expensive
beyond the obvious?

**2.** `service._get_name_column` exists only to feed Jedi a position. Name two
reasons it can produce the wrong answer, and say why it disappears in the port.

**3.** What does ty's `mro.rs` handle that a naive "walk the bases" would get
wrong? Name three.

---

## The visibility wall, and the way through it

**4.** `plan/02-mapping/03` gives you a snippet calling `literal.iter_mro(...)`
and marks the accessor name `[check]`. What is actually wrong with it, and why
would checking the *name* not have found the problem?

**5.** List what is private here, with visibilities: `Mro`, `iter_mro` on
`ClassLiteral`, `iter_mro` on `ClassType`, `explicit_bases`.

**6.** What is the public route to base classes, and what exactly does it
return? Name the three surprises in its behaviour.

---

## DAG vs MRO

**7.** For `Diamond(Left, Right)` where both inherit `Base`, write out:

- the C3 linearisation
- what a naive depth-first recursion of direct bases produces

Then say what is wrong with the second, in two specific ways.

**8.** You had three options: implement C3, emit the deduped DAG, or fork to
reach `iter_mro`. The answer is the fork. Give the argument that is **not**
about MRO correctness — the one about the next component after this one.

**8b.** The wrapper file lives at `types/vnoc.rs` and not at `types/../vnoc.rs`
or in a new crate. Give the language rule that forces it, and the rebase
argument that makes "one added file plus one `pub mod` line" cheaper than
"flip `pub(crate)` to `pub` where needed".

**8c.** `class_mro` returns `Vec<Option<Definition>>`. Why not
`Vec<ClassBase>`, which is what `iter_mro` actually yields? And why `Option`
rather than skipping the bases ty cannot resolve?

**8d.** Name the three rules that keep a fork cheap to rebase, and for each say
what breaks if you ignore it.

**9.** Is `base_classes` order actually observable downstream? How would you
find out without asking anyone?

---

## Naming

**10.** Fill this in from your own output:

| case | Jedi | yours | match? |
|---|---|---|---|
| `object` | `builtins.object` | | |
| `Outer.Inner` | `hierarchy.Outer.Inner` | | |
| `IntBox`'s base | `hierarchy.Box` | | |
| `Runner`'s base | `typing.Protocol` | | |

**11.** `TypeHierarchyClass` gives you `name`, `file` and two ranges. Why is
that not enough for a nested class, and what do you do about it?

**12.** `Box(Generic[T])` and `IntBox(Box[int])` — what `Type` variant does each
base come back as? What does Jedi report, and what do you have to do to match?

---

## Attributes

**13.** For each expression in `attributes.py`, say whether ty resolves it to a
single method, and why:

- `self.fallback.handle(payload)`
- `self.default.handle(payload)`
- `self.handler.handle(payload)`

**14.** `dispatch` is harder than exercise 07's `emit`. State the extra
difficulty in one sentence. (What changed between "a parameter" and "an
attribute"?)

**15.** Write the three sentences describing what resolving `dispatch`
per-path would require. Then read `plan/03-call-tree/07` and note anything you
missed.

**16.** `static_member_type_for_attribute(model, &ExprAttribute)` takes syntax.
For `self.handler.handle`, what does it use as the receiver's type, and why is
that the wrong receiver for your call tree?

**16b.** `vnoc::member(db, env, ty, name)` takes a type instead. Does that solve
`dispatch`? Say precisely what it does and does not remove from
`plan/03-call-tree/07`'s workload.

---

## Practical

**17.** Compare your `Diamond` output against `python3 -c "... __mro__"`. If
they differ, is that a bug in your code or a difference between "MRO" and "what
I computed"? Say which, and what you would change.
