# 03.05 — Binding arguments: the replacement for `as_context()`

The single most important function in the project.

```python
# what you're replacing — call_resolver.py:166-179
arguments = self.create_args(callee_for_args, trailer, inference_state, call_context)
function_context = callee_for_args.as_context(arguments)
```

---

## The environment

```rust
/// Immutable, structurally shared. One per activated call frame.
#[derive(Clone)]
pub struct Env<'db> {
    /// Local bindings for this frame: parameter names → values.
    locals: rpds::HashTrieMap<Name, Values<'db>>,

    /// Attribute store for instances constructed on this path. → 06
    attrs: rpds::HashTrieMap<(OriginId, Name), Values<'db>>,

    /// The scope these locals belong to. Guards against a name from frame A
    /// leaking into frame B when both happen to use `x`.
    scope: ScopeId<'db>,

    /// Enclosing frame's env, for closures over enclosing-function names.
    parent: Option<Arc<Env<'db>>>,
}
```

**Why persistent maps (`rpds` / `im`) rather than `FxHashMap` + clone:** you
create one `Env` per call site, and frames share most of their content. `rpds`
insert is O(log n) with structural sharing — cloning is a pointer copy. A
`FxHashMap` clone is O(n) allocation, and at your call volume that is the whole
profile. This is not premature optimisation; it is the difference between the
port being fast and being embarrassing.

**Why `scope` is a field:** without it, `x` bound in `outer` would be visible in
an unrelated callee that also names a local `x`. Look-ups must check the scope
matches, or walk `parent` only for genuine closure captures. This is the bug you
will hit around week two if you skip it.

---

## The binding function

```rust
fn bind_parameters(
    &self,
    func: FunctionDef<'db>,
    args: &EvaluatedArgs<'db>,
    receiver: Option<AbstractValue<'db>>,   // Some(_) for methods
    caller_env: &Env<'db>,
) -> Env<'db> {
    let mut locals = rpds::HashTrieMap::new();
    let params = func.parameters(self.db);

    let mut positional = args.positional.iter();

    // 1. `self` / `cls` — bind FIRST, before consuming positionals.
    let mut params_iter = params.iter();
    if let Some(recv) = receiver {
        if let Some(first) = params_iter.next() {
            locals.insert_mut(first.name().clone(), smallvec![recv]);
        }
    }

    // 2. positional-only + positional-or-keyword
    for param in params_iter.by_ref() {
        if param.is_vararg() || param.is_kwarg() || param.is_keyword_only() { break }

        // keyword wins over positional if both somehow present
        if let Some(v) = args.keywords.get(param.name()) {
            locals.insert_mut(param.name().clone(), v.clone());
        } else if let Some(v) = positional.next() {
            locals.insert_mut(param.name().clone(), v.clone());
        } else if let Some(default) = param.default() {
            // ★ defaults evaluate in the DEFINING scope, not the caller's
            let v = self.eval(default, &self.env_of_defining_scope(func));
            locals.insert_mut(param.name().clone(), v);
        } else {
            locals.insert_mut(param.name().clone(), smallvec![AbstractValue::Unknown]);
        }
    }

    // 3. keyword-only params: keywords or defaults only
    for param in params_iter { /* same, minus positional */ }

    // 4. *args / **kwargs — see below
    // ...

    Env {
        locals,
        attrs: caller_env.attrs.clone(),   // ★ carry object state forward. → 06
        scope: func.body_scope(self.db),
        parent: self.closure_parent(func, caller_env),
    }
}
```

### Five things that are easy to get wrong

**1. `self` binding order.** Bind the receiver to the *first* parameter, then
match remaining positionals against the *rest*. Off-by-one here shifts every
argument in every method call — and it will look like your inference is broken
rather than your binding.

**2. Defaults evaluate in the defining scope.**
```python
DEFAULT = JsonWriter()
def emit(w=DEFAULT): w.write(1)
```
`DEFAULT` is not in the caller's env. Evaluate defaults against the module scope
of the function's own file. Getting this wrong silently produces `Unknown`,
which is the failure mode you'll never notice — it just quietly loses precision.

**3. `*args` / `**kwargs`.**
Your Jedi version delegates to `TreeArguments`, which does model these. Options,
in increasing order of effort:
- **(a)** bind them to `Unknown`. Calls made *through* `*args` lose precision;
  the body is still walked. **Start here.**
- **(b)** bind `*args` to a `Values` of the leftover positionals, so
  `args[0].run()` can resolve if you also model constant subscripts.
- **(c)** full splat modelling. Not worth it.

Do (a), measure how often real code hits it, then decide. Most `*args`
forwarding in application code is `**kwargs` pass-through to a framework — i.e.
already leaving project code.

**4. Star-args at the call site** (`emit(*items)`): the arity mapping becomes
unknown. Bind **all** remaining parameters to `Unknown` rather than guessing an
alignment. A wrong alignment produces a confidently wrong tree, which is worse
than an imprecise one.

**5. Decorated functions.** `func.parameters()` is the *undecorated* signature.
If the decorator changes arity (`functools.partial`-like), bindings shift.
Detect a non-trivial decorator list and fall back to binding everything
`Unknown`. Cheap insurance. Note that `@staticmethod` / `@classmethod` /
`@property` change the receiver rule and *must* be handled:

| Decorator | Receiver binding |
|---|---|
| none (method) | first param ← instance |
| `@staticmethod` | **no receiver**; positionals start at param 0 |
| `@classmethod` | first param ← the *class*, not an instance |
| `@property` | accessed without `()`; see [`06`](06-attributes-and-self.md) |

---

## Evaluating arguments at the call site

```rust
struct EvaluatedArgs<'db> {
    positional: SmallVec<[Values<'db>; 4]>,
    keywords:   SmallVec<[(Name, Values<'db>); 2]>,
    has_splat:  bool,
}

fn eval_arguments(&self, args: &ast::Arguments, env: &Env<'db>, model: &SemanticModel<'db>)
    -> EvaluatedArgs<'db>
{
    let mut out = EvaluatedArgs::default();
    for arg in args.args.iter() {
        match arg {
            ast::Expr::Starred(_) => { out.has_splat = true; }
            e => out.positional.push(self.eval(e, env, model)),
        }
    }
    for kw in args.keywords.iter() {
        match &kw.arg {
            Some(name) => out.keywords.push((name.id.clone(), self.eval(&kw.value, env, model))),
            None       => { out.has_splat = true; }   // **kwargs at the call site
        }
    }
    out
}
```

**Evaluated in the caller's env** — that is what makes transitive pass-through
work (fixture #2). `emit(w)` where `w` is itself a parameter of the enclosing
function: `self.eval(w, env)` finds `w` in `env.locals` and returns the value
bound one level up. The chain flattens automatically.

## `eval` — the expression evaluator

Deliberately small. Handle what carries path information; delegate the rest.

```rust
fn eval(&self, expr: &ast::Expr, env: &Env<'db>, model: &SemanticModel<'db>) -> Values<'db> {
    match expr {
        // ★ env first — the whole point
        ast::Expr::Name(n) => {
            if let Some(v) = env.lookup(&n.id) { return v.clone() }
            lift(self.db, self.env(), n.inferred_type(model).unwrap_or(Type::unknown()))
        }

        // ★ construction: creates a NEW instance with a fresh origin
        ast::Expr::Call(c) => self.eval_call_result(c, env, model),

        // ★ attribute: dispatch on the value we have  → 06
        ast::Expr::Attribute(a) => {
            let recv = self.eval(&a.value, env, model);
            recv.iter().flat_map(|v| v.member(self.cx(), a.attr.as_str())).collect()
        }

        // ★ closures as values  → 07
        ast::Expr::Lambda(l) => smallvec![AbstractValue::Lambda(/* ... */)],

        // ★ keep alternatives separate
        ast::Expr::If(t) => {
            let mut v = self.eval(&t.body, env, model);
            v.extend(self.eval(&t.orelse, env, model));
            v
        }
        ast::Expr::BoolOp(b) => b.values.iter().flat_map(|e| self.eval(e, env, model)).collect(),

        // everything else: ty's problem
        other => lift(self.db, self.env(),
                      other.inferred_type(model).unwrap_or(Type::unknown())),
    }
}
```

Seven arms with real logic; one fallback. That is the whole evaluator. Resist
growing it — each new arm is a place your semantics can diverge from the Python
driver, and the fallback is already correct-if-imprecise.

### `eval_call_result` — the return value of a call

```python
w = make_writer()      # ← what is `w`?
w.write(1)
```

Three cases, in priority order:

1. **Constructor.** `Foo()` → `Instance { class: Foo, origin: fresh }`, never
   `Ty(NominalInstance)`. The source of every interesting `self` in the tree.
2. **Project function** → interpret its `return` statements with the bound env.
3. **Anything else** → ty's declared return type via `inferred_type`.

> ⚠ **Case 2 is not optional.** An earlier draft of this plan called it out of
> scope on the grounds that `call_resolver.py` has no explicit code for it.
> That was wrong: Jedi performs the inference natively, inside the execution
> context your `as_context(arguments)` built, so `w = make_writer(); w.write()`
> **resolves today**. Dropping it is a regression, not parity.
>
> Full design, with the recursion guard, depth cap, and memo table it needs:
> → [`10-return-values-and-state.md`](10-return-values-and-state.md)

---

## Test fixtures for this chapter

```python
def f(a, b=DEFAULT, *rest, kw=None, **extra): ...

f(X())                      # a←X inst, b←DEFAULT, kw←None
f(X(), Y())                 # a←X, b←Y
f(b=Y(), a=X())             # keywords out of order
f(X(), kw=Z())              # keyword-only
f(*items)                   # has_splat → everything Unknown
obj.m(X())                  # self←obj, a←X       ← the off-by-one test
Cls.m(obj, X())             # unbound call: self←obj explicitly
S.static(X())               # @staticmethod: a←X, NOT self←X
C.klass(X())                # @classmethod: cls←C, a←X
```

Nine cases. Write them first, as a table-driven test. They will catch every
binding bug you are going to write, and they run in microseconds.

---

→ Next: [`06-attributes-and-self.md`](06-attributes-and-self.md)
