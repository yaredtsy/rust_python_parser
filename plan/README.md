# pylspt — porting the v-noc Python driver from Jedi to Ruff/ty

> **Goal:** same behaviour, same output, 10–100× faster.
> **Non-goal:** fixing analysis logic. Behaviour that exists today is treated as
> the specification, including its quirks.
>
> **The contract is the JSON, not the code shape.** Much of `call_resolver.py`
> is calls into Jedi's machinery (`create_context`, `helpers.infer`,
> `as_context`, `TreeInstance`), not logic the driver owns. Where ty offers a
> better mechanism for the same observable result, take it — transliterating
> Python into Rust is not the goal.

---

## The one thing to understand before reading anything else

Your current driver has two layers, and they port *completely differently*:

| Layer | What it does | Ruff/ty story |
|---|---|---|
| **Syntax layer** (`parser.py`, `id_injector.py`, `file_folder_ids.py`) | parso/libcst tree walks, docstring ID injection | **Direct port.** Ruff has faster, better equivalents. Mechanical work. |
| **Inference layer** (`call_resolver.py`, `mro_resolver.py`) | Jedi's `as_context(arguments)` — context-sensitive execution simulation | **No equivalent exists.** ty answers a *different question*. You must build this yourself, on top of ty. |

`ty_ide` ships a `call_hierarchy` module. It will look like the answer. **It is not.**
It resolves a call site to *every declaration the callee could be*, statically, once.
You need *which one it actually is, on this path, given these arguments*.
See [`03-call-tree/02-why-ty-alone-cannot.md`](03-call-tree/02-why-ty-alone-cannot.md).

**Budget your effort accordingly: ~20% on everything else, ~70% on `03-call-tree/`, ~10% on plumbing.**

---

## New to Ruff and ty? Read the tutorial first

This plan assumes you already know how ty works. If you don't yet, start at
[`../tutorial/README.md`](../tutorial/README.md) — 12 chapters that teach
Ruff/ty from a Jedi background, plus the Rust you need to read the code.

---

## Reading order (top to bottom)

```
plan/
│
├── README.md ...................................... you are here
│
├── 00-orientation/ ................................ what you have, why it hurts
│   ├── 01-what-you-have-today.md .................. inventory + the contract to preserve
│   ├── 02-why-it-is-slow.md ....................... where the milliseconds actually go
│   └── 03-what-ruff-is.md ......................... ruff_* vs ty_*, the split that confuses everyone
│
├── 01-crates/ ..................................... the toolbox
│   ├── 01-crate-map.md ............................ every crate, tiered by whether you care
│   ├── 02-the-salsa-db.md ......................... Db, File, ProgramFile, incremental caching
│   ├── 03-python-version.md ....................... ⚠ THE TRAP. Read twice.
│   └── 04-public-vs-private-api.md ................ what's pub, what isn't, and the fork decision
│
├── 02-mapping/ .................................... the mechanical ports
│   ├── 01-parso-to-ruff-ast.md .................... parser.py    → ruff_python_ast
│   ├── 02-id-injection.md ......................... id_injector.py → the libcst crate
│   ├── 03-jedi-mro-to-ty-mro.md ................... mro_resolver.py → ty MRO
│   └── 04-jedi-inference-to-ty.md ................. the conceptual gap, precisely stated
│
├── 03-call-tree/ .................................. ★ THE HARD PART ★
│   ├── 01-what-jedi-actually-does.md .............. read the mechanism you're replacing
│   ├── 02-why-ty-alone-cannot.md .................. why outgoing_calls is not your answer
│   ├── 03-the-abstract-interpreter.md ............. the architecture
│   ├── 04-value-domain.md ......................... what an "abstract value" is
│   ├── 05-binding-arguments.md .................... call site → parameter environment
│   ├── 06-attributes-and-self.md .................. self.handler.run(), the object model
│   ├── 07-callbacks-and-higher-order.md ........... functions as values
│   ├── 08-termination-and-cycles.md ............... why this halts
│   ├── 09-path-identity.md ........................ your "unique path per function" invariant
│   └── 10-return-values-and-state.md .............. value flow: returns, assignments, mutation
│
├── 04-build/ ...................................... shipping it
│   ├── 00-dev-cli.md .............................. ★ file in, JSON out. build this first
│   ├── 01-wiring-cargo.md ......................... dependencies that actually resolve
│   ├── 02-milestones.md ........................... ordered, each independently verifiable
│   └── 03-transport-and-parity.md ................. JSON-RPC + proving equivalence
│
└── 05-reference/
    ├── api-cheatsheet.md .......................... verified API surface, copy-paste
    └── glossary.md ................................ jedi word ↔ ty word
```

---

## Provenance of the facts in this plan

Everything marked **[verified]** was read out of your local checkouts on 2026-09-02:

- Ruff: `/Users/yared/Documents/Programing/ruff` @ `ac201b8`, ruff `0.16.5`,
  ty crates `0.0.11`, edition 2024, toolchain `1.98.0`.
- Driver: `/Users/yared/Documents/Programing/ide/v-noc/src/lsp/py/vnoc_lsp_python` (all 13 modules).

Anything marked **[check]** is a design assumption you should confirm against the
code when you get there. Ruff's internals move fast; `pub(crate)` today may be
`pub` in three months, and vice versa.

---

## Start here

→ [`00-orientation/01-what-you-have-today.md`](00-orientation/01-what-you-have-today.md)
