# 01.08 — `*args`, `**kwargs` and splats

**You build:** the two places where argument alignment stops being knowable:
variadic parameters, and `*` / `**` at the call site. Both bind `Unknown`.
**Fixture:** `python/varargs.py`
**Plan:** `plan/03-call-tree/06-binding-arguments.md` § "Five things that are
easy to get wrong", points 3 and 4

---

## The fixture

```python
def collect(*fns):
    for f in fns: f()
def by_name(**fns): fns["x"]()
def two(x, y): y()
def after_star(*rest, cb): cb()

def main():
    collect(a, b)            # collect, no children
    by_name(x=a)             # by_name, no children
    items = [a, b]
    two(*items)              # two, no children
    two(a, b)                # two > b
    two(**{"x": a, "y": b})  # two, no children
    after_star(a, cb=b)      # after_star > b

# expect:
# main
#   collect
#   by_name
#   two
#   two
#     b
#   two
#   after_star
#     b
```

## Predict

1. `two(*items)` with `items = [a, b]`: Python calls `b`. Why does the
   expected tree show no child?
2. Which is worse for a v-noc user: a missing edge, or an edge to the wrong
   function? Why?
3. In `after_star(a, cb=b)`, where does `a` go?

---

## Build

### Decision 1: variadic parameters are `Unknown`

`*fns` and `**fns` bind `[Unknown]`. Calls made through them get no edge. The
body is still walked, so every other call in it is found.

**Why:** a tuple or a dict of values is a container, and containers are not
modelled (`plan/03-call-tree/04-values.md` § "What is not modelled"). Modelling
`*args` as a tuple of the passed values is possible, and is listed for later
(`15` § "Growth path", v1.2). Measure how often it costs an edge on real code
first. Most `**kwargs` in application code is passed straight to a framework.

### Decision 2: a splat at the call site makes every parameter `Unknown`

```rust
ArgOrKeyword::Arg(Expr::Starred(st)) => {
    self.eval(&st.value, env);       // still evaluated: calls inside are found
    out.has_splat = true;
}
ArgOrKeyword::Keyword(kw) if kw.arg.is_none() => {   // **d
    self.eval(&kw.value, env);
    out.has_splat = true;
}
```

and in `bind_parameters`, first thing:

```rust
if args.has_splat {
    for name in all_parameter_names(def) { env.bind(name, smallvec![AbstractValue::Unknown]); }
    return env;
}
```

**Why every parameter:** after `*items` you don't know how many positionals
there are, so you don't know which parameter any *later* argument lands on
either. Guessing an alignment gives a tree that is confidently wrong: an edge
Python never takes, with nothing to say it is a guess. `Unknown` gives a
missing edge, which a fixture can pin and a counter can measure. **An
imprecise tree is better than a wrong one.** That rule comes back in every
part.

### Decision 3: keyword-only parameters after `*rest` still bind

`after_star(a, cb=b)`: `a` goes into `*rest` (Unknown), and `cb` is
keyword-only, matched by name. Leftover positionals never shift into
keyword-only parameters, which is Python's own rule.

### Yours

- `has_splat` in `EvaluatedArgs`, set by `*x` and `**x` at the call site.
- `bind_parameters`: the splat rule first; then bind `params.vararg` and
  `params.kwarg` names to `Unknown` after the others.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/bind.rs (changed parts)
#[derive(Clone, Default)]
pub struct EvaluatedArgs<'db> {
    pub positional: SmallVec<[Values<'db>; 4]>,
    pub keywords: SmallVec<[(Name, Values<'db>); 2]>,
    pub has_splat: bool,
}

pub fn eval_arguments(&mut self, args: &Arguments, env: &mut Env<'db>) -> EvaluatedArgs<'db> {
    let mut out = EvaluatedArgs::default();
    for arg in args.iter_source_order() {
        match arg {
            ArgOrKeyword::Arg(Expr::Starred(st)) => {
                self.eval(&st.value, env);
                out.has_splat = true;
            }
            ArgOrKeyword::Arg(e) => {
                let v = self.eval(e, env);
                out.positional.push(v);
            }
            ArgOrKeyword::Keyword(kw) => {
                let v = self.eval(&kw.value, env);
                match &kw.arg {
                    Some(name) => out.keywords.push((name.id.clone(), v)),
                    None => out.has_splat = true,
                }
            }
        }
    }
    out
}

pub fn bind_parameters(&mut self, def: &StmtFunctionDef, args: &EvaluatedArgs<'db>) -> Env<'db> {
    let mut env = Env::default();
    let params = &def.parameters;
    let unknown = || smallvec![AbstractValue::Unknown];

    if args.has_splat {
        for p in params.iter() {               // [check] ruff's iterator over every parameter
            env.bind(&p.name().id, unknown());
        }
        return env;
    }

    // … positional and keyword-only, as in step 07 …

    if let Some(p) = &params.vararg {
        env.bind(&p.name.id, unknown());
    }
    if let Some(p) = &params.kwarg {
        env.bind(&p.name.id, unknown());
    }
    env
}
```

</details>

---

## Run

```rust
#[test]
fn p01_08_varargs() {
    check("01-functions/python/varargs.py");
}
```

---

## Where you are

```
EvaluatedArgs { positional, keywords, has_splat }
```

Binding handles every parameter kind except the receiver (part 03) and
decorators that change a signature (step 12).

---

## Exercise

1. `def f(a, *rest): rest[0]()` called as `f(x, y)`. What does Python call?
   What does the tree show, and which decision explains the gap?
2. Why is `has_splat` checked before anything else in `bind_parameters`, and
   not only for the parameters after the splat?
3. Where would you count how often decision 1 costs an edge on a real project?

<details><summary>Answers</summary>

1. Python calls `y`. The tree shows `f` with no child: `rest` is `Unknown`
   (decision 1), so `rest[0]` is `Unknown`, and the call has no target.
2. A splat can stand for any number of positionals, including zero, so even
   the parameters *before* its position are uncertain once keywords and
   defaults are considered: `f(*xs, b=1)` may or may not fill `a`. All of them
   are `Unknown`.
3. A counter in the stats, incremented wherever a call's callee is `Unknown`
   because it came from a variadic parameter or a splat. Part 05 collects every
   counter like it in `Stats`, so a missing edge can be explained by a number.

</details>

---

→ Next: [`09-branches-and-loops.md`](09-branches-and-loops.md)
