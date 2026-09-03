# 9. Types and inference — the important chapter

This is where Jedi and ty really differ. Everything in the plan comes from this
one idea. Read it slowly.

---

## `Value` and `Type` are not the same thing

They look the same. They are not.

### Jedi's `Value` — "an object that would be here"

A Jedi `Value` is a **specific thing**. When Jedi evaluates `JsonWriter()`, it
makes a `TreeInstance` — a value that represents *that particular object,
created by that particular call, with those particular arguments*.

Values remember where they came from. They carry a context. Ask a value for an
attribute, and it answers based on how it was built.

### ty's `Type` — "the set of things that could be here"

A ty `Type` is a **set**. `Type::NominalInstance(JsonWriter)` means *"some
JsonWriter, we do not know which"*.

Two `JsonWriter` instances made at different places in your program have the
**exact same `Type`**. There is no "which one" — the type system does not have
that concept, on purpose.

```
Jedi:  JsonWriter()  at line 5  →  Value A
       JsonWriter()  at line 9  →  Value B      (A ≠ B)

ty:    JsonWriter()  at line 5  →  NominalInstance(JsonWriter)
       JsonWriter()  at line 9  →  NominalInstance(JsonWriter)   (identical!)
```

**That is the whole problem in two lines.** Your tool needs to tell those two
apart, because they might hold different handlers.

---

## The `Type` enum

Here are the variants you will actually meet:

```rust
pub enum Type<'db> {
    FunctionLiteral(FunctionType<'db>),      // a specific function object
    BoundMethod(BoundMethodType<'db>),       // obj.method — receiver attached
    ClassLiteral(ClassLiteral<'db>),         // the class itself, e.g. `JsonWriter`
    NominalInstance(NominalInstanceType<'db>),// an instance, e.g. `JsonWriter()`
    ModuleLiteral(ModuleLiteralType<'db>),   // a module object
    Callable(CallableType<'db>),             // anything callable with a signature
    Union(..),                               // A or B
    Intersection(..),                        // A and B
    Dynamic(..),                             // Any / Unknown
    Never,                                   // no value can be here
    Divergent(..),                           // cycle marker during inference
    // ... about 20 more
}
```

Four of these do most of the work for you:

| Variant | Means | Your use |
|---|---|---|
| `FunctionLiteral` | a specific function | a call target |
| `ClassLiteral` | a class object | a constructor target |
| `NominalInstance` | an instance of a class | a receiver for `.method()` |
| `BoundMethod` | method + its receiver | `obj.method` as a value |

### `Union` — the one that hurts

```python
if flag:
    w = JsonWriter()
else:
    w = CsvWriter()
w.write(1)
```

ty says `w` is `Union([JsonWriter, CsvWriter])`. One type holding two options.

For a type checker this is perfect. For your tree it is a problem: if you keep
it as a union, you get one node with two possible callees mixed together. You
want **two separate branches**.

So when you get a `Union` from ty, you must **explode it** into separate values
and follow each one. The plan calls this "keep alternatives enumerable". If you
ever collapse them back into a union, you have quietly turned your tree back
into a graph.

---

## Asking for a type

```rust
use ty_python_semantic::HasType;

let ty: Option<Type> = expr.inferred_type(&model);
```

That is it. One method, works on any expression.

Note the `Option`. **"I do not know" is a normal answer, not an error.** ty is
still young; some things infer to `Unknown`. Your code must handle it as an
everyday case.

---

## Now the important part: what inference actually means

Here is the example from chapter 1, in full.

```python
class JsonWriter:
    def write(self, d): ...

class CsvWriter:
    def write(self, d): ...

def emit(writer, data):
    writer.write(data)         # ← the interesting line

def read_json():
    emit(JsonWriter(), {"a": 1})     # path A

def read_csv():
    emit(CsvWriter(), ["a", 1])      # path B

def main():
    read_json()
    read_csv()
```

> The two calls are in **different functions** on purpose. If both were in
> `main`, your driver would merge them into one `emit` node (it dedupes children
> by name), and the merged picture looks a lot like ty's answer — so it would
> prove nothing. Keeping them apart makes the difference visible.

### What Jedi does

Your code at `call_resolver.py:166-187`:

```python
arguments = self.create_args(callee_for_args, trailer, ...)  # the actual args
function_context = callee_for_args.as_context(arguments)     # ← bind them
self._analyze_function(function_node, function_context, ...)  # walk with them
```

`as_context(arguments)` makes a context where `writer` is bound to the argument
from *this* call site. Then, walking `emit`'s body:

- On path A (through `read_json`): `writer` → `JsonWriter` → `JsonWriter.write`
- On path B (through `read_csv`): `writer` → `CsvWriter` → `CsvWriter.write`

**Two different answers for the same line of code**, depending on the path.

### What ty does

Ask ty for the type of `writer` inside `emit`. It has no annotation, so ty says
`Unknown`. And it says that **every time**, no matter who called `emit`.

There is no `as_context(arguments)`. There is nowhere to put the caller's
arguments. `SemanticModel` takes a file, not a call site.

### Why ty cannot just add it

This is not a missing feature. It is a design choice that ty cannot undo:

1. **The cache would die.** Salsa caches `infer_scope_types(scope)`. A
   context-sensitive version would be `infer_scope_types(scope, arguments)` —
   keyed on a value that is almost never the same twice. The cache hit rate goes
   to nearly zero, and ty's speed comes from that cache.

2. **A checker needs one answer.** To report one error per line, ty must have
   one type per expression. N answers for N paths is a different product.

3. **Paths grow exponentially.** A checker must be roughly linear in program
   size. Your tool is deliberately not.

**So: ty is fast *because* it does not do what you need.** There is no flag to
turn on.

---

## Flow-sensitive is not the same as context-sensitive

This trips people up, so let us be precise. ty **is** flow-sensitive:

```python
def f():
    w = JsonWriter()
    w.write(1)          # ty knows w is JsonWriter here ✓
    w = CsvWriter()
    w.write(2)          # ty knows w is CsvWriter here ✓
```

ty tracks how types change *through statements inside one scope*. This is called
**narrowing**, and it works well.

What ty does **not** do is track types *across a function boundary based on the
caller*:

```python
def g(w):
    w.write(1)          # ty has no idea; the caller is not part of the question
```

- **flow-sensitive** = "the answer depends on where you are in the function" ✓ ty
  does this
- **context-sensitive** = "the answer depends on who called the function" ✗ ty
  does not

You need the second one. Do not be fooled by seeing narrowing work and
concluding that ty tracks values across calls.

---

## What `ty_ide::outgoing_calls` gives you

There is a ready-made call hierarchy in `ty_ide`. It will look like the answer.
Here is what it actually returns for the example:

```
main
├── read_json
│   └── emit
│       └── write → { JsonWriter.write, CsvWriter.write }   ← BOTH
└── read_csv
    └── emit
        └── write → { JsonWriter.write, CsvWriter.write }   ← BOTH again
```

The **same answer under both parents**, because `outgoing_calls` cannot know
which parent it is under.

And here is what your Jedi driver gives:

```
root
└── main
    ├── read_json
    │   └── emit
    │       └── JsonWriter.write     ← only this one
    └── read_csv
        └── emit
            └── CsvWriter.write      ← only this one
```

Now the difference is obvious. ty says *"`write` might be either of these,
always."* Your tree says *"reading JSON reaches `JsonWriter.write`, and nothing
else."*

Look at the signature to see why:

```rust
pub fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize)
    -> Vec<OutgoingCall>
```

`db`, a file, an offset. That is the whole input. **There is nowhere to say
"given that `writer` is a `JsonWriter`".**

Still read the source — `ty_ide/src/call_hierarchy/outgoing_calls.rs`. Its tree
walking is excellent and you should copy it. It already handles decorators,
default values, annotations, and nested functions correctly. You only need to
replace the part where it resolves a callee.

---

## So what do you build?

An **abstract interpreter**. That sounds grand; it is simpler than it sounds.

You walk the tree keeping a small map: **name → value on this path**.

```
              ┌───────────────────────────────┐
   look here  │ YOUR Env                      │
   first  ──► │   writer → JsonWriter instance│  ← precise, knows the path
              └──────────────┬────────────────┘
                             │ not found?
                             ▼
              ┌───────────────────────────────┐
   fall back  │ ty                            │
   to this ──►│   inferred_type(expr)         │  ← fast, path-free
              └───────────────────────────────┘
```

In code, the whole idea is two lines:

```rust
fn resolve(&self, expr: &Expr, env: &Env) -> Values {
    self.resolve_from_env(expr, env)                    // precise
        .unwrap_or_else(|| self.resolve_from_ty(expr))  // general
}
```

**Precise where you have path information. Fast everywhere else.**

And this is why the port is possible at all. You are not rewriting Jedi. You are
writing *one* thing Jedi does — binding arguments to parameters — and letting a
much faster engine do everything else: imports, classes, MRO, descriptors,
stdlib, comprehensions, decorators.

---

## What ty still does for you

Do not let this chapter make ty sound useless. It does an enormous amount:

| Job | ty handles it |
|---|---|
| parsing | yes, cached |
| following imports | yes, better than Jedi |
| class hierarchies and MRO | yes, cached |
| attributes, properties, descriptors | yes |
| stdlib types via typeshed | yes |
| generics, protocols, dataclasses | yes |
| **binding call arguments across a path** | **no — this is yours** |

One row is yours. The rest is free.

---

## Check yourself

1. In one sentence: how is Jedi's `Value` different from ty's `Type`?
2. Why do two `JsonWriter()` calls make the same ty `Type`?
3. What must you do with a `Union` type, and what happens if you do not?
4. What is the difference between flow-sensitive and context-sensitive?
5. Why can't the ty team just add an `arguments` parameter to inference?
6. What is the one job ty does not do for you?

If question 6 is unclear, read "So what do you build?" again. That is the
project.

---

→ Next: [`10-worked-example.md`](10-worked-example.md)
