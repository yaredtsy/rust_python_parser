# 07 — Types and inference ★

**Goal:** you can print the type of any expression, you can classify what ty
knows and what it does not, and you can state precisely — with a fixture in your
hand — why ty cannot answer the question your call tree asks.

This is the conceptual centre of the whole port. Everything before it was
mechanical. Everything after it is a consequence.

---

## Read first

- `tutorial/09-types-and-inference.md` — the most important chapter
- `plan/02-mapping/04-jedi-inference-to-ty.md` — the gap, stated precisely
- `tutorial/10-worked-example.md` — read it *after* step 4 below, not before

---

## The mental model

### `Value` and `Type` answer different questions

Jedi's `Value` is *"what is this, on this path, right now"* — the product of a
lazy interpreter that pretends to run your code. `parent_context.create_context(leaf)`
and `as_context(arguments)` exist to carry the *situation* along with the value.

ty's `Type` is *"what could this be, in every possible run"* — a sound static
description. It is not a simulation and never pretends to be.

```
def emit(writer, data):
    writer.write(data)
```

| question | answer |
|---|---|
| ty: what is `writer`? | `JsonWriter \| XmlWriter` — both, because both happen somewhere |
| jedi, called from `run_json`: what is `writer`? | `JsonWriter` — it bound the actual argument |

**Both are correct.** They are answers to different questions. Your call tree
needs the second one, and no amount of reading ty's API will find it, because it
is not a missing feature — it is a different question.

### Flow-sensitive ≠ context-sensitive

This distinction is the single most important idea in the port, and it is easy
to blur.

**Flow-sensitive** = the answer depends on *where in the code* you ask.
ty is fully flow-sensitive:

```python
thing = 1          # here, thing is Literal[1]
thing = "two"      # here, str
thing = Cache()    # here, Cache
```

**Context-sensitive** = the answer depends on *who called this function*.
ty is **not** context-sensitive, and cannot be:

```python
def emit(writer, data):
    writer.write(data)     # depends on the caller. ty has one answer for all callers.
```

The reason is architectural, not an oversight. Inference is cached as
`infer_scope_types(scope)`. Keying that on a call-site environment would give
almost every call a distinct key — the cache would store everything and hit
nothing. `plan/03-call-tree/02` makes this argument in full.

So: **ty is your oracle for leaf questions; the path-dependent part is yours.**
That is the 70% of the project in `plan/03-call-tree/`, and you are not building
it in this folder. You are just going to *see* the boundary.

### What `Unknown` means

ty is pre-1.0. Inference gaps are normal, frequent, and not errors. Your
interpreter must treat `Unknown` (a `Dynamic` variant) as an ordinary outcome
with a fallback, not as a failure. `unannotated(param)` in the fixture produces
one on purpose.

Counting `Unknown`s is your precision dashboard — `plan/04-build/00-dev-cli.md`
puts it in `--stats` for exactly this reason.

---

## The API, verified at `ac201b8`

```rust
use ty_python_semantic::{SemanticModel, HasType, HasDefinition};
use ty_python_semantic::types::Type;

let model = SemanticModel::new(db, program_file);
expr.inferred_type(&model)     // -> Option<Type<'db>>       trait HasType
node.definition(&model)        // -> Definition<'db>         trait HasDefinition
model.program_environment()    // -> ProgramEnvironment<'db>
```

### The `Type` variants — the real list at this revision

```
Dynamic          Divergent        Never
FunctionLiteral  BoundMethod      KnownBoundMethod    WrapperDescriptor
Callable         ModuleLiteral
ClassLiteral     GenericAlias     SubclassOf
NominalInstance  ProtocolInstance
SpecialForm      KnownInstance    PropertyInstance    SlotDescriptor
Union            Intersection     EnumComplement
AlwaysTruthy     AlwaysFalsy      LiteralValue
TypeVar          BoundSuper       TypeIs   TypeGuard   TypeForm
TypedDict        TypeAlias        NewTypeInstance
DataclassDecorator   DataclassTransformer
```

**[verified, `ty_python_semantic/src/types.rs:1629`]**. Longer than the plan's
list — `SlotDescriptor`, `EnumComplement`, `LiteralValue`, `TypeIs`,
`TypeGuard`, `TypeForm`, `TypedDict` are all in there.

You care about six of them:

| variant | what it means for your call tree |
|---|---|
| `FunctionLiteral` | a function object — **this is a callee you can resolve** |
| `BoundMethod` | a method with a receiver — **callee + the `self` you need** |
| `ClassLiteral` | a class object — a call on it is a **constructor** (quirk 7) |
| `NominalInstance` | an instance of a class — the receiver in `x.method()` |
| `Union` | several possibilities — **the fan-out problem** |
| `Dynamic` | Unknown — fall back, count it, do not crash |

### ⚠ The wall you will hit

```rust
// ty_python_semantic/src/types.rs   [verified]
fn Type::static_member(...)                        // private
fn Type::bindings(...)                             // private
pub(crate) fn Type::member_lookup_with_policy(..)  // pub(crate)
pub(crate) fn Type::try_call_dunder_get(...)       // pub(crate)
```

Counting `pub fn` on `Type` at this revision gives you about ten, against
roughly seventeen `pub(crate)` **[verified]** — and the public ones are things
like `is_none`, `is_deprecated`, `definition`. **The operational core of the
type system is deliberately not public API.**

The public tools take **AST nodes**, not types:

```rust
ty_python_semantic::types::ide_support::static_member_type_for_attribute(
    model, attribute: &ast::ExprAttribute
) -> Option<Type>
```

Read that signature carefully. It takes the *syntax* `writer.write` and decides
the receiver's type itself, internally. You cannot say "here is the receiver I
chose; give me the member". That is exactly backwards from what an interpreter
needs, and it is the crux of `plan/01-crates/04`'s Option A/B/C decision.

**You are not solving that here.** You are confirming it is real, so that when
you read `plan/03-call-tree/06` it lands as a memory rather than a claim.

---

## The fixtures

```
python/
├── flow.py ....... everything ty DOES answer: narrowing, reassignment,
│                   unions, literals, call return types, and one Unknown
└── context.py .... the one it does not: `emit` called from two frames
```

`context.py` is built to respect a rule you already know
(`MEMORY.md`): frame identity is `(parent, qname)`, and two calls **from the
same frame** merge. So the writers arrive from **different frames** —
`run_json → emit` and `run_xml → emit` — which is what makes the paths
genuinely distinct instead of merging into one node.

---

## Build it

### Step 1 — a type printer

Walk every expression in a file and print `source_slice → Type`, using
`inferred_type`. Something like:

```
flow.py:33  thing              Literal[1]
flow.py:34  thing              Literal["two"]
flow.py:35  Cache()            Cache
flow.py:36  thing.get("k")     …
```

`Type` implements a display/debug representation — find which one gives the
readable form and use it consistently.

**This tool is the deliverable of the exercise.** Every later question is
answered by pointing it at a fixture.

### Step 2 — confirm flow sensitivity

Run it over `flow.py` and check each function against your prediction:

| function | question |
|---|---|
| `narrowing` | what is `value` before the guard, and after it? |
| `reassigned` | three types for one name — do you get all three, at the right lines? |
| `branched` | what type at the `return`? Is it a `Union`? |
| `unannotated` | what is `param`? What variant exactly? |
| `literal_types` | does ty say `int` or `Literal[42]`? Both, in different places? |
| `calls_returning` | what is the type of the *call expression* `cache.get("k")`? |

The `literal_types` row is worth dwelling on. Literal types are a real
capability Jedi does not have in the same form, and they matter later: an
argument of `Literal["json"]` is more informative than `str` when you are
deciding which branch a callee takes.

### Step 3 — classify the callees

For every call in `flow.py` and `context.py`, print the type of `call.func`.
Then bucket them:

- `FunctionLiteral` → a plain function; you can get its `Definition`
- `BoundMethod` → a method; you also get the receiver
- `ClassLiteral` → a constructor call; quirk 7 says rewrite `target_id` to
  `ClassSchema/…` and descend into `__init__`
- `Union` → several; how many members?
- `Dynamic` → Unknown; count it

This bucketing **is** the skeleton of `resolve_calls`. You are not writing the
call tree, but you are writing the function that answers "what am I calling",
and every later exercise reuses it.

### Step 4 — the experiment that justifies the project

Point your type printer at `context.py`, inside the body of `emit`.

Print the type of `writer` and the type of `writer.write`.

**Predict first.** Then look.

You should get both writers — a `Union`, or two declarations, depending on how
you ask. Now answer these:

1. Is ty's answer *wrong*?
2. Which of the two `write` methods should appear under `run_json → emit` in
   your call tree?
3. Can you get that answer from `writer`'s type alone? From any function in
   ty's public API?
4. What extra input would you need at the moment you resolve `writer.write`?

Question 4's answer is the whole of `plan/03-call-tree/05`: an **environment**
mapping parameters to the values bound at this call site. Write your answer down
in your own words before you read that chapter — it will be a much better
chapter afterwards.

Now read `tutorial/10-worked-example.md`. It traces this exact shape through
Jedi and ty step by step, and it will read like a description of something you
just did.

### Step 5 — measure the gap

Take `run_deep → pass_through → emit`, where the writer travels two calls before
being used.

Ask ty for the type of `writer` inside `emit`. Then ask for the type of the
argument at the `run_deep` call site. Are they the same? What did the
intermediate frame do to the information?

Then count, on a real project of yours: how many callees resolve to a **single**
`FunctionLiteral`/`BoundMethod` (where ty alone is enough), and how many resolve
to a union or to `Unknown` (where it is not)?

That ratio is the honest scope of `plan/03-call-tree/`. It is usually smaller
than people fear and larger than they hope, and knowing your own number is worth
more than any general claim.

### Step 6 — confirm the wall

Try to write this function:

```rust
fn member_of(db: &dyn Db, receiver: Type<'_>, name: &str) -> Option<Type<'_>>;
```

Give it fifteen minutes. Read the error messages. Then find
`static_member_type_for_attribute` in `cargo doc` and read *its* signature.

Write down, in one sentence, why the public API cannot express your function.
That sentence is the justification for whichever option you eventually pick in
`plan/01-crates/04`, and having derived it yourself is worth more than adopting
the plan's recommendation.

---

## Traps

- **Expecting `Type` to be a value.** `NominalInstance(Cache)` means "an
  instance of `Cache`" — not *which* instance. Two different `Cache()` objects
  have the same type. `plan/03-call-tree/04` is about needing to tell them
  apart, and this is where you first feel that need.
- **Treating `Unknown` as an error.** It is normal. Fall back and count.
- **Assuming a union of two is small.** Unions grow multiplicatively along call
  chains; that is why `plan/03-call-tree/08` has fan-out caps.
- **Calling `inferred_type` on a node from another file's model.** It panics —
  the doc comment says so explicitly.
- **Concluding "ty is not good enough".** ty answers its question extremely
  well. Your tool asks a different one. Getting this framing right matters:
  every design in `plan/03-call-tree/` uses ty as an oracle rather than working
  around it.

---

## Done when

- [ ] your type printer works on any file and prints readable types
- [ ] you have answers for all six rows of the `flow.py` table
- [ ] every call in both fixtures is bucketed by callee variant
- [ ] you can state what ty says about `writer.write` and why it is correct
- [ ] you wrote down what extra input a per-path answer would need
- [ ] you know your project's single-callee vs union/Unknown ratio
- [ ] you tried to write `member_of` and can say in one sentence why you cannot

---

→ [`exam.md`](exam.md), then [`../08-classes-and-mro/README.md`](../08-classes-and-mro/README.md)
