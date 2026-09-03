# 03.01 — What your Jedi resolver actually does

Before replacing a mechanism, state it precisely. This is a line-by-line reading
of `call_resolver.py` **[verified]**, reduced to an algorithm.

---

## The loop, in pseudocode

```
resolve(call_node, parent_context, frame):

  1. leaf ← parent_context.tree_node.get_name_of_position(
                (call_node.position.line, call_node.call_col_pos))
     # the identifier immediately left of the '('

  2. if leaf.value in BUILTIN_NAMES: return          # by NAME, before inference

  3. call_context ← parent_context.create_context(leaf)
     callees      ← jedi.helpers.infer(state, call_context, leaf)
     # ★ resolved IN parent_context — so parameters bound by the caller are visible

  4. trailer ← the '(...)' node containing the arguments

  5. for callee in callees:
       a. if not under project path: continue            # first-party only
       b. qname ← callee.name.get_qualified_names(True)
       c. if qname already seen at this frame: continue
       d. id ← extract `ID:` from callee's docstring
          if id is None: continue                        # ★ no ID → invisible
       e. if frame.is_ancestor(qname): continue          # cycle guard
       f. child ← frame.add_child(CallFrameStack(qname, id))
          # add_child dedupes by qname and bumps call_count

       g. arguments ← TreeArguments(state, call_context, trailer.children[1], trailer)

       h. if callee.is_function():
              ctx ← callee.as_context(arguments)         # ★★ THE CONTEXT ★★
              analyze(callee.tree_node, ctx, child)

          elif callee.api_type == "class":
              child.target_id ← "ClassSchema/" + id
              instance ← TreeInstance(state, callee.parent_context, callee, arguments)
              init     ← callee.py__getattribute__("__init__")[0]
              bound    ← BoundMethod(instance, callee, init)
              ctx      ← bound.as_context(arguments)     # ★★ SAME MECHANISM ★★
              analyze(init.tree_node, ctx, child)


analyze(function_node, function_context, frame):
  calls ← every call-trailer in function_node's body,
          NOT descending into nested Class/Function     # parser.py:78-80 rule
  for c in calls:
      resolve(c, function_context, frame)               # recurse
```

---

## The three load-bearing lines

```python
# 1. resolution happens in the CALLER's context
callee_values = helpers.infer(self.inference_state, call_context, leaf)

# 2. arguments are captured as unevaluated expressions + their context
arguments = TreeArguments(inference_state, call_context, arglist, trailer)

# 3. the callee body is analysed in a context where params are bound to them
function_context = callee_for_args.as_context(arguments)
```

Line 3 is the one with no ty equivalent. Everything in `03-call-tree/` exists to
reconstruct it.

## What `as_context(arguments)` gives you, concretely

Inside the returned `FunctionExecutionContext`, when Jedi resolves a name it
first checks the function's own parameters. If the name is a parameter, it
evaluates the corresponding *argument expression* **in the caller's context** —
lazily, on demand. So:

```python
def outer():
    w = JsonWriter()
    middle(w)

def middle(x):
    inner(x)          # `x` → evaluated in outer's context → JsonWriter instance

def inner(y):
    y.write(1)        # `y` → evaluated in middle's context → `x`
                      #      → evaluated in outer's context → JsonWriter instance
                      # → resolves to JsonWriter.write ✓
```

**The binding chains transitively through arbitrary depth.** This is the
property you must preserve, and the reason a one-level "just look at the
arguments" hack is not sufficient.

Jedi does it lazily (each `Value` holds a reference to its defining context).
Your Rust version will do it **eagerly** — evaluate arguments to abstract values
at the call site, store the values in the child environment. Eager is simpler,
avoids Jedi's recursion-guard complexity, and is faster. The cost is that you
evaluate arguments that the callee never uses; in practice that's cheap and
usually a win because you evaluate once instead of once per use.

---

## What it does *not* do — and the trap in that phrase

`call_resolver.py` contains explicit code for exactly one thing:
**binding call arguments to parameters**. Everything else it delegates to Jedi.

That makes "what the resolver doesn't do" a misleading question. The right
question is **"what does the whole system do today"** — because Jedi's ordinary
inference runs *inside* the `function_context` your resolver built, so some of
what looks absent from the resolver is present in the behaviour.

### Genuinely absent — and keeping it that way is what keeps this finite

- **No branch analysis.** `if cond: f() else: g()` yields *both* `f` and `g` as
  children of the same frame. Conditions are never evaluated, paths never split
  on them.
- **No loop modelling.** A call in a `for` body is visited once.
- **No `*args` / `**kwargs` reasoning** beyond what `TreeArguments` does.
- **No global / nonlocal state.**

### Present via Jedi, *not* via the resolver ⚠

These are the ones that will silently regress if you port `call_resolver.py`
line-by-line, because the Rust version has no Jedi underneath to fall back on:

- **Return-value flow.** `w = make_writer()` then `w.write()` resolves today.
  Jedi re-infers `make_writer()` lazily when you ask about `w`, and it does so
  in the current execution context — so parameters flowing into `make_writer`
  can influence the answer. **[check]** — confirm the context-sensitive part
  with a fixture; the plain inference part is core Jedi and certain.

- **Attribute / object state.** `self.handler = handler` in `__init__`, read
  back as `self.handler.run()` in another method, resolves today because
  `TreeInstance` carries the constructing `arguments`
  (`call_resolver.py:191-196`).

> ty replaces Jedi's *type* answers but not its *value* answers. Anywhere the
> current behaviour depends on Jedi's lazy in-context inference rather than on
> your explicit binding code, you must build the equivalent yourself or lose it.
> → [`10-return-values-and-state.md`](10-return-values-and-state.md)

### The line to hold

Add **value flow** (returns, assignments, object state). Do **not** add
**path conditions** (branch feasibility, loop iteration counts, comparisons).

Value flow is what makes the tree accurate on real code — factories, builders,
dependency injection, constructor-injected handlers. It is bounded: each value
is computed once per environment and memoised.

Path conditions are what make it explode. `if`/`else` contributing both branches
is a union of size 2; deciding *which* branch runs requires evaluating
conditions, which requires modelling comparisons, which requires modelling
integers, and you are writing a symbolic executor.

**Union at branches, precision at bindings.** That is the whole design rule.

---

## The invariants to test against

Write these as fixtures now; they are your parity suite for
[`04-build/03`](../04-build/03-transport-and-parity.md).

| # | Fixture | Expected |
|---|---|---|
| 1 | `read_json()` calls `emit(JsonWriter())`, `read_csv()` calls `emit(CsvWriter())`, `main` calls both | two `emit` nodes under **different parents**, one leaf each. ★ the cross-scope form — do not use two calls from one frame |
| 2 | 3-deep parameter pass-through (`outer→middle→inner`) | correct leaf at depth 3 |
| 3 | direct recursion `f` calls `f` | one `f` node, no infinite loop |
| 4 | mutual recursion `f→g→f` | terminates, `f` not nested in `f` |
| 5 | callee without `ID:` docstring | absent from tree, *and* not descended into |
| 6 | two calls to same fn from one frame | one child, `call_count == 1` |
| 7 | call to `len()` | absent (builtin) |
| 8 | call into `site-packages` | absent (non-project) |
| 9 | `Foo()` constructor | `ClassSchema/` id, children from `__init__` |
| 10 | `if c: f() else: g()` | both `f` and `g` as siblings |
| 11 | nested `def` containing a call | call belongs to the inner def, not the outer |
| 12 | `a.b().c()` | two CallNodes, `call_index` 0 and 1 |

Run all twelve against the **Python driver first** and record the output as
golden files. Those goldens are your specification.

---

→ Next: [`02-why-ty-alone-cannot.md`](02-why-ty-alone-cannot.md)
