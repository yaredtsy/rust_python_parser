# 11 — Capstone: file in, JSON out

**Goal:** `pylspt-dev parse corpus/calls.py` prints your node tree as JSON, with
real positions, real IDs and real base classes — and a snapshot suite that tells
you when you break it.

No new concepts. This is assembly, and it is where the ten previous exercises
stop being separate things.

---

## Read first

- `plan/04-build/00-dev-cli.md` — this exercise builds most of it
- `plan/04-build/02-milestones.md` — you are completing M1–M4

---

## What you are building

```
src/
├── lib.rs              ← all analysis. every exercise added a module here.
├── db.rs               (ex 00, 03, 04)
├── position.rs         (ex 01)
├── nodes.rs            (ex 02)
├── modules.rs          (ex 05, 06)
├── types.rs            (ex 07)
├── mro.rs              (ex 08)
├── inject.rs           (ex 10)
└── bin/
    └── pylspt-dev.rs   ← argument parsing and printing. NO analysis.
```

**The one rule** from `plan/04-build/00-dev-cli.md`:

> The CLI must call the exact same functions the RPC layer calls.

Not a simplified path, not a reimplementation. If the CLI and the server can
disagree, your fixtures stop proving anything about what ships. The CLI is a
different *transport*, nothing more.

### Commands for this exercise

| command | RPC equivalent | you built it in |
|---|---|---|
| `version` | — | ex 00 + 04 |
| `parse <file>` | `parse_file` | ex 02 + 01 + 08 |
| `ids <file>` | `read_or_inject_file_id` | ex 10 |
| `mro <file>` | `parse_file(resolve_mro=true)` | ex 08 |
| `outgoing <file> <pos>` | — (diagnostic) | ex 09 |
| `all <file>` | — | this exercise |
| `test <dir>` | — | this exercise |

`calls` is **not** on the list. That is M5–M6, and it needs
`plan/03-call-tree/`.

### Flags

```
-o, --out <path>        write JSON here (default: stdout)
    --project <dir>     project root (default: walk up for pyproject.toml/.git)
    --pretty            indented JSON
    --write             ★ actually write ID injections (default: DRY RUN)
    --stats             timing + counters to stderr
    --repeat <n>        run n times, assert identical output
```

---

## Build it

### Step 1 — the single-file mode problem

Most fixtures have no project, no `pyproject.toml`, no venv. Handle it up front
or you will write scaffolding for every test file:

> No project root found → synthesise minimal metadata rooted at the file's
> parent, `python_version = latest_ty()`, search path = that directory.

Add `--python-version 3.12` as an override so you can pin a fixture without a
config file. And remember exercise 04's conclusion: the version is a **startup**
decision, made when you build `ProjectMetadata`, not per parse.

### Step 2 — wire the pipeline

```
path
  → ProjectMetadata::discover (or synthesise)      ex 00
  → ProjectDatabase                                 ex 03
  → system_path_to_file → db.program_file(file)     ex 03
  → parsed_module(...).load(db)                     ex 02
  → your scanner → Vec<Node>                        ex 02
      ├── positions via line_index(db, file)        ex 01
      ├── ids via docstring scan                    ex 02 / 06
      └── base_classes when --mro                   ex 08
  → serde_json
```

Every arrow is something you already wrote. If any of them needs rewriting to
fit, that is the exercise doing its job — find out now, not in M6.

### Step 3 — stdin mode

```bash
echo 'def f(): g()' | pylspt-dev parse -
```

Tightest possible debugging loop, and it exercises the "content is not on disk"
path that `parse_file(file_path, content)` uses in production — the one place
you legitimately parse outside the database, with an explicit target version
(exercise 04, rule 2).

### Step 4 — snapshots and `--bless`

```bash
pylspt-dev test python/corpus/            # run every .py, diff against .expected.json
pylspt-dev test python/corpus/ --bless    # rewrite the .expected.json files
```

`--bless` is the ergonomic win: change something, re-bless, read the **diff** in
`git diff`. Reviewing a JSON diff is how you notice you broke something
unrelated.

> ⚠ Generate the first `.expected.json` files **from the Python driver**, not
> from your Rust output. Otherwise you are snapshotting your own bugs. That is
> M0 in `plan/04-build/02-milestones.md`, and it is listed first for this
> reason.
>
> If the Python driver is not runnable right now, hand-check the first snapshot
> for at least two files and mark the rest provisional. A snapshot you have not
> read is a record of what your code did, not of what it should do.

Wire the same comparison into `cargo test` with `insta` (which ruff itself
uses), so CI runs it without the CLI.

### Step 5 — determinism

```bash
pylspt-dev parse corpus/calls.py --repeat 10
# → OK: 10 identical runs
```

Catches hash-map iteration order — the main way this pipeline goes
non-deterministic, along with parallel merge order once you add rayon. Cheap to
run, and a flaky parity suite is much harder to diagnose than a failing one.

Use `FxHashMap` freely for lookups, but **sort before serialising** anything
whose order is observable.

### Step 6 — `--stats`

```
parse           1.2ms
semantic index  8.4ms   (cold)
nodes          14.1ms
─────────────────────
total          23.7ms

nodes emitted        1,284
ids missing             12       ← would trigger injection
unresolved imports       3
libcst invocations       0       ← the fast path held
```

These counters are your dashboard. `ids missing` and `unresolved imports` are
the two that explain most "the output looks wrong" reports before you open a
debugger.

### Step 7 — run it on real code

Point it at an actual project — ideally one v-noc analyses.

Expect it to fall over on something. That is the deliverable of this step: a
list of real-world inputs your fixtures did not cover. Every one of them becomes
a fixture.

Then compare timings against your Python driver on the same files. You now have
the beginnings of the M8 baseline, and — for the first time — a number instead
of a hope.

---

## The final exam

`exam.md` covers all eleven exercises. Do it in one sitting, closed-book. It
takes about an hour and it will find the two or three things you learned and
then quietly forgot.

---

## What comes next

You now have M1–M4 of `plan/04-build/02-milestones.md`: wiring, version report,
syntax layer, ID injection, MRO. **Three of the six RPC methods work, and they
are already dramatically faster than the Python driver.**

What is left is the project:

```
M5  ████                        call tree, context-free — the scaffolding
M6  ██████████████████████      ★ context sensitivity — the actual work
M7  ██                          transport
M8  ████                        performance
```

Read `plan/03-call-tree/` now, in order, all ten chapters. It will read
differently than it would have two weeks ago:

- **`01-what-jedi-actually-does`** — the mechanism you are replacing
- **`02-why-ty-alone-cannot`** — you proved this yourself in exercise 09
- **`03-the-abstract-interpreter`** — the architecture
- **`04-value-domain`** — you felt the need for this in exercise 07 (two `Cache()`
  instances, one type)
- **`05-binding-arguments`** — you named this requirement in exercise 07, step 4
- **`06-attributes-and-self`** — you hit this wall in exercise 08, step 5
- **`07`–`10`** — callbacks, termination, path identity, value flow

Before you start M5, do one thing from `plan/04-build/02`: **M0, the golden
files.** Run the Python driver over a real corpus and record every request and
response, with timings. Everything after this point is verified against that
data, and several open design questions — including how `base_classes` order is
used and whether two calls to the same function merge — are *answered* by
looking at it rather than argued about.

---

## Done when

- [ ] `version`, `parse`, `ids`, `mro`, `outgoing`, `all` all work
- [ ] `test` + `--bless` run over the corpus
- [ ] `--repeat 10` passes on every fixture
- [ ] `--write` defaults off and dry-run reports `modified` correctly
- [ ] the CLI contains no analysis code
- [ ] you ran it on a real project and wrote down what broke
- [ ] you have a timing comparison against the Python driver
- [ ] you did the final exam

---

→ [`exam.md`](exam.md) — the final exam
→ then `plan/03-call-tree/01-what-jedi-actually-does.md`
