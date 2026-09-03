# 03.02 — Why `ty_ide::outgoing_calls` is not your answer

You will find this module, it will look like the whole project is done, and it
is important that you understand precisely where it stops. **Read
`ty_ide/src/call_hierarchy/outgoing_calls.rs` (797 lines) yourself** — this
chapter is the summary, not a substitute.

---

## What it does **[verified]**

```rust
pub fn outgoing_calls(db: &dyn Db, file: ProgramFile<'_>, offset: TextSize)
    -> Vec<OutgoingCall>

pub struct OutgoingCall {
    pub to: CallHierarchyItem,        // the callee
    pub from_ranges: Vec<TextRange>,  // where it's called from
}
```

Algorithm:
1. `find_goto_target` at `offset` → the function/class under the cursor.
2. Resolve it to `Definition`s.
3. Walk its body with a `SourceOrderVisitor` (`OutgoingCallsFinder`), **stopping
   at nested callables** — the same rule as your `parser.py:78-80`.
4. For each callee leaf: `goto_target.definitions(model, ResolveAliases)` →
   `ResolvedDefinition`s.
5. **Group by `CalleeKey { file, selection_range }`**, folding multiple call
   sites into one entry.

## The five reasons it isn't enough

### 1. It is one level deep
No recursion. You'd have to call it repeatedly, which is fine — but see #2.

### 2. It has no context parameter
```rust
outgoing_calls(db, file, offset)
//             ^^^^^^^^^^^^^^^^^  that is the entire input
```
There is nowhere to say "…given that `writer` is a `JsonWriter`". So recursing
into a callee gives you the *same* answer regardless of how you got there. Every
path through `emit` produces identical children. **Your tree collapses into a
graph.** That is the whole property you're trying to keep.

### 3. It resolves declarations, not values
```rust
let definitions = goto_target
    .definitions(self.model, ImportAliasResolution::ResolveAliases)
    .and_then(|d| d.goto_declaration(self.model, &goto_target))?;
for resolved in &definitions { /* every one becomes a child */ }
```
For `writer.write(x)` with `writer: JsonWriter | CsvWriter`, you get **both**
`write` methods as children — a union, flattened, always. Jedi would give you
the one that this path passed in.

### 4. It deliberately groups
```rust
let mut groups: FxHashMap<CalleeKey, (CallHierarchyItem, Vec<TextRange>)>
// "Use a stable group key so multiple call sites to the same callee fold into
//  one outgoing entry."
```
Correct for an LSP panel. Wrong for you — though note your `add_child` *also*
dedupes by qname, so this one is closer to your semantics than it first appears.
The difference is that yours dedupes per *frame*, theirs per *query*.

### 5. It has no notion of `ID:`, `call_index`, `call_col_pos`, or your schema
Cosmetic, but it means the output types need translating anyway.

---

## The illustration

⚠ **Pick the example carefully.** Two calls from the *same* frame is the wrong
demonstration, because `add_child` merges them by qname
([`09`](09-path-identity.md#the-merge-rule)) and the merged output happens to
look like ty's union anyway:

```python
def main():
    emit(JsonWriter(), a)      # both from main → ONE emit node,
    emit(CsvWriter(),  b)      # both writes accumulate inside it
```
```
root                          ty gives roughly the same picture here.
└── emit  (call_count=1)      Useless as a proof.
    ├── JsonWriter.write
    └── CsvWriter.write
```

The real difference shows only when the calls come from **different frames**:

```python
def emit(writer, data):
    writer.write(data)          # ← the interesting line

def read_json():
    emit(JsonWriter(), data)

def read_csv():
    emit(CsvWriter(), data)

def main():
    read_json()
    read_csv()
```

**Your Jedi resolver:**
```
root
└── main
    ├── read_json
    │   └── emit
    │       └── JsonWriter.write     ← ONLY this one. This path passed JsonWriter.
    └── read_csv
        └── emit
            └── CsvWriter.write      ← ONLY this one.
```

Two `emit` nodes — not because there were two call sites, but because they sit
under **different parents**. Each one shows only the callee its own path
reaches.

**`ty_ide::outgoing_calls`, applied recursively:**
```
main
├── read_json
│   └── emit
│       └── write → { JsonWriter.write, CsvWriter.write }   ← BOTH. wrong.
└── read_csv
    └── emit
        └── write → { JsonWriter.write, CsvWriter.write }   ← BOTH. wrong.
```

Same answer under both parents, because `outgoing_calls(db, file, offset)` has
no way to know which parent it is under.

**That is the whole gap.** Your tree says *"reading JSON reaches
`JsonWriter.write`, and nothing else."* ty says *"`emit` might reach either,
always."*

---

## What you should take from it anyway

A lot. Don't dismiss the module — **steal its skeleton**:

| Take | Why |
|---|---|
| `OutgoingCallsFinder`'s visitor structure | it already implements your "stop at nested callables" rule, correctly, including the subtle bits |
| `walk_callable_signature` / `walk_class_signature` | handles decorators, defaults, annotations, type-params, base-class exprs — call sites in places you'd forget |
| `CalleeLeaf::resolve` | maps a call expression to the identifier leaf you resolve — your `get_name_of_position` equivalent, done properly |
| `CallHierarchyItem::from_definition` | `Definition` → name/kind/range extraction |
| The not-`#[salsa::tracked]` decision | documented in the header comment; follow it |
| `incoming_calls.rs` (1308 lines) | if you ever want reverse edges |

**Concretely:** copy `OutgoingCallsFinder` into your crate, keep the traversal
verbatim, and replace only `record_callee` — where it calls
`goto_target.definitions(...)`, you instead consult your environment first and
fall back to `definitions(...)` when the environment has nothing. That is a
~40-line change to an 800-line file you get for free, and it inherits every edge
case the ty team already fixed.

That substitution is the entire architecture of the next chapter.

---

→ Next: [`03-the-abstract-interpreter.md`](03-the-abstract-interpreter.md)
