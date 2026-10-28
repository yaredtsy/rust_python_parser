# 03.01 — Constructing

**You build:** classes as values, and construction. Calling a class makes a
new object, walks `__init__` with `self` bound to it, and gives the object as
the call's value. The tree shows a node for the class, with `__init__`'s calls
under it.
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
def make(): return Pipeline()

def main():
    Plain()               # Plain
    Pipeline()            # Pipeline > setup
    Wrapper(Pipeline())   # Pipeline > setup, then Wrapper > log
    Child()               # Child > log   (Base.__init__)
    make()                # make > Pipeline > setup
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
```

## Predict

1. `Child()` has no `__init__` of its own. Whose body is walked, and what is
   the node called?
2. `Wrapper(Pipeline())`: in which order do the two nodes appear, and why?
3. `Pipeline()` appears three times in this fixture. Are they three objects or
   one? What would tell them apart?

---

## Build

### Decision 1: a class is a value, and calling it constructs

```rust
pub enum AbstractValue<'db> {
    Function(Definition<'db>),
    Closure { func: Definition<'db>, activation: ActivationId },
    Class(Definition<'db>),                                  // ← new: the class object
    Instance { class: Definition<'db>, origin: OriginId },   // ← new: one object
    Module(Type<'db>),
    Ty(Type<'db>),
    Pending(DeferId),
    Unknown,
}
```

`sem::shape` gains `Shape::Class(def)` for a class literal or a generic alias
(`Box[int]` is still `Box`), and `eval_call` gains an arm: a project `Class`
callee goes to `construct`.

### Decision 2: the node is the class, its children are `__init__`'s calls

```rust
fn construct(&mut self, s: &mut Solver<'db>, class: Definition<'db>, site: TextRange,
             args: &EvaluatedArgs<'db>, env: &mut Env<'db>) -> Values<'db> {
    let origin = self.origins.intern(OriginKey::New { site, act: self.frame.act });
    let obj = AbstractValue::Instance { class, origin };
    let mut node = Node::call(s.sem.qname(class), site);
    if let Some(init) = s.sem.init_of(class).filter(|&i| s.sem.is_project(i)) {
        let run = self.walk_inline(s, init, None, Some(obj), args, env);   // self ← obj
        node.children = run.children;
    }
    self.frame.children.push(node);
    smallvec![obj]
}
```

**Why the class, not `__init__`:** that is what the Jedi driver produces, and
what v-noc shows: a `ClassSchema/<id>` node for the class, with `__init__`'s
calls under it (`plan/03-call-tree/14-output.md` § "The tree is a view",
"Classes").

**Why `__init__` comes from ty's lookup on the class:** `Child()` runs
`Base.__init__`. Finding it means walking the MRO, which ty already does:
`sem.init_of(class)` is `vnoc::member` for `__init__` on the class. When
nothing in the MRO is project code with a body (no `__init__` anywhere, a
dataclass, a `NamedTuple`), the node still appears, with no children.

`walk_inline` is step 01.05's `call_function`, split so the node can be built
by the caller: it takes the function, the closure activation if any, the
receiver if any, the arguments and the caller's env, and returns the callee's
children and returns. `call_function` becomes "`walk_inline`, then push a node
for the function".

### Decision 3: `self` is the first parameter, whatever it is called

`bind_parameters` gains a `receiver: Option<AbstractValue>`. When it is
`Some`, it goes to the **first** parameter, and the positionals are matched
against the rest. Step 02 uses the same rule for every method call.

### Decision 4: an object is named by where it was built

```rust
pub enum OriginKey {
    /// An object built in this walk, at this site, in this run of its body.
    New { site: TextRange, act: ActivationId },
}
// Walk gains: origins: Interner<OriginKey, OriginId>
```

`Instance { class, origin }` is one object. The three `Pipeline()`s in the
fixture are three objects: three sites, or the same site in two runs of
`make`.

**Why not just the class:** step 03 gives objects state. Two `Pipeline`s with
different handlers must not share one `handler` attribute. The class can't
tell them apart; where they were built can.

**Why the site *and* the run:** in inline mode, `make`'s body runs once per
call to `make`. Its `Pipeline()` site evaluates once per run, and each run
makes a new object. The run's activation tells them apart. In summary mode
(part 04) a site is evaluated once per walk, so the walk itself plays that
role.

### Decision 5: constructing guards recursion like calling

`class Node: def __init__(self): self.child = Node()` constructs forever. The
class's qualified name goes on the path while its `__init__` is walked, and a
construction of a class already on the path is a `[recursive]` leaf, as in
step 01.06. The renderer in part 02 uses the same rule, since a `Construct`
node's name is the class.

### Yours

- `Class`, `Instance`, `Shape::Class`, `OriginKey`, `Walk.origins`.
- `sem.init_of(class)`, and `sem.class_type(def)`: the class's own `Type`,
  for `vnoc::member`. It needs one more `vnoc` wrapper, `type_of_definition`
  over ty's `binding_type` (`pub(crate)`, `types.rs:235`), per the wrapper
  rules in `types/vnoc.rs`.
- `walk_inline`, `construct`, and the receiver in `bind_parameters`.
- A non-project class (`dict()`) gets no node; its value is ty's type of the
  call, lifted.

<details><summary>Reference: this step's code</summary>

```rust
// vendor/ruff/crates/ty_python_semantic/src/types/vnoc.rs (one more wrapper)
/// The type a definition binds: a class's own type for a `class` statement.
/// `binding_type` (`types.rs:235`), pub(crate).   [check] the path
pub fn type_of_definition<'db>(db: &'db dyn Db, def: Definition<'db>) -> Type<'db> {
    binding_type(db, def)
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

// shape(): Type::ClassLiteral(_) | Type::GenericAlias(_) =>
//     match ty.definition(self.db, &env) { Some(TypeDefinition::StaticClass(d)) => Shape::Class(d), _ => Shape::Other }
```

```rust
// src/interp/call.rs
pub struct InlineRun<'db> {
    pub children: Vec<Node>,
    pub returns: Values<'db>,
}

fn walk_inline(&mut self, s: &mut Solver<'db>, def: Definition<'db>, enclosing: Option<ActivationId>,
               receiver: Option<AbstractValue<'db>>, args: &EvaluatedArgs<'db>, env: &mut Env<'db>)
    -> InlineRun<'db>
{
    let parsed = s.sem.parsed(def);
    let Some(node) = s.sem.function_node(def, &parsed) else {
        return InlineRun { children: vec![], returns: smallvec![AbstractValue::Unknown] };
    };
    let new_frame = self.new_frame(s, def);
    let caller = std::mem::replace(&mut self.frame, new_frame);
    let bound = self.bind_parameters(s, node, args, receiver, enclosing);
    let start = self.env_from_bound(s, def, node, &bound, enclosing);
    if self.walk_stmts(s, &node.body, start).is_some() {
        let none = self.none_value(s);
        self.frame.returns.extend(none);
    }
    let callee = std::mem::replace(&mut self.frame, caller);
    InlineRun { children: callee.children, returns: callee.returns }
}
```

`env` is passed through untouched for now. Step 03 uses it: that is where an
object's state flows into and back out of the call.

</details>

---

## Run

```rust
#[test]
fn p03_01_construct() {
    check("03-classes/python/construct.py");      // inline mode only, until part 04
}
```

Then every earlier test, in both modes. Summary mode doesn't construct yet: in
part 01 and 02 fixtures there are no classes, so nothing changes there.

---

## Where you are

```
AbstractValue += Class(def) | Instance { class, origin }
Walk          += origins: OriginKey::New { site, act } → OriginId
construct(class) → Node(class) with __init__'s children, value [Instance]
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
3. `A()()` (an object whose class defines `__call__`) or `make()()` start two
   calls at one offset. With the start alone, two different constructions could
   get the same origin and share their state.

</details>

---

→ Next: [`02-methods-and-self.md`](02-methods-and-self.md)
