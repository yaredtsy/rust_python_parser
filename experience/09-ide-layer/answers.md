# Answers 09 — The IDE layer

---

**1.**

```rust
let module = parsed_module(db, file.python_file(db)).load(db);
let model  = SemanticModel::new(db, file);
```

**[verified, `outgoing_calls.rs:34-35`]**. Memorise it; every analysis in this
port starts there.

**2.** `find_goto_target(&model, &module, offset)`, then it takes the
definitions the goto target resolves to. Same machinery as goto-definition —
which is the right reuse, since "what is the thing under the cursor" is one
question with one answer.

**3.** That the API is a **view of callees, not of call sites**: two calls to
the same function are one row with two ranges. That is nearly identical to your
quirk 6 (`add_child` dedupes by `target_qname` and increments `call_count`).

Both designs arrived at grouping for the same reason — a hierarchy display wants
one entry per callee. The difference is that yours also counts (`call_count`
starts at 0, so it is "extra calls beyond the first"), while ty keeps the actual
ranges.

**4.** Any three of: recursion into callees; project-code filtering (quirk 2);
builtin filtering by name (quirk 3); the no-ID drop (quirk 4); the ancestor
cycle guard (quirk 5); constructor entry through `__init__` (quirk 7); a budget
or depth cap; carrying an environment.

**5.** It tells you the protocol is **stateless and client-driven**: the editor
asks for one level, the user expands a node, the editor sends the item back and
asks for the next level. The server never holds a tree.

That does not fit your RPC because `resolve_calls` returns a **whole tree in one
response**, and the tree's shape depends on the path taken to each node. A
stateless per-node protocol cannot express "this node, reached this way" — the
client would have to send the path back, and the path is exactly what the LSP
item does not carry.

---

**6.**

| entry | reason |
|---|---|
| `twice` | **grouping semantics** — ty returns one `OutgoingCall` with two `from_ranges`; you need one child with `call_count: 1` |
| `recurse` | **no recursion and no cycle guard** — ty returns one level and stops because that is all it does, not because a guard fired |
| `calls_undocumented` | **no ID filter** — ty returns `no_id_callee`; quirk 4 says you drop it *and* do not descend |
| `diamond` | **graph vs tree** — ty is a per-node view; you need `leaf` to appear twice, once under each parent |
| `constructs` | **constructor semantics** — quirk 7 rewrites the target to `ClassSchema/…` and descends into `__init__`; ty just reports the class |
| `emit` | **path insensitivity** — the answer depends on the caller, and ty has one answer for all callers |

Six reasons, and only the last is unfixable. The first five are things you add on
top; the sixth is the reason there is a project.

**7.** Your tree contains **two** `leaf` nodes — one under `left`, one under
`right`. A call graph contains **one** `leaf` node with two incoming edges.

Quirk 1. It was chosen because the tool answers "what happens when I run
`diamond`", and the two activations of `leaf` are genuinely different events
with different contexts. A graph collapses them and loses the distinction that
the whole tool exists to show.

**8.** A global visited set says "I have seen `leaf` before, skip it" — which
would collapse the `diamond` case to one `leaf` and break quirk 1. The ancestor
guard says "`leaf` is not on the path from the root to here, so descend" — a
function may appear many times in the tree, just never inside itself.

Different trees: `diamond`. Global-visited gives `left → leaf` and `right →`
(nothing, already seen). Ancestor-guard gives both. `plan/03-call-tree/08` and
`/09` are the chapters.

---

**9.** Two — `JsonWriter.write` and `XmlWriter.write` (or one entry widened over
both, depending on how the union resolves). **Neither is wrong.** Both are
genuinely reachable somewhere in the program.

**10.** **No.** Not because a parameter is missing, but because the function
answers a different question: "which declarations could this call site reach,
across all runs". A path-specific answer is not a refinement of that answer — it
is a different computation, over a different input (an environment), which the
function neither takes nor could obtain.

The distinction is worth keeping crisp: this is not "ty is incomplete". It is
"ty is complete for its question".

**11.** You would need to thread an environment — parameter name → chosen value
— from the call site into inference, so that resolving `writer` consults the
environment before falling back to the declared type. That means every inference
query on the way down takes the environment as an input.

What that does to salsa: inference is memoised as `infer_scope_types(scope)`. Add
an environment to the key and nearly every call gets a distinct key. The cache
stores everything, hits almost nothing, and still pays full dependency-tracking
cost. `plan/03-call-tree/02` makes this argument; having sketched the change
yourself is what makes it convincing rather than assertive.

The conclusion follows: the environment lives in **your** interpreter, and ty
stays the oracle for environment-free questions.

**12.** Whatever you write here, the useful part is the second half of the
question. Things people commonly work out on their own but do not see stated:
that ty's grouping is nearly quirk 6, that `find_goto_target` is reusable, and
that five of the six differences are additive rather than fundamental. That last
observation is genuinely encouraging and the plan does not emphasise it — most
of the gap is scaffolding you can build, and only one part is the hard problem.

---

**13.** Take: the two-line prologue; `find_goto_target` for offset →
definition; the callee-resolution step (`ExprCall.func` → type → definition);
`CallHierarchyItem::from_definition`'s approach for building a display item from
a definition (you need the same for `target_qname` and `target_id`); the
visitor's structure for finding calls in a body.

Build: the recursive driver; the environment; the project filter; the builtin
filter; the ID lookup and drop; the ancestor guard; `call_count` merging; the
budget; constructor entry.

**14.** `document_symbols` includes variables, constants and class attributes as
symbols — you drop everything that is not a class, function or call.

You include **calls**, which no symbol view has: a symbol tree describes
definitions, and calls are uses. That single difference is why you could not
have built `parse_file` on top of `document_symbols`, even though the outputs
look similar at a glance.

**15.** "Who calls this function" — a find-callers or impact-analysis feature in
v-noc.

Your call tree cannot serve it directly: it is rooted at an entry point and
descends, so answering "who calls `leaf`" would mean searching every tree you
have ever built, and only finds callers on paths you happened to explore.
`incoming_calls` answers it properly, project-wide, from the semantic index. If
that feature ever comes up, use ty's — do not try to invert your tree.

---

**16.** Expect inference to dominate cold, and for the warm run to be dominated
by traversal (everything else is a cache hit). If parsing dominates, you are
probably re-parsing outside the db — check that you go through `parsed_module`.

The reason to measure: `plan/04-build/02`'s M8 asks for p50/p95/p99 against a
baseline, and knowing *which layer* the time is in decides whether the answer is
memoisation (`plan/03-call-tree/08` layer 5) or parallelism.

**17.** Something like:

```rust
fn resolve_calls<'db>(
    db: &'db dyn Db,
    file: ProgramFile<'db>,
    offset: TextSize,
    env: &Env<'db>,              // ← the environment. does not exist in ty's version
    frame: &Frame,               // ← the ancestor chain, for quirk 5 and identity
    budget: &mut Budget,         // ← depth/node/deadline caps
    tracer: &mut impl Tracer,    // ← the --trace output
) -> Vec<CallFrameStack>;        // ← recursive, not one level
```

versus

```rust
fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize) -> Vec<OutgoingCall>;
```

Four extra parameters and a recursive return type. `env` is the project;
`frame` and `budget` are the scaffolding that makes the recursion terminate;
`tracer` is what makes it debuggable. `plan/03-call-tree/03` builds exactly this.
