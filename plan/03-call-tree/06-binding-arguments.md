# 03.06 — Binding arguments

The replacement for Jedi's `as_context(arguments)`:

```python
# call_resolver.py:166-179
arguments = self.create_args(callee_for_args, trailer, inference_state, call_context)
function_context = callee_for_args.as_context(arguments)
```

In pylspt, binding matches call-site values to parameters, **in the caller's
terms**. It does not build the callee's env. The bound values become the
callee's key ([`08`](08-keys.md)), and the callee's env is built from the key.

---

## Evaluating arguments at the call site

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EvaluatedArgs<'db> {
    positional: SmallVec<[Values<'db>; 4]>,
    keywords:   SmallVec<[(Name, Values<'db>); 2]>,
    has_splat:  bool,
}

fn eval_arguments(s: &mut Solver<'db>, w: &mut Walk<'db>, args: &ast::Arguments, env: &Env<'db>)
    -> EvaluatedArgs<'db>
{
    let mut out = EvaluatedArgs::default();
    for arg in args.args.iter() {
        match arg {
            ast::Expr::Starred(st) => { eval(s, w, &st.value, env); out.has_splat = true; }
            e => out.positional.push(eval(s, w, e, env)),
        }
    }
    for kw in args.keywords.iter() {
        let v = eval(s, w, &kw.value, env);
        match &kw.arg {
            Some(name) => out.keywords.push((name.id.clone(), v)),
            None       => out.has_splat = true,      // **kwargs at the call site
        }
    }
    out
}
```

Evaluated in the caller's env, left to right, each once. A splat's expression
is still evaluated, so calls inside it are reached.

---

## The binding function

```rust
/// One entry per parameter of `func`, in order: receiver first, defaults filled.
pub struct Bound<'db> {
    pub slots: SmallVec<[Values<'db>; 4]>,
}

fn bind_parameters(
    s: &Solver<'db>,
    func: Definition<'db>,
    args: &EvaluatedArgs<'db>,
    receiver: Option<AbstractValue<'db>>,   // Some(_) for a bound method, the class for a classmethod
    defining: Option<ActivationId>,         // the closure's activation, for its defaults
    w: &Walk<'db>,
) -> Bound<'db> {
    let params = s.parameters(func);                  // from the def's AST node
    let mut slots = SmallVec::new();
    if args.has_splat || s.decorator_changes_signature(func) {
        return Bound { slots: params.iter().map(|_| smallvec![AbstractValue::Unknown]).collect() };
    }
    let mut positional = args.positional.iter();
    let mut params_iter = params.iter();

    // 1. the receiver goes to the FIRST parameter, before any positional
    if let Some(recv) = receiver {
        if params_iter.next().is_some() { slots.push(smallvec![recv]); }
    }

    // 2. positional-only and positional-or-keyword
    for param in params_iter.by_ref() {
        if param.is_vararg() || param.is_kwarg() || param.is_keyword_only() { /* step 3 */ break }
        slots.push(
            args.keyword(param.name()).cloned()
                .or_else(|| positional.next().cloned())
                .or_else(|| default_value(s, w, func, param, defining))
                .unwrap_or_else(|| smallvec![AbstractValue::Unknown]),
        );
    }

    // 3. keyword-only: keywords or defaults. *args and **kwargs: Unknown.
    // ...
    Bound { slots }
}
```

### Five things that are easy to get wrong

**1. Receiver order.** Bind the receiver to the first parameter, then match the
remaining positionals against the rest. Off by one here shifts every argument
in every method call, and looks like broken inference.

**2. Defaults belong to the defining scope.**

```python
DEFAULT = JsonWriter()
def emit(w=DEFAULT): w.write(1)
```

`DEFAULT` is not in the caller's env. A default runs when the `def` runs, and
the walk that ran it already evaluated it and recorded its calls
([`05`](05-the-walk.md#straight-line-statements)). Binding never evaluates a
call again. `default_value` is:

- a module-level function: ty's type of the default expression, lifted. A
  name is fine; a call is `Unknown`, because ty types a call by the callee's
  *annotated* return type and never infers one (`mdtest/function/return_type.md`),
  and the module body is not walked;
- a nested def: the value the walk computed when the `def` statement ran.
  The `def` arm evaluates each default anyway (for its calls), and keeps the
  values with the closure, keyed by `(func, activation)`. A closure that
  crosses a call carries them in `Fp::Closure` and `SVal::Closure`, like its
  free values. This is Python's own rule (a default is evaluated once, when
  the `def` runs), and it doesn't depend on ty's return types.

Getting this backwards fails quietly: the caller's env misses the name, ty
answers, and the bug hides until a caller shadows the name.

**3. `*args` / `**kwargs`.** Bind them to `Unknown`. Calls made through them lose
precision; the body is still walked. Measure before modelling more. Most
`**kwargs` in application code is pass-through to a framework.

**4. A splat at the call site** (`emit(*items)`): the alignment is unknown. Bind
every parameter to `Unknown`. A wrong alignment gives a confidently wrong tree,
which is worse than an imprecise one.

**5. Decorated functions.** The `def`'s own parameters are the undecorated
signature. Callee resolution treats decorators as transparent
([`11`](11-closures-and-callbacks.md#decorators)), but a decorator that changes
arity would shift the binding. `decorator_changes_signature` is true for a
decorator not on a small allow-list (`functools.wraps`-style project
decorators, `lru_cache`, `cache`), and then every parameter is `Unknown`.
**[check]** the allow-list against the golden files, and count how often it
fires. The receiver rules must be handled:

| decorator | receiver |
|---|---|
| none (method) | first parameter ← the instance |
| `@staticmethod` | none. Positionals start at parameter 0 |
| `@classmethod` | first parameter ← the class, not an instance |
| `@property` | accessed without `()`, the getter is called by `member` ([`07`](07-attributes-and-self.md#properties)) |

### Bound call and unbound call

```python
Greeter().greet(Payload())              # self ← Greeter(), name ← Payload()
Greeter.greet(Greeter(), Payload())     # the same binding
```

The bound call passes the receiver from the `BoundMethod`. The unbound call
passes no receiver; `Greeter()` is the first positional and lands on `self`
by position. Same result, different route. The unbound case checks that the
receiver rule is about position, never about the name `self`.

---

## Fixtures

```python
def f(a, b=DEFAULT, *rest, kw=None, **extra): ...

f(X())                      # a←X, b←DEFAULT, kw←None, rest/extra←Unknown
f(X(), Y())                 # a←X, b←Y
f(b=Y(), a=X())             # keywords out of order
f(X(), kw=Z())              # keyword-only
f(*items)                   # splat → every parameter Unknown
obj.m(X())                  # self←obj, a←X          ← the off-by-one test
Cls.m(obj, X())             # unbound: self←obj explicitly
S.static(X())               # @staticmethod: a←X, NOT self←X
C.klass(X())                # @classmethod: cls←C, a←X
```

Nine cases, table-driven, microseconds each. Write them first. The closure
fixtures are in [`11`](11-closures-and-callbacks.md#fixtures).

---

→ Next: [`07-attributes-and-self.md`](07-attributes-and-self.md)
