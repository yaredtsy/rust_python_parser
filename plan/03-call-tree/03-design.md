# 03.03 — The design

The call-tree analysis in one chapter. Every later chapter in this folder
expands one part of it.

---

## The one-sentence design

> Walk each function body **once per distinct context**, carrying an
> environment. Resolve each callee from the environment first and from ty
> second. Answer a project call with the callee's **summary** for that
> context. Render the per-path tree from the summaries at the end.

```
            ┌──────────────────────────────────────────────┐
            │ Solver                                       │
            │  contexts → summaries, recursion groups,     │
            │  cost limits                                 │
            └───────────────┬──────────────────────────────┘
                            │ get(context)
            ┌───────────────▼──────────────────────────────┐
            │ Walk (one body, one context)                 │
            │  Env: names → values, object attributes      │
            └───────┬───────────────────────┬──────────────┘
          env hit ◄─┤                       │ env miss
                    │                       ▼
                    │       ┌──────────────────────────────┐
                    │       │ ty (the oracle)              │
                    │       │  inferred types, member, MRO │
                    │       └──────────────┬───────────────┘
                    │                      ▼
                    │       ┌──────────────────────────────┐
                    │       │ salsa: parsed module,        │
                    │       │ semantic index               │
                    │       └──────────────────────────────┘
```

The rule inside one walk:

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>) -> Values<'db> {
    self.resolve_from_env(expr, env)                    // precise, path-aware
        .unwrap_or_else(|| self.resolve_from_ty(expr))  // fast, path-free
}
```

Precision where the walk has information. ty everywhere else.

---

## Why summaries

A context-sensitive call tree is exponential in depth. If the average function
makes 3 project calls:

| depth | nodes |
|---:|---:|
| 5 | 243 |
| 10 | 59,049 |
| 15 | 14,348,907 |
| 20 | 3.5 × 10⁹ |

The Jedi driver survives only because it is slow and stops early
(`per_function_execution_limit`). A fast Rust port reaches the deep parts.

A summary records what one call did, relative to its inputs. Two calls with the
same inputs get the same summary. Work becomes the number of distinct contexts
times body size. Paths only appear when the tree is rendered, and rendering is
bounded.

This is the functional approach to interprocedural analysis (Sharir and Pnueli,
1981). Jarvis reuses a function's type graph when argument types match. Infer
Pulse keeps per-procedure pre/post summaries. Pysa computes per-function
summaries. Pyright infers call-site return types from argument types. None of
them keep an exploded per-path tree as the main artifact.

---

## The rules

1. **A summary is a function of its key.** The walk for a context starts from
   what the key holds, and from nothing else. If the callee needs something
   the key does not hold, it gets `Unknown`, its annotation, or ty's answer.
2. **Nothing walk-local leaves a summary.** No `OriginId`, `ActivationId`,
   `ValueId` or `DeferId` appears in a `FunctionResult`. Values are written
   relative to the inputs. The types enforce it
   ([`09`](09-summaries.md#values-relative-to-the-inputs)).
3. **Precision depends only on the key and the code. Cost limits only
   truncate.** Every rule that widens a value is decided by the key and the
   code. The key-side rules live in `KeyPolicy`; the walker's fan-out caps act
   on values the walk computed from its key. Limits on stack depth, context
   count and time never widen. They produce an `Incomplete(Cut)` result, which
   is marked in the output and never reused.

Rule 1 makes reuse safe across callers. Rule 2 makes it safe across requests,
and later as a salsa query. Rule 3 keeps a summary independent of the order its
context was reached in. Rule 1 needs that, and so does the cross-request cache
in v1.1.

The no-reuse mode tests whether reuse changes the graph
([`15`](15-checks-and-build-order.md#no-reuse-mode)).

---

## Precision policy

The goal is a useful graph of possible calls, with known gaps. `Finished` means
finished under this policy. It does not mean every call Python could make has
been found.

Two choices are easy to confuse, and they give different graphs:

- **Ignoring branch conditions** combines the arms of a branch. Every arm is
  possible.
- **Ignoring assignment order** would combine every value ever assigned to a
  name.

v1 makes the first choice and not the second:

```python
w = JsonWriter()
w.write(1)        # v1: JsonWriter.write only
w = CsvWriter()
w.write(2)        # v1: CsvWriter.write only
# Ignoring assignment order would show both writes at both calls.
```

**Union at branches, precision at bindings.** Deciding which branch runs needs
comparisons, which need integers, which is a symbolic executor.

| Python feature | v1 policy |
|---|---|
| `if`, `match`, ternary, `and` / `or` | Conditions are not evaluated. Every arm starts from the state before the branch, and the arms are joined after it |
| assignment order | Respected. On one path, a later assignment replaces an earlier one. A value assigned in one arm is not visible in another arm |
| loops | Body walked once, joined with "did not run". A value set on a second iteration is not seen |
| attribute writes | Replace the old value only when exactly one object can be the target and the write happens on every path. Otherwise joined with the old value |
| writes to a caller's object | Reported to the caller as an effect at the object's path, up to `max_path` steps. Deeper: a weak `Unknown` effect at the deepest path that fits, and the summary is marked |
| recursion | Each body walked once per context. A recursive call is recorded, and the cycle's values are found by repeating only the deferred operations until nothing changes |
| effects of a call inside a recursion cycle | Joined into the exit state. Statements after the call did not see them |
| containers, tuple destructuring, `*args`, `global` / `nonlocal` | Not modelled. `Unknown` |
| decorators | Transparent. The `def`'s own function is used |
| generators | A call returns `Unknown`. The body is still walked |
| `await` | Transparent |

The walker, the summaries and the fixtures all follow this table. Changing a row
is a change of policy, and the fixtures that pin that row change with it.

### Joins and object identity

A join is union plus dedup. The only iteration anywhere is inside a recursion
group ([`12`](12-recursion.md)), and it uses the same join.

| where | what is joined |
|---|---|
| a branch merge (`Env::join`) | the arms' locals, attributes and must-write flags |
| the end of a walk | the env at every exit |
| returns | the values of every `return` reached |
| a call site | the targets of every callee and every combo |

Objects keep their identity through every join:

- Values are deduplicated by full equality. Two instances are equal only when
  their origins are equal, so two `JsonHandler` objects built at different
  sites stay two values.
- In a summary, each escaping object is its own `objects[j]`, and every
  reference to it is `Obj(j)`. Two objects are never merged because they share
  a class.
- In the caller, `Obj(j)` maps to one origin per `(site, t, combo, j)`, and
  every reference to `Obj(j)` maps to that same origin.
- Fingerprints are the one place where two objects can look the same, on
  purpose. Two objects with the same class and attributes share a fingerprint,
  which is how summaries are shared. Identity across a call is carried by
  `alias` and `Ref` paths, never by fingerprints.

---

## Pipeline

```
1. sem       ty: parse, semantic index, types               (incremental already)
2. collect   functions, classes                             (existing stage)
3. ids       docstring IDs                                  (existing stage)
4. entry     one Context per requested function: params from annotations, else Unknown
5. solve     Solver::get(entry): walk each body once per context, resolve recursion groups
6. graph     contexts + call edges, with stable IDs
7. render    the per-path tree, for the current wire contract
```

pylspt's output is the graph (step 6). The tree (step 7) is a view of it. The
response carries both ([`14`](14-output.md)).

Calls are found by evaluation reaching them in step 5. There is no separate pass
that collects call expressions first.

---

## Modules

```
src/
├── sem/             the only code that imports the vendored ty crates or types/vnoc.rs
├── interp/
│   ├── value.rs     AbstractValue, Values, lift, cap_fanout          → 04
│   ├── env.rs       Env, activations, join, must-write tracking      → 05
│   ├── walk.rs      walk_stmts, eval, exec_simple                    → 05
│   ├── bind.rs      bind_parameters, eval_arguments                  → 06
│   ├── member.rs    member, bind_attribute                           → 07
│   ├── key.rs       Fp, Context, KeyPolicy, fingerprint, materialize → 08
│   ├── summary.rs   FunctionResult, SVal, PlainVal, Effect, finish   → 09
│   ├── call.rs      eval_call, call_project, resolve and apply, constructors → 10
│   ├── defer.rs     pending values, deferred operations, replay      → 12
│   └── solver.rs    Solver, slots, groups, fixpoint, cuts            → 12, 13
├── stable.rs        StableDef, StableType, StableContextId, canonical key encoding → 14
├── graph.rs         graph output                                     → 14
└── render.rs        tree view                                        → 14
```

---

## Where ty gets consulted

| question | answered by |
|---|---|
| callee is a bare `Name` not in the env | ty's inferred type, lifted |
| callee is `mod.func` where `mod` is an import | `member` on the module value |
| callee is `x.attr` and `x` is in the env | **your** `member` ([`07`](07-attributes-and-self.md)), with `vnoc::member` for the class side |
| argument is a literal, comprehension, f-string, stdlib call | `inferred_type`, lifted |
| argument is a name in the env | **your** env. Do not ask ty |
| what does `Foo()` construct | **your** constructor ([`10`](10-one-call.md#constructors)) |
| MRO and member lookup on a type | `vnoc::member` |
| an unknown parameter with an annotation | the annotation, lifted ([`08`](08-keys.md#materializing)) |

Rule of thumb: **ty is asked about things that don't depend on the path.** The
moment path information exists, the env owns the answer.

---

## Eager arguments

Jedi is lazy: `TreeArguments` stores unevaluated expressions plus the caller's
context and evaluates them on demand. pylspt is eager: at the call site, every
argument is evaluated once to `Values` in the caller's env.

| | lazy (Jedi) | eager (pylspt) |
|---|---|---|
| unused args | free | evaluated anyway |
| args used N times | re-evaluated | evaluated once |
| recursion guards | needed | not needed |
| deep chains | context chain walk per lookup | flat map lookup |
| in Rust | self-referential contexts, lifetimes | straightforward |

Transitive pass-through (`outer → middle → inner`) still works. At each call
site the argument is evaluated in the current env, which already holds the value
from the level above. The chain is flattened at bind time.

---

## The contract is the output, not the structure

Much of `call_resolver.py` is calls into Jedi: `create_context`,
`helpers.infer`, `as_context(arguments)`, `TreeInstance`, `BoundMethod`. That
shape is Jedi's, not a design to copy.

**What must match: the JSON.** Same nodes, same tree shape, same `target_qname`
and `target_id`, same skip decisions, and the same saved tree after v-noc's
merge. Everything else is pylspt's to design. Where pylspt's output differs on
purpose (recursive leaves, values that now flow), the difference is listed in
[`15`](15-checks-and-build-order.md#parity-with-the-jedi-driver).

---

## Invariants

1. **Envs are persistent and cheap to clone.** `rpds` maps. A call snapshot is
   a pointer copy.
2. **Every fallible step returns "no result", never an error.** A partial
   graph is a valid answer.
3. **Values are small and `Copy`.** `AbstractValue` is an enum over salsa
   handles and walk-local ids, not owned data.
4. **Nothing walk-local outlives its walk**, except inside a recursion group,
   whose walks are kept until the group is resolved.
5. **Every limit is counted.** A missing edge should be explainable from a
   number in `Stats`, not from a debugger session.

---

→ Next: [`04-values.md`](04-values.md)
