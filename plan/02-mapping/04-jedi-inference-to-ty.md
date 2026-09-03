# 02.04 — Jedi inference vs ty inference: the conceptual gap

The bridge chapter. Read it before `03-call-tree/`.

---

## They answer different questions

> **Jedi:** *"If this program ran, what object would be here?"*
> **ty:** *"For all possible runs, what is the set of types that could be here?"*

Jedi is an **abstract interpreter with lazy evaluation**. It has a notion of
"the value at this point, given how we got here". Its `Context` objects form a
chain that mirrors a call stack.

ty is a **type checker**. It computes, for each *scope*, a fixed assignment of
types to names — flow-sensitive within the scope (narrowing works), but
**call-site-insensitive across scopes**. It infers each scope once and caches it.

That caching *is* the speed. It is also exactly what you lose.

---

## Worked example

```python
class JsonWriter:
    def write(self, d): print(json.dumps(d))

class CsvWriter:
    def write(self, d): csv.writer(sys.stdout).writerow(d)

def emit(writer, data):
    writer.write(data)          # ← the interesting line

def read_json():
    emit(JsonWriter(), data)    # path A

def read_csv():
    emit(CsvWriter(), data)     # path B

def main():
    read_json()
    read_csv()
```

### What your Jedi resolver produces today

```
root
└── main
    ├── read_json
    │   └── emit
    │       └── JsonWriter.write    ← `writer` was bound to JsonWriter()
    └── read_csv
        └── emit
            └── CsvWriter.write     ← same code, different path, different answer
```

Two `emit` nodes, because they hang under **different parent frames**. Each
shows only what its own path reaches. This is the "tree, not call graph"
property, and it is the entire value of your tool.

> Note the calls are deliberately in *different functions*. Two calls to `emit`
> from one frame would **merge** into a single node with both writes inside it
> — `add_child` dedupes by qname. See
> [`03-call-tree/09`](../03-call-tree/09-path-identity.md#the-merge-rule).

The mechanism, from `call_resolver.py:166-187`:
```python
arguments = self.create_args(callee_for_args, trailer, ...)   # actual arg exprs
function_context = callee_for_args.as_context(arguments)      # ← bind them
self._analyze_function(function_node, function_context, ...)  # ← descend with them
```
`as_context(arguments)` is the whole trick. Inside `function_context`, the name
`writer` resolves to whatever was passed.

### What ty produces

`writer` is unannotated. ty infers its declared type as `Unknown` (or, with
inference of unannotated parameters, some union). Asking ty for the type of
`writer` inside `emit` gives you **one answer, the same on every path**. It has
no `as_context(arguments)`. There is nowhere to put the information.

### What `ty_ide::outgoing_calls` produces

```
emit
└── write   ×2 declarations: JsonWriter.write, CsvWriter.write
```

A flat, unioned, path-free answer. Correct for an IDE's "Call Hierarchy" panel.
**Not your tree.** → [`03-call-tree/02`](../03-call-tree/02-why-ty-alone-cannot.md)

---

## Term-by-term dictionary

| Jedi | ty | Same? |
|---|---|---|
| `Value` | `Type<'db>` | ✗ — a Jedi `Value` can be a *specific object*; a ty `Type` is a *set of values* |
| `Context` | `ScopeId` + inferred types | ✗ — ty scopes have no per-call identity |
| `ModuleContext` | `ProgramFile` + `SemanticModel` | ≈ |
| `FunctionExecutionContext` (`as_context(args)`) | **nothing** | ✗ **the gap** |
| `helpers.infer(state, ctx, leaf)` | `expr.inferred_type(&model)` | ≈ but context-free |
| `value.py__getattribute__("x")` | `Type::member_lookup_with_policy` | ≈, but `pub(crate)` |
| `TreeInstance(state, ctx, cls, args)` | `Type::NominalInstance` | ✗ — ty's instance has no per-construction identity |
| `BoundMethod(inst, cls, func)` | `Type::BoundMethod` | ≈ |
| `TreeArguments(state, ctx, arglist, trailer)` | `CallArguments` + `Bindings` | ≈ |
| `value.py__mro__()` | `ClassLiteral::iter_mro` | ✓ |
| `name.get_qualified_names(True)` | module name + class/function name | ≈ (see 02-mapping/03) |
| `is_builtins_module()` | `ruff_python_stdlib` / `KnownModule` | ✓ |
| `Script(path, project, env)` | `SemanticModel::new(db, file)` | ✓ but **hold one db, not one per call** |

The single row that matters: **`as_context(arguments)` has no ty equivalent.**

---

## Why ty *cannot* just add it

Not an oversight. Context-sensitivity is incompatible with ty's performance model:

- Salsa caches `infer_scope_types(scope)`. A context-sensitive version would be
  `infer_scope_types(scope, environment)` — keyed on an unbounded value that
  almost never repeats. The cache hit rate collapses to ~0.
- Type checking needs *one* answer per expression to report *one* diagnostic.
  N answers per N paths is a different product.
- The path space is exponential. A checker must be roughly linear in program
  size. Your tool is deliberately not.

So: ty is fast **because** it doesn't do what you need. You are not going to
find a flag.

---

## The consequence, stated plainly

**You are writing an abstract interpreter.** ty is its substrate, not its
engine. Concretely, ty provides:

| Role | ty gives you |
|---|---|
| Parsing | `parsed_module` — cached, error-recovering, version-aware |
| Name → definition | `definitions_for_name`, the semantic index |
| Import resolution | `ty_module_resolver` |
| Class model | MRO, attributes, descriptors, metaclasses |
| Signature matching | `resolved_call_signature`, `CallSignatureDetails` |
| **Fallback inference** | when your environment has no binding for an expression, ask ty |

That last row is the design's keystone. Your interpreter is **precise where it
has path information and delegates to ty everywhere else**. It does not need to
understand comprehensions, `with` statements, decorators, or the stdlib — it
asks ty and moves on.

That is also why this port is *tractable*: you are not reimplementing Jedi. You
are implementing one narrow thing Jedi does — argument binding across a call —
and letting a much faster engine do everything else.

---

## The good news on performance

Your current traversal already pays for full Jedi inference at every node. The
Rust version pays for:

- ty inference: cached per scope, amortised to ~0 across paths
- your environment lookups: `FxHashMap` hits, ~nanoseconds
- the tree walk: pointer chasing over an already-parsed AST

The *shape* of the work is unchanged. The constant factor drops by 1–2 orders
of magnitude, and the repeated work (same function on 50 paths) drops from
"re-infer everything" to "re-walk the AST with a different env".

---

→ Next: [`03-call-tree/01-what-jedi-actually-does.md`](../03-call-tree/01-what-jedi-actually-does.md) ★
