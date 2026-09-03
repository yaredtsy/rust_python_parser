# 10. A worked example, step by step

One small file. We trace it through Jedi, then through ty, then through the
interpreter you will build. Everything from chapters 4 to 9 shows up here.

---

## The file

```python
# app.py
 1  class JsonWriter:
 2      """ID: aaa"""
 3      def write(self, d):
 4          """ID: bbb"""
 5          print(d)
 6
 7  class CsvWriter:
 8      """ID: ccc"""
 9      def write(self, d):
10          """ID: ddd"""
11          print(d)
12
13  def emit(writer, data):
14      """ID: eee"""
15      writer.write(data)
16
17  def main():
18      """ID: fff"""
19      emit(JsonWriter(), {"a": 1})
20      emit(CsvWriter(), ["a", 1])
```

The question: **starting from `main`, what is the call tree?**

The answer we want:

```
root
└── emit                          ID: eee
    ├── JsonWriter.write          ID: bbb    ← from line 19
    └── CsvWriter.write           ID: ddd    ← from line 20
```

Both `write` methods appear, and each one is there *because of a specific call
site*.

Note there is **one** `emit` node, not two. Both calls are in `main`, so they
share a frame, and `add_child` merges them by name — one node, `call_count = 1`,
with both writes accumulated inside. Two calls from the same frame are the same
path.

> ⚠ Because it is merged, this example's JSON looks a lot like what a path-free
> tool would print. It is good for learning the *mechanism*, but it does not
> prove ty is insufficient. The example that does is at the end of this
> chapter — same calls, different callers.

---

## Part 1 — How Jedi gets there

### Step 1: parse

`jedi.Script(path="app.py")` parses with parso. You get a tree of nodes with
`start_pos` / `end_pos` tuples.

### Step 2: find the call sites in `main`

Your `_analyze_function` walks `main`'s body and finds two `atom_expr` nodes
that end in a call trailer. It makes two `CallNode` objects:

```
CallNode(name="emit", line=19, call_col_pos=8, call_index=0)
CallNode(name="emit", line=20, call_col_pos=8, call_index=0)
```

### Step 3: resolve the first one

```python
leaf = tree_node.get_name_of_position((19, 8))     # the name `emit`
call_context = parent_context.create_context(leaf)
callee_values = helpers.infer(inference_state, call_context, leaf)
# → [FunctionValue for emit]
```

Then the checks, in order:

| Check | Result |
|---|---|
| is `emit` a builtin name? | no |
| is it project code? | yes |
| qualified name | `app.emit` |
| seen already at this frame? | no |
| does it have an `ID:`? | yes, `eee` |
| is it an ancestor? | no |

So it becomes a child. `add_child` puts `emit` under `root`.

### Step 4: ★ bind the arguments

```python
arguments = TreeArguments(inference_state, call_context, arglist, trailer)
# arglist is: JsonWriter(), {"a": 1}

function_context = callee_for_args.as_context(arguments)
```

Now `function_context` is a context where:

```
writer → the value of JsonWriter()
data   → the value of {"a": 1}
```

**This is the step with no ty equivalent.**

### Step 5: walk `emit`'s body inside that context

`_analyze_function` finds `writer.write(data)` on line 15. It resolves it:

```python
leaf = get_name_of_position((15, ...))     # the name `write`
callee_values = helpers.infer(inference_state, function_context, leaf)
```

Because we are in `function_context`, Jedi resolves `writer` to the
`JsonWriter` instance. So `writer.write` resolves to `JsonWriter.write`.

That has `ID: bbb`. It becomes a child of `emit`.

### Step 6: repeat for line 20

Same thing, but `as_context` binds `writer → CsvWriter()`. So this time
`writer.write` resolves to `CsvWriter.write`, `ID: ddd`.

### Step 7: `add_child` merges

Both call sites resolved `emit` with the same qualified name `app.emit`. So
`add_child` finds the existing child and merges:

```python
if existing.target_qname == child.target_qname:
    existing.call_count += 1
    return existing
```

One `emit` node, `call_count = 1`, with both `write` methods under it.

> **This is the rule, confirmed by the driver's author:** a frame's identity is
> `(parent frame, qualified name)`. Same parent + same name = one node. Different
> parents = different nodes.
>
> One consequence to design for: `emit`'s body is walked **twice** — once with
> `writer → JsonWriter`, once with `writer → CsvWriter` — both times into the
> *same* frame. So your walk must be able to add children to a frame that
> already has some.
> ([`plan/03-call-tree/09`](../plan/03-call-tree/09-path-identity.md#the-merge-rule))

### The Jedi summary

| Step | Who did the work |
|---|---|
| parse | parso |
| find call sites | **your code** |
| resolve a name to a function | Jedi |
| bind arguments to parameters | Jedi (`as_context`), called by your code |
| resolve `writer` inside `emit` | Jedi, using the context |
| build the tree | **your code** |

Only two rows are yours. That is why the port is confusing at first — most of
what your tool does is not in your file.

---

## Part 2 — What ty gives you, alone

### Setup

```rust
let db = ProjectDatabase::use_defaults(metadata, system);
let file = system_path_to_file(&db, "app.py")?;
let module = parsed_module(&db, file.python_file(&db)).load(&db);
let model = SemanticModel::new(&db, file);
```

### Finding the calls in `main`

A visitor over `main`'s body, skipping nested functions:

```rust
// finds: ExprCall at line 19, ExprCall at line 20
```

Same as Jedi's step 2, just written as a visitor.

### Resolving `emit`

```rust
let ty = call.func.inferred_type(&model);
// → Some(Type::FunctionLiteral(emit))
```

Good — same answer as Jedi.

### Now walk into `emit` and ask about `writer`

```rust
// inside emit's body, on line 15
let receiver_ty = attribute.value.inferred_type(&model);   // `writer`
// → Type::Dynamic(Unknown)
```

**Unknown.** And it is Unknown from line 19's path and from line 20's path,
because `inferred_type` never knew there were paths at all.

If you ask ty for the definitions of `writer.write` anyway:

```rust
definitions_for_attribute(&model, attribute, ResolveAliases)
// → [] , or possibly both write methods if ty guesses from all classes
```

Either way you cannot tell which one belongs to which call site.

### The ty-only tree

```
main
└── emit
    └── write  →  { JsonWriter.write, CsvWriter.write }    ← merged, path-free
```

Close — in fact for *this* file the two trees are nearly the same shape. The
lost information is that line 19 specifically leads to `JsonWriter.write`.

To see that loss actually change the output, you need the calls in different
frames. See the last section.

---

## Part 3 — Your interpreter

Now the same trace, with your `Env`.

### Enter `main`

```
Env = {}                    (empty — main has no parameters)
Frame = root
```

### Line 19: `emit(JsonWriter(), {"a": 1})`

**Step A — resolve the callee.**

```rust
eval_callee(Name("emit"), env)
  → look in env: not found
  → ask ty: Type::FunctionLiteral(emit)
  → lift to: AbstractValue::Function(emit)
```

**Step B — evaluate the arguments, in the current env.**

```rust
eval(JsonWriter(), env)
  → it is a Call whose callee is a Class
  → make an instance with a NEW origin
  → AbstractValue::Instance { class: JsonWriter, origin: #1 }

eval({"a": 1}, env)
  → not something we model
  → ask ty → Ty(dict[str, int])
```

**Step C — the checks** (same six as Jedi): project code ✓, qname `app.emit` ✓,
ID `eee` ✓, not an ancestor ✓. Add child.

**Step D — ★ bind parameters. This is your `as_context`.**

```rust
Env_emit = {
    writer → Instance { JsonWriter, #1 },
    data   → Ty(dict[str, int]),
}
```

**Step E — walk `emit`'s body with `Env_emit`.**

Line 15 is `writer.write(data)`.

```rust
eval_callee(Attribute(Name("writer"), "write"), Env_emit)
  → evaluate the receiver `writer`
      → look in env: FOUND → Instance { JsonWriter, #1 }     ← ★ the win
  → look up "write" on JsonWriter, walking the MRO
      → BoundMethod { func: JsonWriter.write, receiver: #1 }
```

Checks pass, `ID: bbb`. Add as a child of `emit`.

Then recurse into `JsonWriter.write` with `self → Instance{JsonWriter,#1}` and
`d → Ty(dict)`. Its body calls `print(d)` — a builtin, so it is skipped, and
that branch ends.

### Line 20: `emit(CsvWriter(), [...])`

Same, but:

```rust
eval(CsvWriter(), env) → Instance { class: CsvWriter, origin: #2 }
                                                             ↑
                                            a DIFFERENT origin from #1
```

So `Env_emit` this time binds `writer → Instance{CsvWriter, #2}`, and line 15
resolves to `CsvWriter.write`, `ID: ddd`.

### The result

```
root
└── emit                    ID: eee     call_count = 1
    ├── JsonWriter.write    ID: bbb
    └── CsvWriter.write     ID: ddd
```

Matches Jedi. ✓

---

## What did the work

| Step | Who |
|---|---|
| parse | **ty** (cached) |
| find calls in a body | **you** (copied from `ty_ide`) |
| resolve `emit` (a plain name) | **ty** |
| construct `JsonWriter()` | **you** (need a fresh origin) |
| bind `writer` and `data` | **you** ← the whole project |
| resolve `writer` on line 15 | **you** (env hit) |
| look up `.write` on the class | **ty** (MRO) |
| know `print` is a builtin | **ty** / `ruff_python_stdlib` |
| build the tree | **you** |

Three rows are the real work. Everything else is ty.

---

## Why `origin` matters — a harder example

Change `main` to keep both writers alive:

```python
def main():
    a = JsonWriter()
    b = CsvWriter()
    emit(a, 1)
    emit(b, 2)
```

Now `a` and `b` are both instances in the env. If your `AbstractValue` did not
have an `origin` field, `Instance{JsonWriter}` and `Instance{CsvWriter}` would
still be distinguishable — different classes.

But this one breaks without `origin`:

```python
def main():
    a = Pipeline(JsonHandler())
    b = Pipeline(CsvHandler())
    a.run()          # should reach JsonHandler.process
    b.run()          # should reach CsvHandler.process
```

Both `a` and `b` are `Instance{Pipeline}` — **the same class**. Only the
`origin` tells them apart. Without it, their `handler` attributes merge and both
calls resolve to both handlers.

That is why the plan puts `origin: OriginId` in the value type, and why
"two live instances, no cross-talk" is one of the first tests to write.

---

## The version where ty visibly fails

Move the two calls into separate functions:

```python
def read_json():
    emit(JsonWriter(), {"a": 1})

def read_csv():
    emit(CsvWriter(), ["a", 1])

def main():
    read_json()
    read_csv()
```

**Your tree** — two `emit` nodes now, because they sit under different parents:

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

**ty's answer** — the same under both parents, because `outgoing_calls(db, file,
offset)` has no way to know which parent it is under:

```
main
├── read_json
│   └── emit
│       └── write → { JsonWriter.write, CsvWriter.write }
└── read_csv
    └── emit
        └── write → { JsonWriter.write, CsvWriter.write }
```

Now the difference is undeniable, and it is the difference your tool exists to
provide: *"reading JSON reaches `JsonWriter.write`, and nothing else."*

**Use this cross-scope form whenever you write a test for path sensitivity.**
The same-frame version merges, and a merged tree can hide a broken interpreter.

---

## Try it yourself

Take this file and trace it by hand, on paper, the same way:

```python
def make(kind):
    if kind == "json":
        return JsonWriter()
    return CsvWriter()

def run(kind):
    w = make(kind)
    w.write(1)
```

Questions to answer:

1. What does `eval(make(kind), env)` return? (Hint: the function has two
   `return` statements.)
2. What goes into the env for `w`?
3. How many children does `run` have in the tree?
4. Which chapter of the plan covers this case?

Answer to 4: [`plan/03-call-tree/10`](../plan/03-call-tree/10-return-values-and-state.md).
This is the return-value flow case, and it is the reason that chapter exists.

---

→ Next: [`11-reading-the-source.md`](11-reading-the-source.md)
