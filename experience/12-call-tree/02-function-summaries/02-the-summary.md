# 02.02 — The summary

**You build:** summary mode, without reuse yet. Each call to a project
function gets its own walk, started from the key alone. The walk ends in a
`FunctionResult`, the callee's summary, and the caller takes its value from
the summary's returns.
**Fixture:** `python/summaries.py`, then every part 01 fixture except
`closures.py`
**Plan:** `plan/03-call-tree/09-summaries.md`, `08-keys.md` § "Materializing",
`03-design.md` § "The rules"

---

## The fixture

```python
def pick(c):
    if c: return a
    return b
def ident(f): return f
def chain(f): return ident(ident(f))
def nothing(): pass

def main():
    pick(1)()           # pick, a, b
    chain(a)()          # chain > ident, ident; a
    ident(pick(0))()    # pick, ident, a, b
    nothing()()         # nothing

# expect:
# main
#   pick
#   a
#   b
#   chain
#     ident
#     ident
#   a
#   pick
#   ident
#   a
#   b
#   nothing
```

## Predict

1. In summary mode, `chain`'s walk starts from its key. What does its env hold
   for `f`, and where did that come from?
2. `ident`'s summary returns "the function `a`". Write that as data, without
   using any id that only means something inside `ident`'s walk.
3. With no reuse yet, how many walks does summary mode do for `main`? The same
   as inline mode, or fewer?

---

## Build

### Decision 1: a walk starts from its key, and nothing else

```rust
// src/interp/key.rs
/// The starting env of a context's walk. Reads the key and the code. Nothing else.
pub fn materialize(&mut self, s: &Solver<'db>, ctx: ContextId, node: &StmtFunctionDef) -> Env<'db> {
    let mut env = Env::new(s.sem.body_scope(s.contexts[ctx].func, node), None);
    for (param, &fp) in parameter_names(node).zip(&s.contexts[ctx].args) {
        let vals = self.from_fp(s, fp);
        env.bind(param, vals);
    }
    env
}

fn from_fp(&mut self, s: &Solver<'db>, fp: FpId) -> Values<'db> {
    match &s.fps[fp] {
        Fp::Function(d) => smallvec![AbstractValue::Function(*d)],
        Fp::Module(t) => smallvec![AbstractValue::Module(*t)],
        Fp::Data(t) => smallvec![AbstractValue::Ty(*t)],
        Fp::Unknown => smallvec![AbstractValue::Unknown],
        Fp::Union(ids) => ids.iter().flat_map(|&i| self.from_fp(s, i)).collect(),
    }
}
```

**This is rule 1: a summary is a function of its key.** If the walk read
anything else from its caller, two callers with equal keys could get
different answers from the same walk, and reusing one walk for both (step 03)
would hand one caller the other's answer.

Inline mode built the callee's env from the caller's values
(`env_from_bound`). Summary mode builds it from the fingerprints. For
functions, the two differ only in literals (`Data(str)` instead of
`Literal["x"]`), which the walker never reads, so you can't make this rule
fail yet. Part 04's objects can: there the key holds an object's attributes
only two steps deep.

### Decision 2: request-wide state moves out of the walk

Until now there was one `Walk` for the whole request. In summary mode every
context gets its own walk, with its own frame and its own activations. What
must outlive one walk moves into a new struct, and every walker method gets
it as a parameter:

```rust
// src/interp/solver.rs
pub struct Solver<'db> {
    pub sem: Sem<'db>,
    pub opts: Options,
    pub fps: Interner<Fp<'db>, FpId>,
    pub contexts: Interner<Context<'db>, ContextId>,
    pub path: Vec<String>,          // qualified names being walked: the recursion guard
    pub stats: Stats,
}

// src/interp/walk.rs
pub struct Walk<'db> {
    pub frame: Frame<'db>,
    pub activations: IndexVec<ActivationId, Env<'db>>,
}

impl<'db> Walk<'db> {
    pub fn eval(&mut self, s: &mut Solver<'db>, expr: &Expr, env: &mut Env<'db>) -> Values<'db>
    // … and the same for every other method
}
```

**Why a parameter and not a field:** a walk could hold `&mut Solver`, and that
would compile for now. In step 05, a walk that waits on a recursive call has
to be **stored in the solver** until its cycle is solved. A struct can't be
stored inside the thing it holds a borrow of. So the plan's rule, from the
start: functions take `(s: &mut Solver, w: &mut Walk)`, here as `&mut self`
plus `s`.

This is a mechanical change: `self.sem` becomes `s.sem` everywhere, the
`Keys` struct from step 01 folds into `Solver`, and `path` moves there too.
Inline mode keeps a single `Walk` for the whole request, so closures can still
escape their frame there.

### Decision 3: a walk ends in its summary

```rust
// src/interp/summary.rs
pub struct FunctionResult<'db> {
    pub calls: Vec<CallEdge<'db>>,   // one per call site reached, in evaluation order
    pub returns: SVals<'db>,         // the union of every return
}

pub struct CallEdge<'db> {
    pub site: SiteId,
    pub targets: SmallVec<[Target<'db>; 2]>,   // one per project callee at this site
}

pub enum Target<'db> {
    Call(ContextId),                 // Construct and External arrive with classes and part 05
}

/// The call expression's range, relative to the start of the enclosing `def`.
pub struct SiteId { pub start: TextSize, pub end: TextSize }
```

The frame records a `CallEdge` at every call site that has a project callee.
`finish` turns the walk's returns into `SVals` and packs both.

**Why `calls` in the summary:** in step 03 a reused summary is not walked
again, so its edges must be stored. They are how the tree under a reused call
is drawn.

**Why `SiteId` is relative to the `def`:** an edit above the function doesn't
move it. Part 05 uses that for identities that last across edits. It is a
range, not a start offset, because `f()()` starts two calls at one offset.

### Decision 4: nothing walk-local can be written into a summary

```rust
pub enum SVal<'db> {
    Plain(PlainVal<'db>),
    // Closure: step 07. Ref, Obj, Bound: part 04
}

/// Values with no identity. No variant can hold an ActivationId, and later
/// no OriginId, ValueId or DeferId, so a leak does not compile.
pub enum PlainVal<'db> {
    Function(Definition<'db>),
    Module(Type<'db>),
    Ty(Type<'db>),
    Unknown,
}
```

**This is rule 2.** An `ActivationId` from `ident`'s walk is meaningless in
`main`'s walk. If a summary could hold one, `main` would read some unrelated
activation of its own, with no error. Making `SVal` a separate type, with no
variant that can carry a walk-local id, turns that bug into a compile error.

`AbstractValue::Closure` holds an `ActivationId`, so it has no `SVal` yet.
Until step 07, `finish` writes a returned closure as `Plain(Unknown)`, and
`closures.py` runs in inline mode only.

### Decision 5: the caller resolves the summary into its own values

```rust
fn resolve(&mut self, returns: &SVals<'db>) -> Values<'db> {
    returns.iter().map(|sv| match sv {
        SVal::Plain(PlainVal::Function(d)) => AbstractValue::Function(*d),
        SVal::Plain(PlainVal::Module(t)) => AbstractValue::Module(*t),
        SVal::Plain(PlainVal::Ty(t)) => AbstractValue::Ty(*t),
        SVal::Plain(PlainVal::Unknown) => AbstractValue::Unknown,
    }).collect()
}
```

For plain values this is a straight copy. It gets interesting in step 07 (a
closure needs a new activation in the caller) and in part 04 (an object must
come back as the caller's own object).

### Decision 6: for this step only, the tree still comes from each walk

There is no reuse yet: every call still walks its callee, from its key. So the
callee walk's frame still collects child nodes, exactly as in inline mode, and
the tree is built the same way. Step 03 adds reuse, and then a reused call has
no walk to take children from.

The recursion guard works as before, on `s.path`.

### Decision 7: the harness runs every fixture in both modes

`Mode` gets its second variant:

```rust
// src/interp/mod.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Follow every call into the callee's body (parts 01 and 03).
    Inline,
    /// One walk per call, started from its key. The caller reads the summary.
    Summary,
}
```

And the harness gets a second entry point:

```rust
// src/interp/fixture.rs
pub fn inline() -> Options  { Options { mode: Mode::Inline,  ..Options::default() } }
pub fn summary() -> Options { Options { mode: Mode::Summary, ..Options::default() } }

/// Inline mode only: for a fixture summary mode can't run yet.
pub fn check(fixture: &str) {
    check_with(fixture, &[inline()]);
}

/// Every mode. The same expected trees must come out of each.
pub fn check_both(fixture: &str) {
    check_with(fixture, &[inline(), summary()]);
}

/// One run per `Options`. Every failing block of every run goes in one report.
fn check_with(fixture: &str, runs: &[Options]) {
    let report: String = runs.iter().map(|opts| failures(fixture, opts)).collect();
    assert!(report.is_empty(), "\n{report}");
}
```

`failures(fixture, opts)` is step 01's `check` with two changes: it takes
`opts` instead of `Options::default()`, and it returns its report instead of
asserting on it. Each header also names the run, so you can tell which mode
failed (reference below).

**Why one expected tree for both modes:** inline mode is your oracle. It
already passes every part 01 fixture, so when only the summary run fails, the
bug is in this part's code, not in binding or returns. It is also how a
fixture stays fixed (README, "Fixtures only ever get stricter"): summary mode
joins a fixture by switching its test from `check` to `check_both`, and its
expected trees stay as they are.

**Why every run reports before the test fails:** "inline passes, summary
fails" and "both fail" point at different code. If the inline run panicked
first, you would never see the summary run's report.

**Why `inline()` and not `Options::default()`:** step 04.09 makes the default
`Mode::Summary`. `check` must stay inline after that. The `..Options::default()`
also keeps both helpers compiling when step 03 adds a field to `Options`.

### Yours

- `Mode::Summary`, and `call_project` beside `call_function`: key, a new
  `Walk` for the context, `materialize`, walk, `finish`, `resolve`.
- `interp/summary.rs`: the types above, `finish`, `resolve`.
- `Solver`, and the `s: &mut Solver` parameter everywhere.
- `call_tree` picks the mode: the entry's env comes from `env_from_bound` in
  inline mode, from `materialize` in summary mode.
- The harness: `inline()`, `summary()`, `check_with`, `failures`,
  `check_both`. Switch every earlier test to `check_both`, except
  `p01_11_closures`.

<details><summary>Reference: this step's code</summary>

```rust
// src/interp/call.rs: summary mode, no reuse yet
fn call_project(&mut self, s: &mut Solver<'db>, def: Definition<'db>, site: SiteId,
                bound: &Bound<'db>, targets: &mut SmallVec<[Target<'db>; 2]>) -> Values<'db> {
    let qname = s.sem.qname(def);
    let ctx = s.key(def, bound);
    targets.push(Target::Call(ctx));
    if s.path.contains(&qname) {
        let mut node = Node::call(qname, self.site_range(site));
        node.marks.push(Mark::Recursive);
        self.frame.children.push(node);
        s.stats.nodes += 1;
        return smallvec![AbstractValue::Unknown];
    }

    let parsed = s.sem.parsed(def);
    let Some(node) = s.sem.function_node(def, &parsed) else { return smallvec![AbstractValue::Unknown] };

    s.path.push(qname.clone());
    s.stats.walks += 1;
    let mut callee = Walk::new(s, def);                         // its own frame, its own activations
    let env = callee.materialize(s, ctx, node);                 // rule 1
    let end = callee.walk_stmts(s, &node.body, env);
    let (result, children) = callee.finish(s, end, node);       // rule 2
    s.path.pop();

    self.frame.children.push(Node { qname, site: Some(self.site_range(site)), marks: vec![], children });
    s.stats.nodes += 1;
    self.resolve(&result.returns)
}
```

```rust
// src/interp/mod.rs: the entry, in both modes
pub fn call_tree(db: &ProjectDatabase, file: File, entry: &str, opts: &Options) -> Option<(Node, Stats)> {
    let sem = Sem::new(db);
    let def = sem.entry(file, entry)?;
    let parsed = sem.parsed(def);
    let node = sem.function_node(def, &parsed)?;
    let mut s = Solver::new(sem, opts.clone());
    let qname = s.sem.qname(def);
    s.path.push(qname.clone());
    s.stats.walks += 1;                                          // the entry

    // Called with no arguments (step 01.11), and keyed like any call.
    let mut w = Walk::new(&mut s, def);
    let bound = w.bind_parameters(&mut s, def, node, &EvaluatedArgs::default(), None);
    let ctx = s.key(def, &bound);
    let env = match opts.mode {
        Mode::Inline => {
            let mut env = w.env_from_bound(node, &bound);
            env.scope = Some(s.sem.body_scope(def, node));
            env
        }
        Mode::Summary => w.materialize(&mut s, ctx, node),       // rule 1 holds for the entry too
    };
    w.walk_stmts(&mut s, &node.body, env);

    s.stats.contexts = s.contexts.len() as u32;
    let mut root = Node::root(qname);
    root.children = w.frame.children;
    Some((root, s.stats))
}
```

```rust
// src/interp/fixture.rs: step 01's `check`, as one run that reports
fn failures(fixture: &str, opts: &Options) -> String {
    let path = std::fs::canonicalize(format!("experience/12-call-tree/{fixture}")).unwrap();
    let root = path.parent().unwrap();
    let db = open_project(SystemPath::from_std_path(root).unwrap()).unwrap();
    let (file, _) = open(&db, SystemPath::from_std_path(&path).unwrap()).unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let module = path.file_stem().unwrap().to_str().unwrap();

    let blocks = expectations(&source);
    assert!(!blocks.is_empty(), "{fixture}: no `# expect:` block");
    let mut report = String::new();
    for e in &blocks {
        let got = call_tree(&db, file, &e.entry, opts)
            .map(|(t, _)| to_text(&t, module))
            .unwrap_or_else(|| format!("(no function `{}`)\n", e.entry));
        println!("--- {fixture}: {} ({:?})\n{got}", e.entry, opts.mode);   // --show-output
        if got != e.tree {
            report += &format!("--- {fixture}: {} {opts:?}\nexpected:\n{}got:\n{}\n", e.entry, e.tree, got);
        }
    }
    report
}
```

`stats_for` doesn't change: it already takes `&Options`, so
`stats_for(fixture, "main", &summary())` gives summary mode's stats.

```rust
// src/interp/summary.rs
impl<'db> Walk<'db> {
    pub fn finish(mut self, s: &mut Solver<'db>, end: Option<Env<'db>>, node: &StmtFunctionDef)
        -> (FunctionResult<'db>, Vec<Node>)
    {
        if end.is_some() {
            let none = self.none_value(s);
            self.frame.returns.extend(none);         // fell off the end
        }
        let mut returns = if contains_yield(&node.body) {
            smallvec![AbstractValue::Unknown]
        } else {
            std::mem::take(&mut self.frame.returns)
        };
        dedup(&mut returns);
        let returns = returns.into_iter().map(to_sval).collect();
        (FunctionResult { calls: self.frame.calls, returns }, self.frame.children)
    }
}

fn to_sval(v: AbstractValue<'_>) -> SVal<'_> {
    SVal::Plain(match v {
        AbstractValue::Function(d) => PlainVal::Function(d),
        AbstractValue::Module(t) => PlainVal::Module(t),
        AbstractValue::Ty(t) => PlainVal::Ty(t),
        AbstractValue::Unknown => PlainVal::Unknown,
        AbstractValue::Closure { .. } => PlainVal::Unknown,   // step 07
    })
}
```

`eval_call` collects one `targets` list per call site, calls `call_project`
(summary mode) or `call_function` (inline mode) for each project callee, and
pushes a `CallEdge` when `targets` is not empty. The empty-argument rule of
step 01.10 stays in `eval_call`: the call is made, its value is dropped.

`site_range(site)` adds the function's start back to get the file range for
the node. `SiteId::new(call.range(), def_start)` does the reverse.

</details>

---

## Run

```rust
#[test]
fn p02_02_summaries() {
    check_both("02-function-summaries/python/summaries.py");
}
```

And every earlier test, now through `check_both`, except `p01_11_closures`,
which stays on `check` until step 07. The `stats_for` assertions in `p01_13`
and `p02_01` stay as they are: `Options::default()` is still inline mode.

```bash
cargo test interp::fixture -- --show-output
```

They all pass in summary mode with the same trees: nothing about the tree
changed, only where the callee's starting env and the call's value come from.
If one fails, the header says which run, for example
`--- 01-functions/python/returns.py: main Options { mode: Summary }`.

---

## Where you are

```
Solver { sem, opts, fps, contexts, path, stats }           one per request
Walk   { frame, activations }                              one per context (summary) or per request (inline)
FunctionResult { calls: [CallEdge { site, targets }], returns: [SVal] }
SVal = Plain(PlainVal)
Mode = Inline | Summary          check_both runs every fixture in both
```

Summary mode walks exactly as many bodies as inline mode. Every walk now
produces something that *could* be reused. Step 03 reuses it.

---

## Exercise

1. Change `materialize` to read the caller's `Bound` instead of the key. Which
   test fails? (Trick question: explain why none does, yet.)
2. Why does `finish` deduplicate the returns before converting them, and why
   does that matter more in step 05 than now?
3. `ident(pick(0))` evaluates `pick(0)` as an argument. Which walk's frame
   records the `CallEdge` for `pick`, and which records the one for `ident`?

<details><summary>Answers</summary>

1. None. For function values the fingerprint loses nothing the walk reads:
   functions keep their identity, and literals lose a value the walker never
   looks at. The bug is real, it just can't produce a different tree until a
   key loses something the walk does read. Part 04's objects do: attributes
   deeper than `fp_depth`.
2. Equal values written twice would make two summaries that mean the same
   thing compare unequal. In step 05 the fixpoint stops when a round leaves
   every summary unchanged, and it compares them with `==`.
3. `main`'s frame records both. `pick(0)` is evaluated in `main`'s walk, as an
   argument, before `ident` is called. `ident`'s walk only sees the value.

</details>

---

→ Next: [`03-the-solver-and-the-graph.md`](03-the-solver-and-the-graph.md)
