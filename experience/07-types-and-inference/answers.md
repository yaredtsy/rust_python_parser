# Answers 07 — Types and inference

---

**1.**

- **Jedi's `Value`:** *"what is this expression, on this path, given how we got
  here?"* — the output of a lazy interpreter that simulates execution.
- **ty's `Type`:** *"what could this expression be, in any possible run of the
  program?"* — a sound static description.

**2.**

- **Flow-sensitive** — the answer depends on *where in the code* you ask.
- **Context-sensitive** — the answer depends on *who called this function*.

ty is flow-sensitive and not context-sensitive. The separating example:

```python
def emit(writer, data):
    writer.write(data)     # ty: one answer for all callers
```

versus

```python
thing = 1
thing = "two"              # ty: a different answer on each line
```

**3.** Inference is cached per scope — `infer_scope_types(scope)`. The cache key
is the scope (plus the program environment), and that is what makes the whole
system incremental.

Context-sensitivity would require the key to include the **call-site
environment**: which values were bound to which parameters. That value is
effectively unbounded and almost never repeats, so every call would be a fresh
key. See answer 8 for why that is worse than useless.

**4.**

| variant | meaning for the call tree |
|---|---|
| `FunctionLiteral` | a function object — resolve to its `Definition`, descend |
| `BoundMethod` | a method plus its receiver — descend, and you know `self` |
| `ClassLiteral` | a constructor call — quirk 7: rewrite `target_id` to `ClassSchema/…`, descend into `__init__` |
| `NominalInstance` | an instance — the receiver for the next attribute lookup |
| `Union` | several candidates — fan-out, needs a cap |
| `Dynamic` | Unknown — fall back, count, do not fail |

**5.** It means ty could not determine the type — missing annotations, dynamic
constructs, or a genuine inference gap in a pre-1.0 checker. It is **normal and
frequent**.

Your interpreter must have a fallback path for every `Unknown` and must count
them. A driver that errors or drops a node on `Unknown` produces a tree that is
mysteriously missing subtrees on exactly the code that is hardest to read.

---

**6.** *Model answer.*

ty's answer is **correct and useless**, and both halves matter.

It is correct because `emit` genuinely is called with a `JsonWriter` in one
place and an `XmlWriter` in another, so across all runs of the program
`writer.write` really can be either method. A type checker that answered
`JsonWriter.write` would be unsound: it would miss a real type error in the XML
path.

It is useless for the call tree because the call tree is not a statement about
all runs. It is a statement about **one activation of one function reached by
one path**. Under `run_json → emit`, the writer *is* a `JsonWriter`, and the
child node must be `JsonWriter.write`. Emitting both would claim `run_json`
calls the XML writer, which is false.

To pick, I need an input ty's API never takes: the **environment** at this call
site — the binding `writer ↦ JsonWriter()` produced by the argument at
`run_json`'s call to `emit`. Given that environment, resolving `writer.write` is
straightforward: look up `writer`, get a chosen value, walk its MRO for `write`.
Without it, no function in ty's public surface can tell me, because the question
I am asking is not the question ty is answering.

**7.** Because frame identity is `(parent, qname)` and `add_child` dedupes by
`target_qname` (quirk 6). Two calls to `emit` **from the same function** merge
into a single child with `call_count = 1` — one node, one environment, and the
context-sensitivity has nowhere to show itself.

By calling from `run_json` and `run_xml` — two different parents — you get two
distinct frames, each with its own `emit` child, each with its own environment.
That is the shape where per-path resolution is observable in the output.

If both calls were in one function, the tree would show one `emit` node with
`call_count: 1`, and its `write` child would have to be *something* — which
raises the question of what an interpreter should do when one frame really does
see two different writers. (That is a design question, not a bug, and
`plan/03-call-tree/09` addresses it.)

**8.** The cache would grow without bound and hit almost never. Every distinct
call-site environment creates a new key, environments rarely repeat, and salsa
would retain them all along with their dependency edges.

Worse than no cache because you pay all the costs — memory, bookkeeping,
dependency tracking, invalidation checks — for none of the benefit. And it would
degrade the *existing* incrementality, since the entries are still edges in the
dependency graph that must be validated on every revision.

Hence the plan's rule for your own code: untracked entry points, tracked
primitives, and a memo table you control for anything keyed on an environment.

---

**9.** Report your own output; the shapes to expect:

| expression | expected shape |
|---|---|
| `value` before the guard | `Cache \| None` |
| `value` after the guard | `Cache` — narrowed |
| `thing` at the three lines | `Literal[1]`, `Literal["two"]`, `Cache` |
| `thing` at `branched`'s return | a `Union` of `Cache` and the module type of `json` |
| `param` in `unannotated` | `Dynamic` / Unknown |
| `n`, `s`, `b` | literal types — `Literal[42]`, `Literal["hello"]`, `Literal[True]` |
| `cache.get("k")` | the return type of `get` |

`branched` is the interesting one: a union whose members are of *different
kinds* — an instance and a module literal. Your bucketing code in step 3 must
handle a union whose members are not all callables.

**10.** ty gives you literal types where it can — `Literal[42]` — and widens to
`int` where it must (after a branch join, or through an unannotated parameter).

`Literal[42]` is more useful when the value decides control flow: a caller
passing `"json"` versus `"xml"` selects a branch, and a literal argument lets
you follow the right one. `int` is more useful when you are just deciding "is
this callable" and the precision would only cause fan-out.

Since your interpreter chooses its own abstract domain
(`plan/03-call-tree/04`), this is a real design input: literals are cheap
precision that ty hands you for free.

**11.** Because a call tree distinguishes **objects**, not classes. Two
`Cache()` expressions produce two different runtime objects; if the first is
stored in `self.a` and the second in `self.b`, then `self.a.get()` and
`self.b.get()` are calls on different objects that happen to share a type.

`Type` has no notion of "the same object as that one", so it cannot express the
distinction. That is why the plan recommends **Option C alongside Option A** —
your own `AbstractValue` domain carrying an object identity that ty's `Type`
does not have. `plan/03-call-tree/04` (value domain) and `/06` (attributes and
`self`) are the chapters.

---

**12.**

```rust
// what an interpreter needs — the receiver is an INPUT
fn member_of(db: &dyn Db, receiver: Type<'_>, name: &str) -> Option<Type<'_>>;

// what is public — the receiver is DERIVED from syntax
fn static_member_type_for_attribute(model: &SemanticModel, attr: &ast::ExprAttribute) -> Option<Type>;
```

The second cannot serve the first because it takes the *expression* `writer.write`
and infers the receiver itself, from the program text — so ty chooses the
receiver, and you cannot supply the one your path selected. The function that
would let you, `Type::static_member`, is private **[verified]**.

**13.** A decision, and a defensible one. `Type`'s operational methods
(`member_lookup_with_policy`, `bindings`, `try_call_dunder_get`) are the
internals of an inference engine under active development; making them public
would freeze an unstable interface and invite consumers to build parallel type
systems that drift.

The consequence for you: **Option B is enough for everything except supplying
your own receiver.** So the decision point is not "which option is better in
general" but "when do I first need a chosen receiver" — which is
`plan/03-call-tree/06`, attributes and `self`. Deciding at that moment (rather
than at the start, or at M6.4 when it blocks you) is the informed version of the
plan's advice.

**14.**

| need | on Option B? |
|---|---|
| plain function call | **yes** — `inferred_type` on `call.func` gives `FunctionLiteral`, then `Definition` |
| `obj.method()` where ty infers `obj` | **yes** — `static_member_type_for_attribute` does exactly this |
| `obj.method()` where *you* chose `obj` | **no** — needs a private member lookup, or you reimplement MRO attribute lookup yourself (Option C) |

Row 3 is the one that decides the project's shape, and it is precisely the
`self.handler.run()` case from `plan/03-call-tree/06`.

---

**15.–16.** Report your numbers. What to do with them:

- A high single-callee fraction means the context-free tree (M5) is already
  mostly right, and M6 is a precision improvement on a working product.
- A high union/`Unknown` fraction means the opposite, and M5's output should not
  be shipped to users even behind a flag.

For the `Unknown` sample, the dominant cause on most Python codebases is
**missing annotations** — which is worth knowing, because it means precision
improves as a codebase adds typing, without you changing anything.

**17.** M5 is a **useful intermediate product internally and a misleading one
externally.**

Useful because it exercises every piece of plumbing — traversal, frames, budget,
serialisation, project filtering, builtin filtering, ID lookup — with the hard
part switched off. When M6 misbehaves you know the bug is in the environment,
not the scaffolding. That is exactly why the plan makes it a separate milestone.

Misleading if shipped, because a context-free tree is *plausible* — right shape,
right names, right IDs — and wrong in a way nobody can see without checking
individual paths. Users would trust it. The plan's phrasing is "produces a
plausible tree"; treat "plausible" as a warning, not a gate.
