# 00.02 — Why it is slow

You said speed is the only problem. Worth being precise about *which* slowness,
because it changes what the port must get right. Roughly ordered by cost.

---

## 1. A fresh Jedi `Script` per call site **[verified — the big one]**

```python
# call_resolver.py:74-77  — inside resolve_call_hierarchy
jm = JediProjectManager(self.jedi_manager.project_path)
self.script = jm.get_script(file_path)
```

and

```python
# jedi_manager.py:26-29
def get_script(self, path: str) -> jedi.Script:
    project = jedi.Project(path=str(self.project_path.parent))
    env = jedi.InterpreterEnvironment()
    return jedi.Script(path=path, project=project, environment=env)
```

`service.resolve_calls` constructs **one `CallHierarchyResolver` per top-level
call** (`service.py:120`), each of which builds a new `JediProjectManager`, a new
`jedi.Project`, a new `InterpreterEnvironment`, and a new `Script` — which
re-reads and re-parses the file.

Jedi's own module cache is keyed on the *environment*, so a fresh
`InterpreterEnvironment` per call site defeats much of it. **Every call site pays
project construction + file parse.** For a file with 200 call sites that is 200
parses of the same file before any inference happens.

> This alone is likely most of your wall-clock. It is also the one thing that
> a Rust port fixes structurally rather than incidentally — salsa makes
> "re-derive this" a hash lookup. See
> [`01-crates/02-the-salsa-db.md`](../01-crates/02-the-salsa-db.md).

## 2. Jedi inference is a tree-walking interpreter in Python

Jedi lazily infers by walking parso trees and allocating `Value` objects. Every
`helpers.infer()` call at `call_resolver.py:118` is interpreted Python doing
graph traversal with dict lookups and `isinstance` chains. There is no way to
make that fast; it is the wrong runtime for the job.

The recursive descent multiplies it: `_analyze_function` re-enters
`resolve_call_hierarchy_for_node` for every call in every callee body, at every
depth, for every path. Work is **exponential in depth** and nothing is memoised
across paths — by design (it's a tree), but with no cheap re-derivation either.

## 3. Cache keyed on whole file content

```python
# scanner.py:9
@lru_cache(maxsize=50)
def _inner_scan(content: str): ...
```

Keyed on the entire file text, so one keystroke is a total miss. It also hashes
the whole string on every call, and `maxsize=50` means a medium project evicts
constantly. And it caches only the *parse*, never inference.

## 4. GIL + threadpool

`rpc.py` wraps every handler in `run_in_threadpool`. That keeps the event loop
responsive but gives **zero parallelism** for the CPU-bound inference, since
Jedi is pure Python under the GIL. Concurrent `parse_file` requests serialise.

## 5. Three parsers per file

`parse_file` on one file runs:
1. `libcst.parse_module` (`id_injector.inject_ids`)
2. `parso.parse` (`JediParser.__init__`)
3. `parso.parse` again inside Jedi's `Script`, if `resolve_mro=True`

libcst is the slowest Python parser in common use, because it builds a lossless
concrete syntax tree with full whitespace nodes.

The fix is **not** to drop libcst — you still want it for ID injection, and the
Rust build of it is much faster than the Python one
([`02-mapping/02`](../02-mapping/02-id-injection.md)). The fix is to stop
running it *unconditionally*. Detect whether a file needs an ID from the ruff
AST you already parsed (free, cached), and only invoke libcst on the files that
will actually be modified — which after the first pass is almost none.

So: 3 parses every time → 1 cached parse, plus libcst on the cold path only.

## 6. MRO does a full `infer()` per class

`_apply_mro_to_classes` calls `resolve_mro` per class, each doing
`get_script()` (→ new Project, new Script, reparse) then `script.infer()`.
N classes = N project constructions.

## 7. Process + serialisation overhead

uvicorn + pydantic validation on every node in a deep tree. `model_dump(mode="json")`
over a large recursive structure is not free, and `CallFrameStack` carries a
`parent` back-reference that forced a hand-written `to_json_tree` to dodge
infinite recursion.

---

## What the port buys you, by cause

| Cause | Fixed by | Expected effect |
|---|---|---|
| 1. Script per call site | one salsa `ProjectDatabase` for process lifetime | **largest single win** |
| 2. Interpreted inference | Rust | 10–50× on the same algorithm |
| 3. Content-keyed cache | salsa revision + file-level invalidation | correct incrementality |
| 4. GIL | `db.snapshot()` + rayon across files | real multicore |
| 5. Three parsers | one cached `parsed_module`; libcst only when writing | ~3× on the syntax layer |
| 6. MRO per class | ty computes MRO once per class, cached | near-free |
| 7. Serialisation | serde, no `parent` cycles | modest |

## What the port does *not* fix by itself

The exponential-in-depth traversal is inherent to "one unique path per
function". Rust makes each step ~30× cheaper, but a pathological graph is still
pathological. Budget for the mitigations in
[`03-call-tree/08-termination-and-cycles.md`](../03-call-tree/08-termination-and-cycles.md):
depth caps, path budgets, and memoising subtrees whose environment is
irrelevant. **Do not skip that chapter** — without it you will hit inputs that
are slower in Rust than in Python, because Rust will happily explore a space
Python was too slow to reach.

---

→ Next: [`03-what-ruff-is.md`](03-what-ruff-is.md)
