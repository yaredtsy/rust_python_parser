# 04.00 — The dev CLI: file in, JSON out

**Build this before anything else in `04-build/`.** It is the inner loop for
every milestone. Without it you are debugging a context-sensitive interpreter
through an HTTP server, which is a bad way to spend a month.

```bash
pylspt-dev parse  fixtures/emit.py            # → stdout JSON
pylspt-dev calls  fixtures/emit.py -o out.json
pylspt-dev calls  fixtures/emit.py --trace    # → human-readable walk
```

---

## The one rule

> **The CLI must call the exact same functions the RPC layer calls.**

Not a reimplementation, not a "simplified path". If the CLI and the server can
disagree, your fixtures stop proving anything about what ships.

```
src/
├── lib.rs              ← all analysis lives here
├── bin/
│   ├── pylspt.rs       ← the JSON-RPC server (04-build/03)
│   └── pylspt-dev.rs   ← this CLI
```

```toml
# crates/pylspt/Cargo.toml
[lib]
name = "pylspt"

[[bin]]
name = "pylspt"
path = "src/bin/pylspt.rs"

[[bin]]
name = "pylspt-dev"
path = "src/bin/pylspt-dev.rs"
```

Both binaries construct the same `Service` and call the same methods. The CLI
is a different *transport*, nothing more.

---

## Commands

Each maps 1:1 onto an RPC method, so `pylspt-dev <cmd>` output is directly
comparable to the golden responses from
[`02-milestones.md#m0`](02-milestones.md#m0--golden-files-from-the-python-driver--do-this-first).

| Command | RPC equivalent | Output |
|---|---|---|
| `parse <file>` | `parse_file` | `{nodes, content, modified}` |
| `calls <file>` | `resolve_calls` | `{call_frame_stack, truncated, stats}` |
| `mro <file>` | `parse_file(resolve_mro=true)` | nodes with `base_classes` |
| `ids <file>` | `read_or_inject_file_id` | `{file_id, modified}` |
| `all <file>` | — | every method, one JSON object |
| `version` | — | resolved Python version + source |

### Flags

```
-o, --out <path>        write JSON here (default: stdout)
    --project <dir>     project root (default: walk up for pyproject.toml/.git)
    --pretty            indented JSON (default: compact)
    --write             ★ actually write ID injections to disk (default: DRY RUN)
    --entry <name>      for `calls`: start from this function (default: module level)
    --trace             human-readable interpreter walk to stderr
    --stats             timing + counters to stderr
    --limits k=v,...    override depth/nodes/deadline from 03-call-tree/08
    --repeat <n>        run n times, assert identical output (determinism check)
```

> ⚠ **`--write` defaults off.** `parse_file` injects UUIDs into source files as
> a side effect (`scanner.py:24` **[verified]**). Running the CLI over a corpus
> would silently rewrite the user's code. Dry-run by default; report what
> *would* change via `modified: true`.

---

## `version` — the first thing to build

Ten lines, and it settles the question from
[`01-crates/03`](../01-crates/03-python-version.md) before you write analysis:

```
$ pylspt-dev version --project ~/some/project
python_version = 3.9   (source: pyproject.toml requires-python ">=3.9")
search_paths   = [src, .venv/lib/python3.12/site-packages, <vendored typeshed>]
ruff           = ac201b8

⚠ running interpreter is 3.12 — jedi would have used 3.12
```

That warning line is worth writing. It turns the most likely parity surprise
into something you see on day one.

---

## `--trace` — the reason this CLI exists

JSON tells you *what* the tree is. Trace tells you *why*. For a
context-sensitive interpreter this is the difference between a ten-minute fix
and an afternoon.

```
resolve_call  emit(...)                          main.py:12:4   depth=1 budget=99998
  callee: Name("emit")
    env miss → ty → FunctionLiteral(emit)
  args:
    [0] JsonWriter()  → Instance{JsonWriter, #3}         ← construct
    [1] {"a": 1}      → Ty(dict[str, int])               ← ty fallback
  bind: writer ← Instance{JsonWriter,#3}
        data   ← Ty(dict[str, int])
  ✓ FunctionSchema/8f2a…  qname=app.emit
  │
  ├─ resolve_call  writer.write(...)             main.py:6:4    depth=2
  │    callee: Attribute(Name("writer"), "write")
  │      recv: env HIT  writer → Instance{JsonWriter,#3}
  │      .write → mro[JsonWriter] → BoundMethod{JsonWriter.write, recv=#3}
  │    bind: self ← Instance{JsonWriter,#3}
  │          d    ← Ty(dict[str, int])
  │    ✓ FunctionSchema/1b7c…  qname=app.JsonWriter.write
  │    │
  │    └─ resolve_call  json.dumps(...)          main.py:2:14   depth=3
  │         ✗ skipped: not project code (site-packages)
  │
  └─ (end emit)
```

**Log every decision, especially the negative ones.** The five skip reasons from
[`03-call-tree/01`](../03-call-tree/01-what-jedi-actually-does.md#the-loop-in-pseudocode)
each get a distinct marker:

```
✗ builtin name             (call_resolver.py:114)
✗ not project code         (call_resolver.py:141)
✗ no ID: in docstring      (call_resolver.py:154)   ← the silent one
✗ already seen this frame  (call_resolver.py:149)
✗ ancestor cycle guard     (call_resolver.py:157)
✗ budget exhausted / depth cap
✗ fan-out cap (union had 12 members, max 4)
```

The `no ID:` case is the one that will waste your time — a callee vanishes from
the tree with no other symptom. Make it loud in trace mode.

Also mark **env hit vs miss** on every callee and every argument. That single
annotation tells you whether context-sensitivity is working, which is the thing
you are actually building.

Implement it as a `Tracer` trait with a no-op default so release builds pay
nothing:

```rust
trait Tracer {
    fn enter_call(&mut self, ...) {}
    fn resolved(&mut self, ...) {}
    fn skipped(&mut self, reason: SkipReason, ...) {}
    fn bound(&mut self, param: &str, value: &AbstractValue) {}
}
struct NoopTracer;   // all defaults
struct TreeTracer { indent: usize, out: StderrLock }
```

---

## Fixture / snapshot mode

```bash
pylspt-dev test fixtures/           # run every .py, diff against .expected.json
pylspt-dev test fixtures/ --bless   # rewrite the .expected.json files
```

```
fixtures/
├── binding/
│   ├── 01-positional.py
│   ├── 01-positional.expected.json
│   ├── 02-keyword.py
│   └── ...
├── attributes/
├── callbacks/
└── termination/
```

One directory per chapter's fixture list —
[`01`](../03-call-tree/01-what-jedi-actually-does.md#the-invariants-to-test-against) (12),
[`05`](../03-call-tree/05-binding-arguments.md#test-fixtures-for-this-chapter) (9),
[`06`](../03-call-tree/06-attributes-and-self.md#fixtures) (8),
[`07`](../03-call-tree/07-callbacks-and-higher-order.md#fixtures) (8).
**37 fixtures**, each a few lines of Python, all running in milliseconds.

`--bless` is the ergonomic win: change the interpreter, run `test --bless`,
read the *diff* in `git diff`. Reviewing a JSON diff is how you notice you broke
something unrelated.

> Generate the initial `.expected.json` files **from the Python driver at M0**,
> not from your Rust output. Otherwise you're snapshotting your own bugs.
> `--bless` is for evolving them afterwards, deliberately.

Wire the same thing into `cargo test` via `insta` (which ruff itself uses) so CI
runs it without the CLI.

---

## Single-file mode

Most fixtures have no project, no `pyproject.toml`, no venv. Handle it:

```rust
// No project root found → synthesise minimal metadata rooted at the file's
// parent, python_version = latest_ty(), search path = that directory.
```

Otherwise every fixture needs scaffolding, and you won't write 37 of them.

Add `--python-version 3.12` to override per-fixture, so you can pin a PEP 695
fixture without a config file.

---

## Stdin mode

```bash
echo 'def f(): g()' | pylspt-dev parse -
```

For the tightest possible loop while debugging the parser. Also makes the CLI
scriptable against generated inputs.

---

## `--repeat` — determinism

```bash
pylspt-dev calls big_file.py --repeat 10
# → OK: 10 identical runs
# → FAIL: run 4 differs (children reordered under app.emit)
```

Catches hash-map iteration order and rayon merge order, which are the two ways
this pipeline goes non-deterministic
([`03-call-tree/09`](../03-call-tree/09-path-identity.md#determinism-)). Cheap
to run, and a flaky parity suite is much harder to diagnose than a failing one.

---

## `--stats`

```
parse          1.2ms
semantic index 8.4ms   (cold)
call tree     14.1ms
─────────────────────
total         23.7ms

nodes visited        1,284
env hits / misses    412 / 871
ty inference calls   871
unknown values       94       ← precision signal
skipped: builtin 203  non-project 88  no-id 12  cycle 4  budget 0
truncated: false
```

`unknown values` and `no-id` are your precision dashboard. When someone says
"the tree is missing things", these two numbers usually answer it without a
debugger.

---

## Watch mode (optional, later)

```bash
pylspt-dev calls app.py --watch
```

Re-runs on file change against the **same `ProjectDatabase`**, so you see
salsa's incrementality directly — first run 400ms, subsequent 8ms. Genuinely
useful for demoing the win, and it exercises the `File::sync_path` + `&mut db`
cancellation path from [`01-crates/02`](../01-crates/02-the-salsa-db.md) that
the RPC server also depends on. Cheap way to find those bugs early.

---

## Build order

| Step | When | Depends on |
|---|---|---|
| `version` | with M1 | wiring only |
| `parse` + `-o` + `--pretty` | with M2 | syntax layer |
| `test` + `--bless` | with M2 | above |
| `--repeat` | with M2 | above |
| `ids` + `--write` guard | with M3 | injection |
| `mro` | with M4 | MRO |
| `calls` + `--stats` | with M5 | call tree |
| `--trace` | **with M5, before M6** | ★ do not defer |
| `--limits` | with M6 | budget |
| `--watch` | with M8 | — |

`--trace` landing before M6 is the important one. M6 is where you will spend
most of the project, and it is the milestone where "the output is wrong and I
don't know why" is the default state.

---

## Deliverable

`pylspt-dev` covering all six commands, 37 fixtures with goldens sourced from
the Python driver, `--bless` workflow, `--trace` output, and determinism
checking — all sharing code with the server binary.

---

→ Next: [`01-wiring-cargo.md`](01-wiring-cargo.md)
