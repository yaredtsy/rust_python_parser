# experience/ — learn Ruff and ty by touching them

The `tutorial/` explains the ideas. The `plan/` says what to build. **This folder
makes you type things and find out.**

Twelve exercises. Each one is a folder. Each folder holds:

```
NN-topic/
├── README.md ....... the lesson + the work to do, step by step
├── python/ ......... Python files to run your Rust against (already written for you)
├── exam.md ......... questions. answer them before moving on
└── answers.md ...... the key. open it AFTER you have written your answers.
```

---

## The rule that makes this work

> **Predict, then run.**

Every step tells you to write down what you expect *before* you run the thing.
The value is not in the output — you could read the output in a doc. The value
is in the gap between what you predicted and what happened. That gap is your
mental model correcting itself, and it is the whole point of this folder.

If you skip the prediction, you are reading a tutorial with extra steps.

The second rule: **do not read `answers.md` until you have written your own
answers down.** Recognising a right answer feels identical to knowing it, and is
not the same thing.

---

## What this covers, and what it deliberately does not

**Covers:** everything in `tutorial/` (all 12 chapters) and the parts of `plan/`
that describe *what exists in Ruff and ty and how it behaves* —
`00-orientation/`, `01-crates/`, `02-mapping/`, `04-build/`, `05-reference/`.

**Does not cover:** building the abstract interpreter (`plan/03-call-tree/`).
That is the ~70% of the port that is *your design work*, and it is the wrong
thing to learn from. You cannot get a mental model of a machine you have not
built yet.

But you **do** need to know exactly where ty stops being enough, because that
boundary is the entire reason the plan exists. So exercise **09** walks you
through ty's own `outgoing_calls` and has you find, empirically, the exact input
where it gives the wrong answer for your purposes. You will not build the
replacement. You will just see the hole with your own eyes.

After exercise 11 you re-read `plan/03-call-tree/` and it will read like a
description of a problem you already have, rather than a description of a
solution you do not understand.

---

## The dendrogram

Read top to bottom. Every exercise assumes the ones above it.

```
experience/
│
├── 00-setup/ .............................. a crate that compiles against ty
│     No Python, no analysis. Cargo, git dependencies, feature flags,
│     the toolchain pin. Ends with: your binary prints a Python version.
│                                       covers: plan/04-build/01, plan/01-crates/04
│
│  ── GENERIC: things that are true of every file, before any meaning ──
│
├── 01-source-and-positions/ ............... bytes, offsets, lines, columns
│     TextSize, TextRange, Ranged, LineIndex. Why ty does not carry
│     (line, column) around. Where your wire format's numbers come from.
│                                       covers: tutorial/04, plan/02-mapping/01
│
├── 02-parse-and-ast/ ...................... text becomes a tree
│     ruff_python_ast node shapes, SourceOrderVisitor, TraversalSignal,
│     call chains, docstrings. This is your parser.py port, learned by hand.
│                                       covers: tutorial/05, plan/02-mapping/01
│
├── 03-the-database/ ....................... the thing that makes it fast
│     salsa, ProjectDatabase, File vs PythonFile vs ProgramFile,
│     parsed_module, invalidation, snapshots. Measure the cache hit.
│                                       covers: tutorial/06, plan/01-crates/02
│
├── 04-python-version/ ..................... ⚠ the silent wrong default
│     Where the version comes from, what breaks when it is wrong,
│     and the f-string trap that moves your column numbers.
│                                       covers: tutorial/07, plan/01-crates/03
│
│  ── SPECIFIC: meaning. names, files, types ──
│
├── 05-modules-and-imports/ ................ finding the other files
│     ModuleName, resolve_module, file_to_module, typeshed,
│     search paths, and "is this my code?" — your project filter.
│                                       covers: tutorial/07, plan/00-orientation/01
│
├── 06-scopes-and-definitions/ ............. where a name lives
│     semantic_index, ScopeId, Definition, goto_definition,
│     definitions_for_name, building a qualified name.
│                                       covers: tutorial/08, plan/05-reference
│
├── 07-types-and-inference/ ................ what an expression is
│     Type, HasType, the variants, Unknown, unions,
│     flow-sensitive vs context-sensitive — the important distinction.
│                                       covers: tutorial/09, plan/02-mapping/04
│
├── 08-classes-and-mro/ .................... objects
│     ClassLiteral, MRO / C3, attribute lookup, qualified-name parity
│     with Jedi, and the public-vs-private API wall you will hit.
│                                       covers: plan/02-mapping/03, plan/01-crates/04
│
├── 09-ide-layer/ .......................... what ty already built for you
│     goto, symbols, call_hierarchy, outgoing_calls. Read it, run it,
│     then find the input where it answers the wrong question.
│                                       covers: tutorial/09+10, plan/03-call-tree/02
│
├── 10-lossless-edits/ ..................... writing files back
│     Why the Ruff AST cannot round-trip, libcst, ID injection,
│     idempotency, and the salsa write-ordering bug.
│                                       covers: plan/02-mapping/02
│
└── 11-capstone/ ........................... put it together
      A dev CLI: file in, JSON out. Node tree with real positions,
      real IDs, real MRO. Plus the final exam over everything.
                                        covers: plan/04-build/00, plan/04-build/02
```

---

## How long this takes

| Exercise | Reading | Doing | Notes |
|---|---|---|---|
| 00 | 20 min | 1–2 h | most of it is a cold `cargo build`, walk away |
| 01 | 20 min | 1 h | |
| 02 | 40 min | 3–4 h | the biggest of the early ones |
| 03 | 30 min | 2 h | |
| 04 | 30 min | 1 h | short, high value |
| 05 | 20 min | 1–2 h | |
| 06 | 40 min | 2–3 h | |
| 07 | 40 min | 3 h | ★ the conceptual centre |
| 08 | 30 min | 2–3 h | |
| 09 | 60 min | 2 h | mostly reading ty's source |
| 10 | 30 min | 2–3 h | |
| 11 | 20 min | 4–6 h | assembly, not new ideas |

Call it two focused weeks, or a month of evenings. **07 and 09 are the two that
matter most.** If you are short on time, do 00, 01, 02, 03, 07, 09.

---

## Setup, once

Exercise 00 covers this properly. The short version, so you know what is coming:

- You depend on Ruff/ty **from git, pinned to one revision**. No submodule, no
  vendored workspace, no path dependencies.
- `ty_ide`, `ty_project` and `ty` are `publish = false`, so crates.io is not an
  option for them — git is the only route. The `ruff_*` crates are on crates.io
  but you take them from the same git rev anyway, so there is exactly one copy
  of everything in your dependency graph.
- The pin is `ac201b8` — the same revision the `plan/` was verified against, so
  every `[verified]` note in the plan holds for your build too.

One consequence to accept up front: with a git dependency you get the **public**
API only. Anything `pub(crate)` in ty is unreachable. That is fine for all
twelve exercises — it becomes a real constraint only when you build the
interpreter, and `plan/01-crates/04-public-vs-private-api.md` is where you make
that decision later, with evidence you will have collected by then.

---

## A note on the code you write

**You write the Rust. All of it.** These exercises give you:

- the exact API surface you need (signatures, verified against `ac201b8`)
- the shape of what to build (types, function boundaries, what returns what)
- the traps, before you fall into them
- Python files to run against, and the output to expect

They do not give you working Rust to paste. A solution you pasted teaches you
where the file is; a solution you fought for teaches you the API.

When you are stuck for more than 30 minutes on the same compiler error, that is
not learning any more — go read the corresponding source file in ty. Exercise 09
and `tutorial/11-reading-the-source.md` teach you how to find it fast.

---

## Progress

Tick these off. Each is the exercise's own "done when" line.

- [ ] **00** — `cargo run` prints a resolved Python version for a real project
- [ ] **01** — you can convert any node range to your wire format's line/column
- [ ] **02** — you emit a nested node tree for a file: classes, functions, calls
- [ ] **03** — you can measure a salsa cache hit, and prove an edit invalidates it
- [ ] **04** — you can state where the version came from, and show a file that parses differently
- [ ] **05** — you can answer "is this file project code?" for any resolved callee
- [ ] **06** — you can print a Jedi-style qualified name for any def in a file
- [ ] **07** — you can print the type of every call's callee, and explain each `Unknown`
- [ ] **08** — you can print base classes matching `mro_resolver.py`'s output
- [ ] **09** — you have an input where `outgoing_calls` is right and useless
- [ ] **10** — you can inject IDs into a file, twice, with the second run a no-op
- [ ] **11** — `pylspt-dev parse file.py` emits your node JSON

---

→ Start: [`00-setup/README.md`](00-setup/README.md)
