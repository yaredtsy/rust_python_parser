# 03.01 — Constructing

**You build:** classes as values, and construction. Calling a class makes a
new object, walks `__init__` with `self` bound to it, and gives the object as
the call's value. The tree shows a node for the class, with `__init__`'s calls
under it. Inline mode only: summary mode keeps asking ty until part 04.
**Fixture:** `python/construct.py`
**Plan:** `plan/03-call-tree/10-one-call.md` § "Constructors", `04-values.md`
§ "`OriginId`: which object"

---

## The fixture

```python
class Plain: pass
class Pipeline:
    def __init__(self): setup()
class Wrapper:
    def __init__(self, inner): log()
class Base:
    def __init__(self): log()
class Child(Base): pass
class Tree:
    def __init__(self): Tree()
def make(): return Pipeline()

def main():
    Plain()               # Plain
    Pipeline()            # Pipeline > setup
    Wrapper(Pipeline())   # Pipeline > setup, then Wrapper > log
    Child()               # Child > log   (Base.__init__)
    make()                # make > Pipeline > setup
    Tree()                # Tree > Tree [recursive]
    dict()                # nothing

# expect:
# main
#   Plain
#   Pipeline
#     setup
#   Pipeline
#     setup
#   Wrapper
#     log
#   Child
#     log
#   make
#     Pipeline
#       setup
#   Tree
#     Tree [recursive]
```

## Predict

1. `Child()` has no `__init__` of its own. Whose body is walked, and what is
   the node called?
2. `Wrapper(Pipeline())`: in which order do the two nodes appear, and why?
3. `Pipeline()` appears three times in this fixture. Are they three objects or
   one? What would tell them apart?
4. `Tree.__init__` calls `Tree()`. Step 01.06's guard lives in
   `call_function`. Does a construction go through it?

---

## Build

Where this step's code goes, in `src/interp/call.rs`:

```
eval_call
  └─ call_values                 callee loop: Function, Closure, and now Class
       ├─ call_function(def)     ─┐
       └─ construct(class)  new  ─┤
                                  └─ inline_node(name, def, receiver)   new
                                       guard → path push → walk_inline → path pop → node
                                       └─ walk_inline(def, receiver)    new: the old middle
                                            frame, bind, key, walk,       of call_function
                                            returns
```

Every inline node is made by `inline_node`, so every one passes the recursion
guard. Step 02's method calls use it too.

### Decision 1: a class is a value, and calling it constructs

```rust
// src/interp/value.rs
pub enum AbstractValue<'db> {
    Function(Definition<'db>),
    Closure { func: Definition<'db>, activation: ActivationId },
    Class(Definition<'db>),                                  // ← new: the class object
    Instance { class: Definition<'db>, origin: OriginId },   // ← new: one object
    Ty(Type<'db>),
    Module(Type<'db>),
    Unknown,
    Pending(DeferId),
}
```

`sem::shape` gains `Shape::Class(def)` for a class literal or a generic alias
(`Box[int]` is still `Box`), and `lift` turns it into `Class(def)`. This
covers every class, including `dict`. Whether calling it walks anything is
decided at the call, by `is_project`, as for functions.

In `call_values`, a project `Class` callee is collected next to the
functions and constructed after them:

```rust
// call_values, the callee loop
AbstractValue::Class(class) if sol.opts.mode == Mode::Inline && sol.sem.is_project(class) => {
    classes.push(class)
}
_ => ask_ty = true,   // as before: `dict`, and every class in summary mode
```

### Decision 2: the node is the class, its children are `__init__`'s calls

```rust
fn construct(&mut self, sol: &mut Solver<'db>, class: Definition<'db>,
             site: TextRange, args: &EvaluatedArgs<'db>) -> Values<'db> {
    let origin = self.origins.intern(OriginKey::New { site, act: self.frame.act });
    let obj = AbstractValue::Instance { class, origin };
    let name = sol.sem.qname(class);
    match sol.sem.init_of(class).filter(|&init| sol.sem.is_project(init)) {
        // `self` ← obj. The guard is keyed on the class's name (Decision 3).
        Some(init) => { self.inline_node(sol, name, site, init, None, Some(obj), args); }
        // no body to walk, so nothing can recurse: a plain leaf
        None => self.push_leaf(sol, Node::call(name, site)),
    }
    if args.has_empty() { Values::new() } else { smallvec![obj] }
}
```

**Why the class, not `__init__`:** that is what the Jedi driver produces, and
what v-noc shows: a `ClassSchema/<id>` node for the class, with `__init__`'s
calls under it (`plan/03-call-tree/14-output.md` § "The tree is a view",
"Classes").

**Why `__init__` comes from ty's lookup on the class:** `Child()` runs
`Base.__init__`. Finding it means walking the MRO, which ty already does:
`sem.init_of(class)` is `vnoc::member` for `__init__` on the class. When
nothing in the MRO is project code (no `__init__` anywhere, so ty finds
`object.__init__`; a dataclass; a `NamedTuple`), the node still appears, with
no children.

**Why the value is the object, whatever `__init__` returns:** `__init__`
returns `None`. Calling the class returns the object. `run.returns` is
dropped.

**`has_empty`:** as in `call_function`. An argument with no value means Python
never makes the call. The node still appears, but the value is empty.

### Decision 3: one guard for every inline node, keyed on the node's name

Step 01.06 put the guard at the top of `call_function`. A construction doesn't
go through `call_function`, so without a change `Tree()` constructs forever.
So the guard moves to the one place that makes inline nodes:

```rust
/// Inline mode: a node named `name` at `site`, with the calls `def` makes under it.
/// `name` already on the path: a `[recursive]` leaf, nothing walked, `None`.
fn inline_node(&mut self, sol: &mut Solver<'db>, name: String, site: TextRange,
               def: Definition<'db>, enclosing: Option<ActivationId>,
               receiver: Option<AbstractValue<'db>>, args: &EvaluatedArgs<'db>)
    -> Option<Values<'db>>
{
    if sol.path.contains(&name) {
        let mut node = Node::call(name, site);
        node.marks.push(Mark::Recursive);
        self.push_leaf(sol, node);
        return None;
    }
    sol.path.push(name.clone());
    let run = self.walk_inline(sol, def, enclosing, receiver, args);
    sol.path.pop();
    let mut node = Node::call(name, site);
    node.children = run.children;
    self.push_leaf(sol, node);
    Some(run.returns)
}
```

`call_function` is now just the call to `inline_node` and the value rules:

```rust
fn call_function(&mut self, sol: &mut Solver<'db>, def: Definition<'db>,
                 enclosing: Option<ActivationId>, site: TextRange,
                 args: &EvaluatedArgs<'db>) -> Values<'db> {
    let name = sol.sem.qname(def);
    match self.inline_node(sol, name, site, def, enclosing, None, args) {
        None => smallvec![AbstractValue::Unknown], // recursive: not walked (step 01.10)
        Some(_) if args.has_empty() => Values::new(), // Python never makes this call
        Some(returns) => returns,
    }
}
```

**Why the class's name, not `__init__`'s:** the path holds the names of the
nodes being walked, and this node is named after the class. Part 04's
renderer stops on a repeated node name, so inline mode has to use the same
rule to produce the same tree. `Tree > Tree [recursive]` reads as the
recursion it is. Keyed on `Tree.__init__`, the leaf would be named after a
node that isn't on the path.

**Why a recursive construction still gives the object:** a function's value
comes from its body, so a call that isn't walked is `Unknown`. A
construction's value doesn't depend on `__init__`'s body: it is a new object
of that class, which is known without walking.

**Rejected: the guard in each caller.** Step 01.06's guard sat in
`call_function`. With constructors and (step 02) methods, there are three
places that walk a body. A guard copied into each one fails the usual way:
the next caller added forgets its copy, and that kind of call loops forever.

### Decision 4: `self` is the first parameter, whatever it is called

`bind_parameters` gains a `receiver: Option<AbstractValue>`. When it is
`Some`, it goes to the **first** parameter, and the positionals are matched
against the rest. Step 02 uses the same rule for every method call.

```rust
// bind_parameters, before the splat / decorator early return
let has_first = params.posonlyargs.first().or(params.args.first()).is_some();
if let (Some(recv), true) = (receiver, has_first) {
    slots.push(smallvec![recv]);    // `self`, even when the rest can't be matched
}
let skip = slots.len();             // 0 or 1: parameters already bound
// … the early return fills `param_names(def).skip(skip)` with Unknown,
// and the positional loop runs over `posonlyargs.chain(args).skip(skip)`.
```

`bind_for_callee` (summary mode) passes `None`.

### Decision 5: an object is named by where it was built

```rust
// src/interp/value.rs
#[newtype_index] pub struct OriginId;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum OriginKey {
    /// An object built in this walk, at this site, in this run of its body.
    New { site: TextRange, act: ActivationId },
}
// Walk gains: origins: Interner<OriginKey, OriginId>   (the Interner from key.rs)
```

`Instance { class, origin }` is one object. The three `Pipeline()`s in the
fixture are three objects, because they are three sites. Exercise 1 is the
other case: one site, two runs of `make`.

**Why not just the class:** step 03 gives objects state. Two `Pipeline`s with
different handlers must not share one `handler` attribute. The class can't
tell them apart; where they were built can.

**Why the site *and* the run:** in inline mode, `make`'s body runs once per
call to `make`. Its `Pipeline()` site evaluates once per run, and each run
makes a new object. The run's activation tells them apart. In summary mode
(part 04) a site is evaluated once per walk, so the walk itself plays that
role.

### Decision 6: summary mode asks ty, for now

The `Class` arm only fires in inline mode (Decision 1). In summary mode a
project class call goes to `ask_ty`, like `dict()`: no node, and the value is
ty's type of the call (`Pipeline`, lifted to `Ty`).

**Why not construct in summary mode too:** the summary walker never keeps
`frame.children`; the tree is rendered from the graph's edges. A node pushed
there would be dropped, and `__init__`'s calls would be walked into a frame
nobody reads. Constructors in summary mode need a key with an object in it,
which is part 04 (04.01 Decision 5).

**Why `needs_defer` doesn't change:** it defers a project callee given a
pending argument, because the callee's key would read the pending value. A
class in summary mode isn't keyed, it's ty's answer, so nothing waits.

### Decision 7: keys get a stand-in for classes and objects

Inline mode still computes a key at every walk (step 02.01, to count
contexts), and `__init__`'s first slot is now an `Instance`. `fp_one` has to
say something for it:

```rust
// src/interp/key.rs, fp_one
AbstractValue::Class(d) => Fp::Class(d),              // a class is a constant, like a function
AbstractValue::Instance { .. } => Fp::Unknown,        // part 04: Fp::Instance { class, attrs }
```

`Fp::Class(d)` is new, with its arm in `from_fp`. `PlainVal::Class(d)` gets
the same treatment in `summary.rs`, so a function that returns a class keeps
it through a summary instead of turning it into `Unknown` (`plain()` has a
wildcard, so the compiler won't point this one out).

### What this step leaves out

`__new__` and a metaclass's `__call__` are ignored. A class that overrides
`__new__` still shows `__init__`'s calls, and a metaclass that changes what
calling the class does isn't followed. This is a choice, not an oversight:
the tree shows the `__init__` that ty finds, and nothing else.

### Yours

- `Class`, `Instance`, `OriginId`, `OriginKey`, `Walk.origins`.
- `Shape::Class` in `sem::shape`, and its arm in `lift`.
- `sem.init_of(class)`, and `sem.class_type(def)`: the class's own `Type`,
  for `vnoc::member`. It needs one more `vnoc` wrapper, `type_of_definition`
  over ty's `binding_type` (`pub(crate)`, `types.rs:235`), per the wrapper
  rules in `types/vnoc.rs`.
- `walk_inline`, `inline_node`, `push_leaf`, `construct`; `call_function`
  reduced to `inline_node` plus its value rules.
- The `Class` arm and the `classes` list in `call_values`.
- The receiver in `bind_parameters`.
- `Fp::Class`, `PlainVal::Class`, and the stand-in for `Instance` in `fp_one`.
- A non-project class (`dict()`) gets no node; its value is ty's type of the
  call, lifted.

<details><summary>Reference: this step's code</summary>

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs (one more wrapper)
/// The type a definition binds: a class's own type for a `class` statement.
pub fn type_of_definition<'db>(db: &'db dyn Db, def: Definition<'db>) -> Type<'db> {
    crate::types::binding_type(db, def)
}
```

```rust
// src/sem.rs
pub fn class_type(&self, class: Definition<'db>) -> Type<'db> {
    vnoc::type_of_definition(self.db, class)
}

/// `__init__` as found through the MRO, if it is a `def` somewhere.
pub fn init_of(&self, class: Definition<'db>) -> Option<Definition<'db>> {
    let file = class.program_file(self.db);
    let t = self.member(self.class_type(class), "__init__", file)?;
    match self.shape(t, file) {
        Shape::Function(d) => Some(d),
        _ => None,
    }
}

// shape(), next to FunctionLiteral:
Type::ClassLiteral(_) | Type::GenericAlias(_) => {
    let env = ProgramEnvironment::from_file(file);
    match ty.definition(self.db, &env) {
        Some(TypeDefinition::StaticClass(d)) => Shape::Class(d),
        _ => Shape::Other,
    }
}
```

```rust
// src/interp/value.rs, lift
Shape::Class(d) => smallvec![AbstractValue::Class(d)],
```

```rust
// src/interp/call.rs, call_values: the new lines, marked
let mut targets: Vec<(Definition<'db>, Option<ActivationId>)> = Vec::new();
let mut classes: Vec<Definition<'db>> = Vec::new();                       // ← new
let mut ask_ty = false;
for c in callees {
    match *c {
        AbstractValue::Function(def) if sol.sem.is_project(def) => targets.push((def, None)),
        AbstractValue::Closure { func, activation } if sol.sem.is_project(func) => {
            targets.push((func, Some(activation)))
        }
        AbstractValue::Class(class)                                         // ← new
            if sol.opts.mode == Mode::Inline && sol.sem.is_project(class) => classes.push(class),
        _ => ask_ty = true,
    }
}
targets.sort_by_key(|(d, _)| sol.sem.def_position(*d));
classes.sort_by_key(|&c| sol.sem.def_position(c));                         // ← new
let mut edge_targets: SmallVec<[Target; 2]> = SmallVec::new();
let mut out = Values::new();
for (def, enclosing) in targets {
    // … unchanged …
}
for class in classes {                                                      // ← new
    let range = self.site_range(site);
    out.extend(self.construct(sol, class, range, args));
}
match edge {
    // … unchanged …
}
```

```rust
// src/interp/call.rs: walk_inline is the old middle of call_function
pub struct InlineRun<'db> {
    pub children: Vec<Node>,
    pub returns: Values<'db>,
}

/// Inline mode: walk `def`'s body now. No node and no guard: that is `inline_node`'s job.
fn walk_inline(&mut self, sol: &mut Solver<'db>, def: Definition<'db>,
               enclosing: Option<ActivationId>, receiver: Option<AbstractValue<'db>>,
               args: &EvaluatedArgs<'db>) -> InlineRun<'db> {
    let parsed = sol.sem.parsed(def);
    let Some(node) = sol.sem.function_node(def, &parsed) else {
        return InlineRun { children: Vec::new(), returns: smallvec![AbstractValue::Unknown] };
    };
    sol.stats.walks += 1;
    let new = self.new_frame(sol, def);
    let caller = std::mem::replace(&mut self.frame, new);
    let bound = self.bind_parameters(sol, def, node, receiver, args, enclosing);
    sol.key(self, def, &bound, enclosing);
    let mut env = self.env_from_bound(node, &bound);
    env.scope = Some(sol.sem.body_scope(def, node));
    env.enclosing = enclosing;
    if self.walk_stmts(sol, &node.body, env).is_some() {
        let none = self.none_value(sol);
        self.frame.returns.extend(none); // fell off the end
    }
    let callee = std::mem::replace(&mut self.frame, caller);
    let returns = if contains_yield(&node.body) {
        smallvec![AbstractValue::Unknown] // a generator object
    } else {
        callee.returns
    };
    InlineRun { children: callee.children, returns }
}

fn push_leaf(&mut self, sol: &mut Solver<'db>, node: Node) {
    self.frame.children.push(node);
    sol.stats.nodes += 1;
}

// inline_node, call_function, construct: as in Decisions 2 and 3.
```

```rust
// src/interp/bind.rs, bind_parameters: the new lines, marked
pub fn bind_parameters(&mut self, sol: &mut Solver<'db>, func: Definition<'db>,
                       def: &StmtFunctionDef,
                       receiver: Option<AbstractValue<'db>>,                 // ← new
                       args: &EvaluatedArgs<'db>,
                       enclosing: Option<ActivationId>) -> Bound<'db> {
    let args = &args.with_unknown_for_empty();
    let mut positional = args.positional.iter();
    let params = &def.parameters;
    let unknown = || smallvec![AbstractValue::Unknown];
    let mut slots: SmallVec<[Values<'db>; 4]> = SmallVec::new();

    let has_first = params.posonlyargs.first().or(params.args.first()).is_some(); // ← new
    if let (Some(recv), true) = (receiver, has_first) {                            // ← new
        slots.push(smallvec![recv]);
    }
    let skip = slots.len();                                                         // ← new

    if args.has_splat || self.decorator_changes_signature(sol, def) {
        for _ in param_names(def).skip(skip) {                                      // ← skip
            slots.push(unknown());
        }
        return Bound { slots };
    }
    for p in params.posonlyargs.iter().chain(&params.args).skip(skip) {            // ← skip
        // … unchanged …
    }
    // … kwonly, *args, **kwargs unchanged …
}
```

```rust
// src/interp/key.rs
pub enum Fp<'db> { /* … */ Class(Definition<'db>) }
// fp_one:
AbstractValue::Class(d) => Fp::Class(d),
AbstractValue::Instance { .. } => Fp::Unknown, // part 04
// from_fp:
Fp::Class(d) => smallvec![AbstractValue::Class(*d)],

// src/interp/summary.rs
pub enum PlainVal<'db> { /* … */ Class(Definition<'db>) }
// plain():   AbstractValue::Class(d) => PlainVal::Class(d),
// to values: SVal::Plain(PlainVal::Class(d)) => AbstractValue::Class(*d),
```

</details>

---

## Run

```rust
#[test]
fn p03_01_construct() {
    check("03-classes/python/construct.py");      // inline mode only, until part 04
}
```

Then every earlier test, in both modes. Nothing changes there: in summary
mode a class call is still ty's answer (Decision 6), and no part 01 or 02
fixture calls a project class.

---

## Where you are

```
AbstractValue += Class(def) | Instance { class, origin }
Walk          += origins: OriginKey::New { site, act } → OriginId
inline_node(name)  → the guard, then walk_inline, then the node
construct(class)   → inline_node(class name, __init__, self = obj), value [obj]
summary mode       → a class call is ty's answer, until part 04
```

Objects exist and have identity. They don't hold anything yet, and calling a
method on one is the next step.

---

## Exercise

1. `def make(): return Pipeline()` is called twice from `main`. How many
   origins exist at the end, and what are their keys?
2. A dataclass `@dataclass class Job: h: Handler`. What does `Job(x)` produce
   in the tree? Where would `job.h` come from, later?
3. Why is `site` a full range here and not just the call's start? (Step 01.01
   asked the same about nodes.)

<details><summary>Answers</summary>

1. Two: `New { site: <Pipeline() in make>, act: <make's first run> }` and the
   same site with `make`'s second run. One site, two runs, two objects.
2. A `Job` node with no children: its `__init__` is synthesised by the
   dataclass machinery, so there is no body to walk. `job.h` has no state in
   the env (nothing wrote it), so step 02's lookup falls to the class side,
   where ty knows the field's declared type, `Handler`.
3. Two constructions can start at the same offset. `A().B()`, where `B` is a
   class defined inside `A`, constructs an `A` and then a `B`, and both calls
   start at the `A`. They run in the same activation, so with the start alone
   they would get one origin, and the two objects would share their state.
   The full range differs (`A()` ends before `.B()`). `A()()` and `make()()`
   also start two calls at one offset, but only one of each pair constructs.
   Calling an `Instance` goes to `__call__`, and `make()` is a function call.

</details>

---

→ Next: [`02-methods-and-self.md`](02-methods-and-self.md)
