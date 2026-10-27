# 03.15 — Checks, known limits and build order

What the design deliberately misses, how it is tested, and the order to build
it in.

---

## Known limits

Each limit has a fixture that lists today's expected edges, including the
missing ones, so a change is noticed.

### The alias vector covers top-level parameters only

`alias` records when two parameters are the same object. It does not record when
one parameter is reachable from another's attributes.

```python
class Node:
    def link(self, other):
        self.peer = other

    def touch(self, other):
        other.h = JsonHandler()
        self.peer.h.process()       # Python: JsonHandler.process

def main():
    a = Node()
    b = Node()
    a.link(b)
    a.touch(b)                      # v1: no JsonHandler.process edge
```

In `touch`, `other` and `self.peer` are materialized as two origins, so the write
to `other.h` is not seen through `self.peer`. Fix later: record nested aliasing
in the key as pairs of paths. The same holds for two deep paths that name one
object ([`07`](07-attributes-and-self.md#objects-the-key-does-not-reach)).

### Writes to module globals are dropped

A module global is read through ty and lifts to an `OPAQUE` instance, which holds
no state. The registry pattern loses edges.

```python
class Registry:
    def add(self, h):
        self.h = h

    def fire(self):
        self.h.process()            # Python: JsonHandler.process

REGISTRY = Registry()

def setup():
    REGISTRY.add(JsonHandler())

def main():
    setup()
    REGISTRY.fire()                 # v1: no JsonHandler.process edge
```

Fix later: a `Root::Global` for module attributes a function reads or writes, in
both the key and the effects.

### `fp_depth = 2` loses reads at depth 3

```python
class Config:
    def __init__(self, h):
        self.handler = h

class App:
    def __init__(self, c):
        self.config = c

class Service:
    def __init__(self, a):
        self.app = a

    def run(self):
        self.app.config.handler.process()      # three steps from self

def main():
    Service(App(Config(JsonHandler()))).run()  # K = 2: no edge. K = 3: JsonHandler.process
```

`self.app.config` is fingerprinted as `Opaque(Config)`, so `.handler` falls to ty,
which knows no class for it. Writes at that depth reach the caller by path;
reads need the key. Measure K = 2 against K = 3 on the fixtures and on one real
project: edges found, contexts created, wall time. Set the default from those
numbers.

### Effects of a call inside a cycle reach only the exit state

The `Tree.fill` example in [`12`](12-recursion.md#known-limit-effects-inside-a-cycle-reach-only-the-exit-state).

### Identity does not go around a cycle

An object built deeper in a recursion keeps its class but not its attributes once
it leaves the group ([`12`](12-recursion.md#identity-does-not-go-around-a-cycle)).

```python
class Leaf:
    def __init__(self, h): self.h = h
class Branch:
    def __init__(self, child): self.child = child

def build(n, h):
    if n:
        return Branch(build(n - 1, h))
    return Leaf(h)

def main():
    build(2, JsonHandler()).child.h.process()
    # Python: JsonHandler.process. v1: `.child` is Opaque(Leaf | Branch),
    # so `.h` comes from ty's declared type.
```

### Rebinding a captured variable is not shared between closures

```python
class Pair:
    def __init__(self, a, b):
        self.a = a
        self.b = b

def make():
    h = CsvHandler()

    def set_json():
        nonlocal h
        h = JsonHandler()

    def run():
        h.process()

    return Pair(set_json, run)

def main():
    p = make()
    p.a()
    p.b()        # Python: JsonHandler.process. v1: CsvHandler.process
```

Each closure in a summary carries its own copy of its free values. Mutating a
captured object is shared, because both copies point at the same object; only
rebinding the name is lost. `nonlocal` is also outside the precision policy. Fix
later: shared cells, one per captured binding, carried in `Fp::Closure` and
`SVal::Closure` by reference.

---

## Permanent checks

### No-reuse mode

`limits.reuse = false` recomputes every `Ready` summary instead of reusing it.
Running, parked and resolving contexts still answer with a pending value, because
a cycle needs that. The output, graph and tree, must equal normal mode byte for
byte. Both runs must report no `Incomplete(Cut)`, so raise the cost limits and
turn the deadline off for the check.

What it checks: does reusing summaries change the graph? If a walk depends on
something its key does not hold, reuse gives a different answer than
recomputing, and the check fails the day that input appears. For recursion, it
also checks that a group's result doesn't depend on which member was entered
first.

What it does not check: that the graph is right. Both modes can drop the same
information and still agree. That is what the expected-edge fixtures are for.

No-reuse costs as much as the old exponential walk. Run it on the fixtures and
on small projects in CI, not on large ones. CLI: `pylspt-dev calls f.py --no-reuse`.

### Expected-edge fixtures

Every fixture lists its expected edges, including the ones missing on purpose
because of a known limit. A known-limit fixture fails when its gap closes, so the
change that closes the gap updates the fixture.

### Determinism

Same input, same output, ten runs in a row, with the deadline off or with no
`Cut::Deadline` reported ([`14`](14-output.md#determinism)).

### Parity with the Jedi driver

Parity means pylspt finds at least the Jedi driver's edges, and every difference
is explained. Differences are expected:

- Jedi stops after 6 executions of any one function and 200 per inference state
  (`per_function_execution_limit`, `total_function_execution_limit`). Past that
  it returns no values, so edges in busy functions are missing from its output.
- Jedi drops recursive calls. pylspt shows them as `Recursive` leaves.
- Values of recursive calls resolve in pylspt ([`12`](12-recursion.md)).
- Values flow through functions with no docstring ID; Jedi skipped those
  entirely.

An extra edge is not automatically a bug. A missing edge is a bug until it is
explained.

---

## Fixtures for the core

Fixtures 1 to 4 come first. Simple fixtures pass with all four bugs present, so
these are the ones that keep them out of real projects. Run every fixture against
its expected edges, and in no-reuse mode too. Each chapter's own fixtures are in
that chapter.

```python
# 1a. Ref resolves against the pre-call state
class Box:
    def __init__(self, h):
        self.h = h

    def replace(self, h):
        old = self.h
        self.h = h
        return old

def main():
    b = Box(JsonHandler())
    old = b.replace(CsvHandler())
    old.process()            # JsonHandler.process only
    b.h.process()            # CsvHandler.process only
```

```python
# 1b. a swap: both effects read the old state
class Pair:
    def __init__(self, x, y):
        self.x = x
        self.y = y

    def swap(self):
        t = self.x
        self.x = self.y
        self.y = t
        # `self.x, self.y = self.y, self.x` gives the same summary once tuple
        # destructuring is modelled. Today it binds Unknown (05).

def main():
    p = Pair(JsonHandler(), CsvHandler())
    p.swap()
    p.x.process()            # CsvHandler.process only
    p.y.process()            # JsonHandler.process only
```

```python
# 2. a cut result is never reused. Run with max_stack = 4.
#    JsonHandler defines no __init__, so constructing it walks nothing.
def leaf(): return JsonHandler()
def mid():  return leaf()
def top():  return mid()
def b():    top()
def a():    b()

def main():
    a()                      # top at depth 3, mid would be 4: cut. top, b, a are Incomplete(Cut)
    top().process()          # top at depth 1: walked again, JsonHandler.process found
# Tree: under main > a > b > top, the nodes a, b and top are marked Incomplete.
#       main > top > mid > leaf is complete, and main has a JsonHandler.process child.
#       main itself is Incomplete(Cut), because it read a's result. The run reports it.
```

```python
# 3. CPA combos get their own origins
class JsonKey:
    def encode(self): ...

class CsvKey:
    def encode(self): ...

class JsonHandler:
    def key(self): return JsonKey()
    def process(self, k): k.encode()

class CsvHandler:
    def key(self): return CsvKey()
    def process(self, k): k.encode()

class Job:
    def __init__(self, h, k):
        self.h = h
        self.k = k

    def go(self):
        self.h.process(self.k)

def build(h):
    return Job(h, h.key())   # split: one context per handler class

def main(c):
    h = JsonHandler() if c else CsvHandler()
    build(h).go()
# JsonHandler.process > JsonKey.encode only.
# CsvHandler.process  > CsvKey.encode only.
# Without the combo index, both encodes appear under both process nodes.
```

```python
# 4. free variables come from the closure's own activation
def make(h):
    def run():
        h.process()
    return run

def main():
    h = CsvHandler()         # same name as make's h, on purpose
    r = make(JsonHandler())
    r()                      # JsonHandler.process only
```

```python
# 5. a render cut is marked. Run with render_nodes = 3 (main counts).
def a(): pass
def b(): pass
def c(): pass

def main():
    a()
    b()
    c()
# main has children a and b. main is marked Cut.
```

```python
# 6. two calls that start at the same offset are two sites
def main():
    make_handler(JsonWriter())(1)
# main > make_handler and main > handle > JsonWriter.write (plus the JsonWriter
# construction): the two calls start at one offset but are two SiteIds.
```

The rest, by chapter:

| # | fixture | where |
|---|---|---|
| 7 | an effect through a bound-method argument (`Step::Recv`) | [`11`](11-closures-and-callbacks.md#bound-methods-as-callbacks) |
| 8 | a recursive call is recorded, not entered | [`12`](12-recursion.md#fixtures) 1 |
| 9 | a value cycle: `grow` returns only `JsonHandler`, `Finished` | [`12`](12-recursion.md#fixtures) 2 |
| 10 | a recursive call's value used in the same body: `walk > Handle.close` | [`12`](12-recursion.md#fixtures) 4 |
| 11 | two hops around a cycle: `a / b / c` | [`12`](12-recursion.md#two-hops-around-a-cycle) |
| 12 | the parser: a context that joins, entered from either end | [`12`](12-recursion.md#a-parser-a-context-that-joins) |
| 13 | the three-step visitor | [`12`](12-recursion.md#a-visitor-three-steps) |
| 14 | a write deeper than the key (`rewire`) | [`07`](07-attributes-and-self.md#fixtures) 12 |
| 15 | a write past `max_path`: weak `Unknown`, marked | [`07`](07-attributes-and-self.md#fixtures) 13 |
| 16 | a closure that captures itself | [`08`](08-keys.md#fixtures) 4 |
| 17 | a `Callable` value from ty | [`14`](14-output.md#stable-identity) |
| 18 | type sensitivity: one summary for equal fingerprints | [`08`](08-keys.md#fixtures) 1 |

---

## Build order

Each step has a gate. Do not start the next step until its gate passes.

| step | build | gate |
|---|---|---|
| S1 | `key.rs`, `summary.rs` and `stable.rs` types, `PlainVal`, `SiteId` as a range | compiles. A walk-local id inside an `SVal` does not compile |
| S2 | the walker produces a `FunctionResult` for one context. Project calls are recorded as edges, not followed | [`05`](05-the-walk.md#fixtures) fixtures 1–9 |
| S3 | `call_project` with snapshot resolve and apply. Effects from `written`. Input objects named by path, and the `max_path` fallback. The annotation fallback. Solver without recursion. The deadline. No-reuse mode | fixtures 1a, 1b, 6, 7, 14, 15. [`07`](07-attributes-and-self.md#fixtures) 1, 2, 9–11. [`09`](09-summaries.md#fixtures) and [`10`](10-one-call.md#fixtures) 1–6. No-reuse equality on all of them |
| S4 | recursion: pending values, deferred operations, groups, joining, merging, the fixpoint, the identity rule, the round cap | fixtures 8–13. All of [`12`](12-recursion.md#fixtures), including the parser entered from both ends under no-reuse. The parser's context count is checked again after S6 |
| S5 | **benchmark against the Jedi driver on one real repository** | numbers recorded: wall time, peak memory, summary reuse (reused gets per walk), edges missing and extra (as caller-to-callee pairs), `contexts_per_func` cuts, group sizes, rounds per group, contexts created per round, `CYCLE` gaps, `deep_write_fallbacks`, `types_dropped` |
| S6 | CPA split and combo origins | fixture 3. [`11`](11-closures-and-callbacks.md#fixtures) 9, both variants |
| S7 | closures from their own activations, `Back` references | fixtures 4, 16. [`11`](11-closures-and-callbacks.md#fixtures) 1–6. The `nonlocal` known limit |
| S8 | the remaining cost limits and `Cut` states | fixture 2. A deep chain (100 functions each calling the next: cut past `max_stack`, marked). A wide fan-out (10 functions each calling 10, depth 6: contexts stay linear, the view is cut by `render_nodes` and marked) |
| S9 | graph output, stable IDs (with `StableType::Callable`), render with marks | fixtures 5, 17. Determinism |
| S10 | the remaining known-limit fixtures. K = 2 vs K = 3, `max_path` and `max_rounds` measured | the alias, globals, depth and identity fixtures above |
| S11 | PyCG and Jarvis suites, projected to caller-to-callee edges. Full Jedi parity run | every difference explained |

S5 comes early on purpose. Once calls, returns, effects and recursion work, one
real repository says whether this design gives v-noc the improvement it needs,
before the rest is built. Edges that need S6 and S7 are expected to be missing at
that point. Run the benchmark again after S8 and S11.

The milestones around this ([`04-build/02`](../04-build/02-milestones.md)) put it
after the syntax layer, IDs and MRO, and before transport.

---

## Growth path

- **v1.** This folder, per request.
- **v1.1.** Keep slots across requests while ty's revision is unchanged. Only
  `Ready` slots are kept. `Cut` slots never are. The cache is keyed and stored
  in stable form ([`14`](14-output.md#stable-identity)), so it holds no `'db`
  borrow between requests. **[check]** the cost of resolving a `StableDef` back
  to a `Definition` when a cached summary is loaded. Clear the cache on any
  edit.
- **v1.2.** Precision, each one a change of the precision policy with its
  fixtures:
  - keep `bool`, `None` and small enum literals in keys, and prune `if` arms on
    literal conditions. This crosses "union at branches" on purpose, as a
    policy, not a walker change
  - container element fingerprints, `*args` as tuples
  - `Root::Global`, nested alias pairs
  - shared cells for `nonlocal`
  - effects inside a cycle seen by later statements: replay the statements
    after a pending call, not only the deferred operations
  - a loop fixpoint inside the walker
- **v1.3.** Read-set keys, postponed until they can be validated. The idea is to
  key only on the input paths the walk read. The risk is that a different input
  can make the walk read different attributes, so the read set from one walk is
  not enough on its own. Before shipping it, every reuse through a read-set key
  must give the same graph as the no-reuse check, on the whole corpus.
- **v2.** Summaries as salsa queries, so an edit reruns only the summaries that
  read what changed. **[check]** how a group maps onto salsa:
  - A deferred operation stores `before: Env`, which holds walk-local origins.
    Rule 2 forbids a query from returning that. Deferred operations must stay
    inside the query that walked them, or be rewritten in `SVal` terms first.
  - Salsa's cycle handling also iterates to a fixpoint from an initial value,
    the same idea as [`12`](12-recursion.md), but it re-runs the queries in the
    cycle, which re-walks bodies. One shape that keeps "each body walked once":
    a query per group head that walks the members and runs the fixpoint, and
    returns every member's summary.

  Decide when v2 starts. Rules 1 and 2 are what make either shape possible.

---

→ Next: [`04-build/00-dev-cli.md`](../04-build/00-dev-cli.md)
