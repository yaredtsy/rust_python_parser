# 00.04 — The summary: what a call did

A `Ready` slot holds a `FunctionResult`: the call's summary. It is the only
thing a caller ever reads about a callee. One summary serves every caller with
the same key, so nothing in it may name a particular caller's object.

**Reading:** [`plan/03-call-tree/09-summaries.md`](../../../plan/03-call-tree/09-summaries.md).

---

## The structures

```rust
pub struct FunctionResult<'db> {
    pub calls:   Vec<CallEdge<'db>>,          // every call site reached, in evaluation order
    pub returns: SVals<'db>,                  // what it returns
    pub effects: Vec<Effect<'db>>,            // writes on objects the caller can see
    pub objects: Vec<NewObject<'db>>,         // objects it built that escape
    pub status:  Status,
}
```

### `calls`: where it called

```rust
pub struct CallEdge<'db> { pub site: SiteId, pub targets: SmallVec<[Target<'db>; 2]> }

pub struct SiteId { pub start: TextSize, pub end: TextSize }   // the call's range, relative to the `def`

pub enum Target<'db> {
    Call(ContextId),                                                // a project function, under this context
    Construct { class: Definition<'db>, init: Option<ContextId> },  // a class; its __init__'s context, if it has one
    External(Definition<'db>),                                      // stdlib or site-packages
    Unresolved,                                                     // the callee is unknown
}
```

One edge per call site, with every target it may reach. A site's range, not just
its start, because `f()()` begins two calls at the same offset.

### `returns`, `effects`, `objects`: values relative to the inputs

```rust
pub enum SVal<'db> {
    Plain(PlainVal<'db>),     // a value with no identity
    Ref(Path),                // "the caller's object at this path, when the call started"
    Obj(u16),                 // "objects[j]: an object I built"
    Bound   { func: Definition<'db>, recv: Box<SVal<'db>> },
    Closure { func: Definition<'db>, free: Box<[(Name, SVals<'db>)]>,      // boxed: an SVal
              defaults: Box<[(Name, SVals<'db>)]> },                       // can't hold SVals inline
    Back(u8),                 // the closure k levels up: one that captures itself
}

pub enum PlainVal<'db> { Function(..), Class(..), Module(..), Ty(Type<'db>), Unknown, Opaque(Definition<'db>) }

pub struct Path { pub root: Root, pub steps: SmallVec<[Step; 2]> }   // up to max_path steps
pub enum Root { Param(u8), Free(u8) }       // parameter i, or captured value i
pub enum Step { Attr(Name), Free(u8), Recv }

pub struct Effect<'db> {
    pub target: Path,                         // which of the caller's objects
    pub attr:   Name,                         // which attribute
    pub value:  SVals<'db>,                   // the new value
    pub strong: bool,                         // written on every path: the caller replaces, else joins
}

pub struct NewObject<'db> {
    pub class: Definition<'db>,
    pub site:  SiteId,                        // where it was built in this function
    pub attrs: Vec<(Name, SVals<'db>)>,       // its final state
}
```

`PlainVal` has no variant that can hold an origin or any other walk-local id.
That is the rule "nothing walk-local leaves a summary", enforced by the type: a
leak doesn't compile.

### `status`: how complete it is

```rust
pub struct Status { pub gaps: Gaps, pub cut: Option<Cut> }   // Finished = no gaps, no cut
// Gaps: CYCLE (a recursion group hit max_rounds), DEEP_WRITE (a write past max_path)
// Cut:  Stack, Contexts, ContextsPerFunc, Deadline
```

A summary with gaps is still reused: gaps depend only on the key. A cut summary
is never reused.

---

## The running example, at this level

**ctx#2, `Pipeline.__init__ {self: fp#0, writer: fp#1}`:**

```
calls:   []
returns: [Plain(Ty(None))]                                      falls off the end
effects: [Effect { target: Param(0), attr: writer, value: [Ref(Param(1))], strong: true }]
objects: []
status:  Finished
```

`self.writer = writer` became "the caller's parameter-0 object gets attribute
`writer` = the caller's parameter-1 object". No mention of which `Pipeline` or
which writer.

**ctx#3, `Pipeline.run {self: fp#2, data: fp#3}`:**

```
calls:   [CallEdge { site: `self.writer.write(data)`, targets: [Call(ctx#4)] }]
returns: [Ref(Param(0).writer)]                                 the caller's own writer object
effects: []
objects: []
status:  Finished
```

**ctx#4, `JsonWriter.write {self: fp#4, data: fp#4}`:**

```
calls: []   returns: [Plain(Ty(None))]   effects: []   objects: []   status: Finished
```

**ctx#1, `read_json {}`:**

```
calls:   [CallEdge { site: `JsonWriter()`,              targets: [Construct { JsonWriter, init: None }] },
          CallEdge { site: `Pipeline(JsonWriter())`,    targets: [Construct { Pipeline, init: Some(ctx#2) }] },
          CallEdge { site: `p.run(1)`,                  targets: [Call(ctx#3)] },
          CallEdge { site: `w.write(2)`,                targets: [Call(ctx#4)] }]
returns: [Plain(Ty(None))]
effects: []                                                     it has no inputs to write to
objects: []                                                     its two objects don't escape
status:  Finished
```

`JsonWriter()`'s edge comes before `Pipeline(...)`'s: arguments are evaluated
before the call that takes them.

**ctx#0, `main {}`:** `calls: [read_json() → Call(ctx#1), read_csv() → Call(ctx#5)]`,
everything else empty or `None`.

---

## Exercise

1. Write ctx#7's summary (`Pipeline.run` with a `CsvWriter`). How does it differ
   from ctx#3's?
2. If `read_json` ended with `return p`, what would change in its summary?
3. If `run` did `self.writer = CsvWriter()` before returning, write its effects
   and objects.

<details><summary>Answers</summary>

1. Only in its call edge: `targets: [Call(ctx#8)]` instead of ctx#4. Its
   returns are still `[Ref(Param(0).writer)]`: relative to the inputs, both
   contexts return "your writer".
2. `returns: [Obj(0)]` and `objects` gains two entries: the `Pipeline`
   (`attrs: [writer: [Obj(1)]]`) and the `JsonWriter` it holds, because it
   escapes through the pipeline.
3. `effects: [Param(0).writer = [Obj(0)], strong]`,
   `objects: [NewObject { class: CsvWriter, site: CsvWriter(), attrs: [] }]`. And
   the return would become `[Obj(0)]`, since it reads `self.writer` after the
   write.

</details>

---

→ Next: [`05-the-walk.md`](05-the-walk.md)
