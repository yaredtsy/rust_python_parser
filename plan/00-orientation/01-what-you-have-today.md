# 00.01 — What you have today

Inventory of `vnoc_lsp_python`, read module by module. **[verified]**

This document is the *specification*. The Rust port is correct when it reproduces
this behaviour, quirks included.

---

## Module inventory

| Module | LOC | Depends on | Role |
|---|---:|---|---|
| `server.py` | 50 | uvicorn, fastapi-jsonrpc | HTTP server, prints `READY port=<n>` |
| `rpc.py` | 91 | fastapi-jsonrpc, pydantic | 6 JSON-RPC methods, all via `run_in_threadpool` |
| `service.py` | 135 | — | orchestration |
| `models.py` | 36 | pydantic | `BaseNode` / `ClassNode` / `FunctionNode` / `CallNode` |
| `parser.py` | 184 | **parso** | syntax tree → nested node tree |
| `scanner.py` | 29 | — | `lru_cache(50)` over file content |
| `id_injector.py` | 185 | **libcst** | UUID injection into docstrings |
| `file_folder_ids.py` | 82 | **libcst** | `FileID:` / `FolderID:` module docstrings |
| `mro_resolver.py` | 77 | **jedi** (private API) | base classes via `py__mro__` |
| `call_resolver.py` | 340 | **jedi** (deep private API) | ★ the context-sensitive call tree |
| `jedi_manager.py` | 32 | jedi | Script/Project construction |

Three separate Python parsers are in play: **parso** (via jedi), **libcst**, and
CPython's own `ast` transitively. The port collapses all three onto one Ruff AST.

---

## The RPC contract (must be preserved exactly)

```
initialize(project_path, language)      → {status, extensions: [".py"]}
parse_file(file_path, content,
           resolve_mro)                 → {nodes: [BaseNode], content, modified}
resolve_calls(file_path, calls)         → {call_frame_stack: {...}}
read_or_inject_file_id(file_path)       → {file_id, modified}
read_or_inject_folder_id(folder_path)   → {folder_id, modified}
shutdown()                              → {status: "ok"}
```

### `BaseNode` wire shape

```jsonc
{
  "id": "FunctionSchema/<uuid>",   // or "ClassSchema/<uuid>"; null for calls
  "name": "do_thing",
  "type": "class" | "function" | "call",
  "position": { "line": 1, "column": 0, "end_line": 3, "end_column": 12 },
  "children": [ /* recursive */ ],

  // call only:
  "call_index": 0,
  "call_col_pos": 14,

  // class only:
  "base_classes": ["pkg.mod.Base", "builtins.object"]
}
```

**Line is 1-based, column is 0-based** — parso's convention. Ruff gives you byte
offsets. Conversion is mandatory and easy to get wrong; see
[`02-mapping/01-parso-to-ruff-ast.md`](../02-mapping/01-parso-to-ruff-ast.md).

### `CallFrameStack` wire shape

```jsonc
{
  "target_qname": "pkg.mod.Class.method",
  "target_id": "FunctionSchema/<uuid>",   // or ClassSchema/ for constructors
  "call_count": 0,
  "children": [ /* recursive */ ]
}
```

---

## Behavioural contract — the quirks that are *not* bugs to fix

These all come out of `call_resolver.py` and `parser.py`. **Preserve them.**

1. **It is a tree, not a graph.** The same callee reached from two different
   *frames* produces two distinct nodes. Two calls from the *same* frame merge
   into one — see quirk 6. This is the whole point of the design.
   → [`03-call-tree/09`](../03-call-tree/09-path-identity.md#the-merge-rule)

2. **Project-only traversal.** `_is_project_code` returns `True` only when the
   callee's file lives under the project path. Note the trailing `return False`
   at `call_resolver.py:310` — the `site-packages` / `lib/python` / `is_stdlib`
   checks above it are unreachable, so *everything* outside the project path is
   excluded regardless. Net effect: **only first-party code is descended into.**

3. **Builtins are skipped early**, by *name*, before inference —
   `BUILTIN_NAMES` at `call_resolver.py:22`. A user function named `list` is
   also skipped. Preserve.

4. **No ID → no node.** `_extract_id_from_docstring` returning `None` causes
   `continue` (`call_resolver.py:154`). Callees without an injected `ID:` are
   dropped from the tree entirely, *and are not descended into*.

5. **Cycle guard is ancestor-based**, not global-visited —
   `is_ancestor(qname)` walks the `parent` chain. A function may appear many
   times in the tree; it just may not appear inside itself.

6. **`add_child` dedupes by `target_qname` and increments `call_count`** rather
   than appending. Two calls to the same function from the same frame collapse
   into one child with `call_count == 1`. Note `call_count` starts at `0`, so
   it is really "extra calls beyond the first".

7. **Classes are entered through `__init__`.** `target_id` is rewritten to
   `ClassSchema/...` and the traversal descends into `__init__`'s body with a
   `BoundMethod` bound to the freshly constructed `TreeInstance`.

8. **Lambdas are dropped** — `parser.py:121` returns `None` for `lambdef`.

9. **Nested defs/classes terminate the scan.** `_scan_children` returns
   immediately on hitting a `Class`/`Function` (`parser.py:78-80`), so calls
   inside a nested function are children of *that* function, not the outer one.

10. **Position dedup.** `_scan_children` drops nodes sharing an identical
    4-tuple position.

11. **`async def` uses the parent `async_stmt` position** (`parser.py:126`), so
    the range starts at the `async` keyword.

12. **`call_index` counts call-trailers within one `atom_expr`.**
    `a.b().c()` produces two `CallNode`s, index 0 and 1, sharing `start_pos`
    but with different `end_pos` and `call_col_pos`. `call_col_pos` is the
    column of the `(`.

13. **Failures are swallowed everywhere.** Every level catches and logs.
    A partial tree is a valid result. The port must be equally forgiving —
    an analyser that returns `Err` on one weird file is a regression.

---

## Side effects (the driver writes to your source files)

| Trigger | Effect |
|---|---|
| `parse_file` | if any def/class lacks `ID:`, **rewrites the file on disk** (`scanner.py:24`) |
| `read_or_inject_file_id` | injects `FileID:` into module docstring, writes file |
| `read_or_inject_folder_id` | **creates `__init__.py` if absent**, injects `FolderID:` |

This is load-bearing: the IDs are the join key to the rest of v-noc. Any port
must keep writing them, and must keep the same `"""... \n\nID: <uuid>"""`
docstring format that `_extract_metadata`'s `(\S+)\s*:\s*(\S+)` regex reads back.

---

→ Next: [`02-why-it-is-slow.md`](02-why-it-is-slow.md)
