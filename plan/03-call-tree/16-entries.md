# 03.16 — Entries: starting from a function, a method or a closure

Every function the analyser is asked about is **its own system, started like
`main`**. It is analysed when it is opened and again when it changes. Calls from
other functions never matter: no caller's data reaches it, and an unknown
parameter stays unknown.

A function can't always start on its own, though. A method needs an object, and
a closure needs the run of its parent. So an entry starts with a **prologue**:
walk down from the module to the entry, through every enclosing function and
class, running exactly what Python would need to reach it, with unknown
arguments. Then the entry itself is walked on what the prologue produced.

This replaces the "entry contexts" rule of [`08`](08-keys.md#entry-contexts),
and the unannotated-`self` part of the annotation fallback (see
[What it replaces](#what-it-replaces)).

---

## The rules

| entry | prologue | the entry is walked with |
|---|---|---|
| a function `f` | nothing | parameters unknown, or from their annotation |
| a method `C.m` | construct `C` with no arguments: walks `__init__` | `self` = that object, with `__init__`'s state |
| a property `C.p` | construct `C` | the getter, `self` = that object |
| a classmethod `C.m` | nothing | `cls` = `Class(C)` |
| a staticmethod `C.m` | nothing | no receiver |
| a closure `outer.inner` | run `outer`, keep its run | `inner` as `Closure { inner, outer's run }` |
| anything nested deeper | [the walk down](#the-walk-down), outermost first | |

Three rules hold for every prologue:

1. **Its nodes are hidden, its effects are kept.** `__init__` and `outer` are
   systems of their own, with their own trees. The entry's tree is only the
   entry's walk. But the object, its state, the free values and the defaults
   the prologue computed are what the entry starts from.
2. **Its arguments are unknown.** The prologue is called with nothing, like any
   entry: parameters take their defaults, or their annotation, or stay
   `Unknown`.
3. **It starts a clean path.** The recursion path holds only the entry when its
   walk begins, so nothing the prologue walked marks the entry `[recursive]`.

---

## The walk down

The prologue follows the **scope chain**: the classes and functions around the
entry, outermost first, from ty's semantic index (`ancestor_scopes`, reversed).
The module and type-parameter scopes (`def f[T]`) are skipped; a lambda or
comprehension can't hold a `def`, so it never appears.

```
enclosing = none        the run of the nearest enclosing FUNCTION
class     = none        the class just stepped into
for each scope in the chain:
    a class    → class = it                          nothing runs: a class body is not a run
    a function → receiver = class.take() → method: build its object (walk __init__)
                                         → classmethod: Class(class), staticmethod: none
                                         no class → none (a plain function or a closure)
                 the entry?  → start from (receiver, enclosing)
                 otherwise   → run it (hidden) with (receiver, enclosing); enclosing = its run
```

| chain | what runs | the entry starts with |
|---|---|---|
| `Svc.run` = `[Svc, run]` | build a `Svc` | `self` = it |
| `Svc.build.step` = `[Svc, build, step]` | build a `Svc`, run `build` on it | `enclosing` = `build`'s run (`self` is a free variable there) |
| `a.b.c` = `[a, b, c]` | run `a`, run `b` in `a`'s run | `enclosing` = `b`'s run, whose `enclosing` is `a`'s |
| `make.Local.run` = `[make, Local, run]` | run `make`; build a `Local`, its `__init__` in `make`'s run | `self` = it, `enclosing` = `make`'s run |
| `Outer.Inner.m` = `[Outer, Inner, m]` | build an `Inner` (never an `Outer`) | `self` = it |

Three rules make it right at any depth:

- **`enclosing` skips classes.** Python looks up a free variable in the
  enclosing *functions*, never in a class body. A method of a class defined
  inside a function reads the function's run, and so does its `__init__`.
- **An object is built only for a method, from the class right above it.** A
  class inside a class replaces the remembered one: `Inner`'s methods can't see
  an `Outer` instance, so no `Outer` is built.
- **An inherited `__init__` gets no run of ours.** Only an `__init__` defined in
  the class itself is walked with the class's `enclosing`. One inherited from a
  class elsewhere has its own chain.

The module itself is not run: module globals still come from ty (`OPAQUE`,
step 03.04). Running the module body would be one more level of the same walk,
and a separate decision.

---

## Functions

```python
def process(h: Handler, retries=3):
    h.handle()                     # Handler.handle: h from its annotation
```

No prologue. `bind_parameters` with no arguments: `retries` takes its default,
`h` takes its annotation through the fallback, with an origin for this run
(`Entry { act, param }`, step 03.07). This is what entries do today.

---

## Classes: methods, properties, class and static methods

### Finding the entry

`sem.entry(file, "C.m")` already descends into classes. What the entry *is*
comes from ty, in the same order `member` uses (step 03.06), on the class `C`
that encloses `m`:

1. `sem.property_parts(C, "m")` has a project getter: a **property**. The
   getter is walked like a method.
2. Otherwise, `vnoc::member` on `instance_type(C)`:
   - a `BoundMethod` that `bound_to` reads as `Object`: a **method**
   - a `BoundMethod` bound to the class: a **classmethod**
   - a plain function: a **staticmethod**

Asking ty instead of reading decorators means `@classmethod`, `classmethod(f)`
and a decorator ty sees through all give the same answer, as they do for calls.

### The prologue: construct the object

```python
class Pipeline:
    def __init__(self, handler: Handler):
        self.handler = handler
        self.log = Logger()
    def run(self):
        self.handler.process()     # entry Pipeline.run: Handler.process
        self.log.write()           # Logger.write
```

Entry `Pipeline.run`:

1. `construct(Pipeline)` with no arguments. It walks `Pipeline.__init__`
   (found through the MRO, so an inherited `__init__` and `super().__init__`
   chains run too). `handler` has no argument, so it starts from its
   annotation: a `Handler` object. `self.handler` holds it, and `self.log`
   holds the `Logger` built there.
2. `run` is walked with `self` = that object. `self.handler` and `self.log` hit
   the object's state.

The object's origin is `New { site: <C's definition range>, act: <the entry's
root run> }`. It is one object built once, like any `New`.

| `C` has… | the object |
|---|---|
| a project `__init__` (own or inherited) | its writes, as above |
| no project `__init__` (`object.__init__`, a library base) | no state: every read goes to the class side |
| a synthesized `__init__` (dataclass, `NamedTuple`) | no state: reads go to the class side, where ty knows the field types |

**What this deliberately does not include.** Only `__init__` runs. A method that
sets state later is not part of the start:

```python
class Job:
    def __init__(self): self.h = A()
    def configure(self): self.h = B()
    def run(self): self.h.go()     # entry Job.run: A.go only
```

That is the "started like `main`" contract: a fresh object, then the method. An
attribute `__init__` never set still falls to the class side, where ty's type is
the union of every assignment in the class.

### Classmethods and staticmethods

No object. A classmethod's `cls` is `Class(C)`, so `cls(...)` inside it is a
construction. A staticmethod has no receiver. Every other parameter follows the
function rules.

### Nested classes

Covered by [the walk down](#the-walk-down): a class in a class builds only the
inner object, and a class in a function builds its object inside the
function's run, so its methods and `__init__` read the function's locals.

One gap remains, outside the prologue: a `Class` value carries no run. When the
entry's walk calls *another* method of a local class (`self.other()` inside
`make.Local.run`), `call_function` walks it with no enclosing run, and its free
variables fall to ty. The fix is a `Class` value that carries the run its class
statement ran in, as `Closure` does. **[later]** until a fixture shows the loss.

---

## Closures

```python
def outer(cfg):
    h = Handler()
    def inner():
        h.go()                     # entry outer.inner: Handler.go
    h = LoudHandler()
    return inner
```

Entry `outer.inner`:

1. Run `outer`, as [the walk down](#the-walk-down) does for every function
   above the entry (so if `outer` is a method, its object is built first). Its
   nodes are hidden.
2. Keep `outer`'s run (its activation). The `def inner` statement has bound the
   closure there, with any defaults it computed.
3. Walk `inner` as `Closure { inner, activation }`, with no arguments. Its free
   variables are read from that run.

**Free values are taken at the end of `outer`'s run.** Python reads a free
variable when the closure runs, and a closure that is an entry is one that was
called after `outer` set it up: the end of the run is that moment. In the
example, `h` is `LoudHandler`, not `Handler`. With several exits (`return` in
branches), it is the join of the exit envs.

That needs one change: `walk_stmts` stores the env before each statement
(`activations[act] = env`), never after the last one. The prologue stores the
joined exit env into the run when `outer`'s walk ends.

If `outer`'s walk never reaches `def inner` (it is after an unconditional
`return`), there is no run to read: `inner` is walked with no enclosing run, and
its free variables come from ty, as today.

**[change]** `sem.entry` only descends into classes (`sem.rs`, the
`Stmt::ClassDef` arm). It must descend into functions too, so `outer.inner` and
`outer.Local.m` can be named.

---

## Both modes

The prologue's body is walked **directly in the entry's root walk**, in both
modes: `__init__` for a method, `outer` for a closure. Its frame is thrown away
(hidden nodes), and its env, runs and origins stay in the walk. Only the calls
*inside* the prologue go through the mode's usual call path.

- **Inline mode:** everything inside is walked inline, as now.
- **Summary mode:** calls inside the prologue are summary calls. A write the
  prologue's own body makes lands in `env.attrs` directly. A write made by a
  callee (`__init__` calling `self.setup()`) comes back as an effect, which
  needs part 04 (`04-class-summaries/03-effects`). Until then, summary-mode
  method entries see only the writes `__init__` makes itself.

The entry context is then keyed with the prologue's values: `self` as an
`Fp::Instance { class, attrs }` with `__init__`'s state, a closure's `free` from
`outer`'s run. It is no longer "the same context as a call that passes nothing";
it is whatever those values fingerprint to. If a real call fingerprints the same
way, the two share one summary (rule 1).

---

## Costs and limits

- One prologue walk per entry: `__init__` and what it calls, or `outer` and what
  it calls. Its callees are reused like any other calls.
- A cut inside the prologue (stack, context count, deadline) marks the entry's
  root `Incomplete`, because what the entry starts from is incomplete.
- `Stats.prologue_walks` counts prologue bodies walked.
- `__new__` and a metaclass `__call__` are ignored, as in step 03.01.

---

## What invalidates an entry

The analyser re-runs an entry when something it started from changes:

| entry | depends on |
|---|---|
| any function | its own body, and every summary its walk used |
| a method or property | also `C.__init__` (through the MRO) and what it calls, and the class's bases |
| a closure | also the parent function's body and what it calls, and the parent's own prologue |

Before summaries are salsa queries (growth path v2 in
[`15`](15-checks-and-build-order.md#growth-path)), re-run on a change to any of
those definitions. With v2, the prologue is part of the entry's query, and salsa
records the dependencies.

---

## What it replaces

- **`08-keys` "Entry contexts":** an entry's context was "every parameter
  `Unknown`". Now it is the prologue's values.
- **The unannotated-`self` / `cls` fallback** (`annotation_fallback` →
  `enclosing_self` in `src/interp/bind.rs`, from step 03.07). At an entry, the
  prologue supplies `self` and `cls`. Elsewhere, an unannotated first parameter
  that arrives `Unknown` stays `Unknown`. The rule also fired for any call with no
  receiver and an unknown first argument, not only at the entry, which this
  removes too.

Retire both, don't keep them next to the prologue.

---

## Fixtures

```python
# 1. function entry: annotation and default
def process(h: Handler, n=3): h.handle()                 # Handler.handle

# 2. method entry: __init__'s state, hidden __init__ nodes
class Pipeline:
    def __init__(self): self.handler = JsonHandler(); log()
    def run(self): self.handler.process()                # Pipeline.run > JsonHandler.process
                                                          # (no JsonHandler / log node: prologue)
# 3. inherited __init__ and super()
class Base:
    def __init__(self): self.h = JsonHandler()
class Child(Base):
    def __init__(self): super().__init__()
    def go(self): self.h.process()                       # Child.go > JsonHandler.process

# 4. only __init__ runs (pinned: the contract)
class Job:
    def __init__(self): self.h = A()
    def configure(self): self.h = B()
    def run(self): self.h.go()                           # Job.run > A.go

# 5. no __init__: the class side
class Plain:
    h: Handler
    def go(self): self.h.handle()                        # Plain.go > Handler.handle

# 6. classmethod and staticmethod entries
class Reg:
    @classmethod
    def create(cls): cls()                               # Reg.create > Reg
    @staticmethod
    def make(cb): cb()                                   # Reg.make (cb Unknown: no edge)

# 7. property entry
class Box:
    def __init__(self): self._h = JsonHandler()
    @property
    def h(self): return self._h.process()                # Box.h > JsonHandler.process

# 8. closure entry: free value at the end of the parent's run
def outer():
    h = Handler()
    def inner(): h.go()                                  # outer.inner > LoudHandler.go
    h = LoudHandler()
    return inner

# 9. closure inside a method: object first, then the method, then the closure
class Svc:
    def __init__(self): self.h = JsonHandler()
    def build(self):
        def step(): self.h.process()                     # Svc.build.step > JsonHandler.process
        return step

# 10. a recursive __init__ in the prologue stays hidden
class Tree:
    def __init__(self): Tree()
    def walk(self): log()                                # Tree.walk > log

# 11. three levels of functions
def a():
    x = A()
    def b():
        y = B()
        def c(): x.go(); y.go()                          # a.b.c > A.go, B.go
        return c
    return b

# 12. a class inside a function: its methods and __init__ read the function's run
def make():
    h = JsonHandler()
    class Local:
        def __init__(self): self.h2 = h
        def run(self): h.process(); self.h2.process()    # make.Local.run > JsonHandler.process ×2
    return Local

# 13. a class inside a class: only the inner object is built
class Outer:
    def __init__(self): log()
    class Inner:
        def __init__(self): self.h = JsonHandler()
        def m(self): self.h.process()                    # Outer.Inner.m > JsonHandler.process
```

---

## Build order

1. **E1.** Find the entry's kind (function, method, property, classmethod,
   staticmethod) from ty. Fixtures 1, 6.
2. **E2.** The method prologue in inline mode: construct with no arguments in a
   hidden frame, then walk the method on the object. Retire `enclosing_self`.
   Fixtures 2, 3, 4, 5, 7, 10.
3. **E3.** `sem.entry` into functions; the walk down the scope chain, with the
   run's end env stored. Fixtures 8, 9, 11, 12, 13.
4. **E4.** Summary mode: the same prologue in the root walk; the entry keyed
   with its values. Callee writes in `__init__` after part 04's effects.
5. **E5.** Invalidation: re-run an entry when anything in its "depends on" row
   changes.

E1–E3 fit after part 03 of the tutorial (`experience/12-call-tree/03-classes`).
E4 belongs with part 04.

---

→ Back: [`15-checks-and-build-order.md`](15-checks-and-build-order.md)
