# 03.03 — The abstract interpreter: architecture

The core design. Everything in this folder elaborates one part of it.

---

## The one-sentence design

> Walk the AST like `outgoing_calls` does, but carry an **environment** mapping
> names to abstract values; resolve each callee **from the environment first**,
> falling back to ty; bind arguments into a **child environment** and recurse.

```
                      ┌─────────────────────────────────────┐
                      │        your interpreter             │
                      │  Env: name → AbstractValue          │
                      │  Frame stack, cycle guard, budget   │
                      └──────────────┬──────────────────────┘
              env hit ◄──────────────┤
                                     │ env miss
                                     ▼
                      ┌─────────────────────────────────────┐
                      │        ty (the oracle)              │
                      │  inferred_type, definitions_for_*,  │
                      │  MRO, member lookup, signatures     │
                      └──────────────┬──────────────────────┘
                                     ▼
                      ┌─────────────────────────────────────┐
                      │  salsa: parsed_module, semantic_idx │
                      └─────────────────────────────────────┘
```

The two-line rule that makes it work:

```rust
fn resolve(&self, expr: &ast::Expr, env: &Env<'db>) -> AbstractValue<'db> {
    self.resolve_from_env(expr, env)                    // precise, path-aware
        .unwrap_or_else(|| self.resolve_from_ty(expr))  // fast, path-free
}
```

**Precision where you have information; speed everywhere else.**

---

## Module layout

```
src/
├── main.rs
├── rpc/                 JSON-RPC surface (mirrors rpc.py 1:1)
├── db.rs                ProjectDatabase ownership, lifecycle, sync
├── syntax/
│   ├── nodes.rs         BaseNode/ClassNode/FunctionNode/CallNode + serde
│   ├── scan.rs          the parser.py port  (02-mapping/01)
│   ├── ids.rs           the id_injector.py port (02-mapping/02)
│   └── position.rs      LineIndex bridge
├── analysis/
│   ├── mro.rs           the mro_resolver.py port (02-mapping/03)
│   └── qname.rs         qualified-name formatting (parity-critical)
└── interp/              ★ THE PROJECT ★
    ├── value.rs         AbstractValue                    → 04-value-domain
    ├── env.rs           Env, scope chain, immutable maps
    ├── bind.rs          call site → child Env            → 05-binding-arguments
    ├── member.rs        attribute lookup on a value      → 06-attributes-and-self
    ├── eval.rs          expression → AbstractValue
    ├── walk.rs          body traversal (from OutgoingCallsFinder)
    ├── frame.rs         CallFrameStack, dedup, call_count
    └── budget.rs        cycles, depth, work limits       → 08-termination
```

---

## The main loop

```rust
pub struct Interp<'db> {
    db: &'db dyn Db,
    project_root: &'db SystemPath,
    budget: Budget,
    stats: Stats,
}

struct Frame<'db> {
    qname: String,
    target_id: String,
    /// Parent chain, for the `is_ancestor` cycle guard.
    parent: Option<&'db Frame<'db>>,
    children: Vec<FrameNode>,
}

impl<'db> Interp<'db> {
    /// Mirrors `resolve_call_hierarchy_for_node`.
    fn resolve_call(
        &mut self,
        call: &ast::ExprCall,
        env: &Env<'db>,           // ← the thing Jedi calls `parent_context`
        model: &SemanticModel<'db>,
        frame: &mut Frame<'db>,
    ) {
        if !self.budget.tick() { return; }

        // 1. builtin fast-path, BY NAME, before any work  (call_resolver.py:114)
        if let ast::Expr::Name(n) = call.func.as_ref() {
            if is_builtin_name(n.id.as_str()) && !env.contains(n.id.as_str()) {
                return;
            }
        }

        // 2. resolve the callee — ★ env first, ty second
        let callees = self.eval_callee(&call.func, env, model);

        // 3. eval arguments ONCE, in the CALLER's env  (call_resolver.py:168)
        let args = self.eval_arguments(&call.arguments, env, model);

        let mut seen: FxHashSet<&str> = FxHashSet::default();

        for callee in callees.iter() {
            let Some(target) = callee.as_callable_definition(self.db) else { continue };

            if !self.is_project_code(target) { continue }              // step (a)
            let Some(qname) = self.qname(target) else { continue };    // step (b)
            if !seen.insert(&qname) { continue }                       // step (c)
            let Some(id) = self.docstring_id(target) else { continue };// step (d)
            if frame.is_ancestor(&qname) { continue }                  // step (e)

            match target.kind() {
                Callable::Function(func) => {
                    let child = frame.add_child(qname, format!("FunctionSchema/{id}"));
                    // ★★★ THE CONTEXT ★★★  — this is `as_context(arguments)`
                    let callee_env = self.bind_parameters(func, &args, callee.receiver());
                    self.walk_body(func, &callee_env, child);
                }
                Callable::Class(class) => {
                    let child = frame.add_child(qname, format!("ClassSchema/{id}"));
                    if let Some(init) = self.lookup_init(class) {
                        let instance = AbstractValue::instance_of(class, &args);
                        let init_env = self.bind_parameters(
                            init, &args, Some(instance),   // `self` ← the new instance
                        );
                        self.walk_body(init, &init_env, child);
                    }
                }
            }
        }
    }

    /// Mirrors `_analyze_function`.
    fn walk_body(&mut self, func: FunctionDef<'db>, env: &Env<'db>, frame: &mut Frame<'db>) {
        // Reuse ty_ide's OutgoingCallsFinder traversal: stops at nested callables,
        // covers decorators / defaults / annotations / base-class exprs.
        for call in collect_calls_in_scope(func) {
            self.resolve_call(call, env, &model_for(func), frame);
        }
    }
}
```

Compare to the pseudocode in [`01-what-jedi-actually-does.md`](01-what-jedi-actually-does.md).
The steps line up one-to-one — **as a scaffold, not a contract.**

> ### The contract is the output, not the structure
>
> Much of `call_resolver.py` is calls *into Jedi*, not logic the driver owns:
> `parent_context.create_context(leaf)`, `helpers.infer(...)`,
> `as_context(arguments)`, `TreeInstance(...)`, `BoundMethod(...)`. Those are
> Jedi's machinery. There is no reason for the Rust version to have the same
> shape, and several reasons for it not to — ty's idioms (salsa queries, memo
> tables with cycle recovery, `Definition` handles) are better fits than
> transliterated Python.
>
> **What must match: the JSON.** Same nodes, same tree shape, same
> `target_qname` / `target_id` / `call_count`, same skip decisions.
> Everything else is yours to design.
>
> Keeping the step numbering early is still useful — it makes the first parity
> failures localisable. Drop it as soon as it stops helping.

---

## Eager vs lazy arguments

Jedi is lazy: `TreeArguments` stores unevaluated expressions plus the caller's
context, and evaluates on demand.

**Be eager.** At the call site, evaluate every argument expression to an
`AbstractValue` and store the values.

| | Lazy (Jedi) | Eager (you) |
|---|---|---|
| Unused args | free | wasted work |
| Args used N times | re-evaluated N times (Jedi memoises partially) | evaluated once |
| Recursion guards | needed, complex | not needed |
| Deep chains | context chain walk per lookup | flat map lookup |
| Implementation | hard in Rust (self-referential contexts, lifetimes) | straightforward |

Eager wins on all the axes you care about. Arguments are typically 0–3 simple
expressions; evaluating them unconditionally is cheap, and it converts Jedi's
transitive context-chain walk into an O(1) hash lookup.

**Important consequence:** transitive pass-through
(`outer→middle→inner`) still works, because at each call site the argument
expression is evaluated in the *current* env, which already contains the
resolved value from the level above. The chain is flattened at bind time
rather than walked at lookup time. Fixture #2 in
[`01`](01-what-jedi-actually-does.md#the-invariants-to-test-against) covers this.

---

## Where ty gets consulted

| Interpreter question | ty answers with |
|---|---|
| callee is a bare `Name` not in env | `definitions_for_name` |
| callee is `mod.func` where `mod` is an import | `definitions_for_imported_symbol` |
| callee is `x.attr` and `x` **is** in env | **your** `member.rs` + ty's MRO |
| callee is `x.attr` and `x` is **not** in env | `definitions_for_attribute` |
| argument is a literal / comprehension / f-string / stdlib call | `inferred_type` |
| argument is a name in env | **your** env — do not ask ty |
| what does `Foo()` construct | `Type::NominalInstance` via your own `instance_of` |
| MRO for member lookup | `ClassLiteral::iter_mro` |
| does this call match this signature | `resolved_call_signature` |

Rule of thumb: **ty is asked about things that don't depend on the path.**
The moment path information exists, your env owns the answer.

---

## Invariants to hold

1. **Environments are immutable and cheaply cloned.** Use `im`/`rpds` persistent
   maps, or `Arc<Env>` with a parent pointer and copy-on-write. You will create
   one per call site; deep-cloning a `HashMap` per frame will dominate your
   profile.
2. **Never mutate an env after a child sees it.** Frames share structure.
3. **Every `resolve_call` decrements a global budget.** No exceptions.
4. **Every fallible step returns "no result", never an error.** Matches the
   Python's swallow-everything behaviour, and a partial tree is a valid answer.
5. **Values are `Copy`-cheap.** `AbstractValue` should be small — an enum over
   salsa handles, not owned data. Passing it by value everywhere is the ergonomic
   default.

---

→ Next: [`04-value-domain.md`](04-value-domain.md)
