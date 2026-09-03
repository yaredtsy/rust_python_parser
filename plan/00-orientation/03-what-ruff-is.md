# 00.03 — What "Ruff" actually is

You said "ruffs, crates, and what they are". This is that chapter.

---

## Ruff is not one thing

`github.com/astral-sh/ruff` is a **Cargo workspace of 54 crates** that ships
*two* products:

```
astral-sh/ruff  (one repo, one workspace)
│
├── ruff ......... the linter + formatter CLI          → published, v0.16.5
│   └── built from the ruff_* crates
│
└── ty ........... the type checker + language server  → published separately, v0.0.x
    └── built from the ty_* crates (which use the ruff_* crates)
```

**[verified]** at `ac201b8`: `ruff` is `0.16.5`, the shared libraries are
`0.0.11`, and `ty_ide` / `ty_project` are `0.0.0` with `publish = false`.

When you said "ruff looks faster", the part you actually need is mostly **ty**.
Ruff-the-linter has no type inference at all.

---

## The split that will confuse you

There are two crates with nearly the same name and completely different jobs:

| | `ruff_python_semantic` | `ty_python_semantic` |
|---|---|---|
| Built for | the **linter** | the **type checker** |
| Model | bindings, scopes, references | full type inference |
| Answers | "is this name defined? shadowed? unused?" | "what *type* is this expression?" |
| Knows about imports | resolves names, not modules across files | resolves modules, reads typeshed stubs |
| Cross-file | **no** | **yes** |
| Cost | very cheap, single pass | expensive, salsa-cached |
| Use it for | nothing in your port | **everything inferential** |

> **If a doc, blog post, or model output tells you to use `ruff_python_semantic`
> for type information, it is wrong.** It has no types. It is a scope/binding
> table for lint rules. You want `ty_python_semantic`.

Same trap with `SemanticModel`: **both crates define a struct with that name.**
Yours is `ty_python_semantic::SemanticModel`.

---

## The layer cake

Read bottom-up. Each layer only depends on the ones below it.

```
┌──────────────────────────────────────────────────────────────┐
│  ty_server · ruff_server            LSP servers              │  ← you replace this
├──────────────────────────────────────────────────────────────┤
│  ty_ide                             goto-def, hover,         │  ← you borrow from this
│                                     call_hierarchy, symbols  │
├──────────────────────────────────────────────────────────────┤
│  ty_project                         ProjectDatabase,         │  ← you instantiate this
│                                     config discovery, walk   │
├──────────────────────────────────────────────────────────────┤
│  ty_python_semantic                 ★ TYPE INFERENCE ★       │  ← your oracle
│  ty_python_core                     semantic index, scopes,  │
│                                     definitions, use-def     │
│  ty_module_resolver                 import → file            │
│  ty_vendored                        bundled typeshed stubs   │
├──────────────────────────────────────────────────────────────┤
│  ruff_db                            salsa Db, File, source,  │  ← the substrate
│                                     parsed_module cache      │
├──────────────────────────────────────────────────────────────┤
│  ruff_python_parser                 source → AST + tokens    │  ← replaces parso
│  ruff_python_ast                    AST types, visitors      │     and libcst
│  ruff_text_size                     TextSize / TextRange     │
│  ruff_source_file                   LineIndex (offset↔line)  │
└──────────────────────────────────────────────────────────────┘
```

---

## Why it is fast (the three reasons)

**1. Rust, obviously.** But that is the least interesting reason.

**2. Byte offsets, not line/column.** Every AST node carries a
`TextRange { start: TextSize, end: TextSize }` — two `u32` byte offsets.
No string splitting, no line arrays, no `(line, col)` tuples threaded through
inference. Line/column is computed *once, on demand*, via a `LineIndex`.

> This is why parso feels heavy: every node holds `start_pos`/`end_pos` tuples
> and computing them requires knowing where the newlines are. Ruff defers that
> to the boundary.

**3. Salsa.** ty is built on [salsa](https://github.com/salsa-rs/salsa), an
incremental-computation framework. Functions marked `#[salsa::tracked]` memoise
on their inputs and record a dependency graph. Change one file, bump the
revision, and *only* the queries that transitively read that file re-run —
everything else is a hash lookup.

This is the structural fix for [problem #1 in the previous
chapter](02-why-it-is-slow.md#1-a-fresh-jedi-script-per-call-site-verified--the-big-one).
Jedi rebuilds; ty *remembers*. See
[`01-crates/02-the-salsa-db.md`](../01-crates/02-the-salsa-db.md).

---

## What ty gives you, mapped to your driver

| You need | ty crate | Status |
|---|---|---|
| parse a file | `ruff_python_parser` | drop-in, better |
| walk the tree | `ruff_python_ast::visitor` | drop-in, better |
| resolve `import` | `ty_module_resolver` | far better than jedi |
| MRO of a class | `ty_python_semantic` | drop-in, cached |
| type of an expression | `ty_python_semantic` | **different semantics** — see 02-mapping/04 |
| **call tree with context** | — | **does not exist. you build it.** |

That last row is the project.

---

## What ty is *not*

- **Not a runtime.** It never executes Python. Neither does jedi, but jedi's
  lazy `Value` model *feels* like execution — that resemblance is what your
  `call_resolver.py` exploits, and it is the thing ty does not offer.
- **Not complete.** ty is pre-1.0. Some inference is `Unknown`. Your
  interpreter must treat `Unknown` as a normal, frequent outcome, not an error.
- **Not stable API.** These are internal crates of a fast-moving project.
  Pin a commit. See [`01-crates/04-public-vs-private-api.md`](../01-crates/04-public-vs-private-api.md).

---

→ Next: [`01-crates/01-crate-map.md`](../01-crates/01-crate-map.md)
