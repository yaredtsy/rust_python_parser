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
| `calls <file>` | `resolve_calls` | `{call_frame_stack, graph, stats}` |
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
    --limits k=v,...    override any 03-call-tree/13 Limits field (e.g. fp_depth=3, max_union_fanout=off)
    --no-reuse          recompute every summary instead of reusing it; output must not change
    --graph             for `calls`: print the context graph instead of the rendered tree
    --merged            for `calls`: apply v-noc's merge (03-call-tree/14) before printing,
                        to compare with the Jedi driver's output by eye
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

JSON tells you *what* the graph is. Trace tells you *why*. For a
context-sensitive analysis this is the difference between a ten-minute fix and
an afternoon.

```
walk  app.emit  ctx#7 {writer: Instance{JsonWriter}, data: Unknown}     main.py:5
  materialize: writer ← Instance{JsonWriter, Entry(P0)}
  call  writer.write(data)                         main.py:6:4
    callee: Attribute(Name("writer"), "write")
      recv: env HIT  writer → Instance{JsonWriter, Entry(P0)}
      .write → vnoc::member → BoundMethod{JsonWriter.write, recv=Entry(P0)}
    key   JsonWriter.write  ctx#8 {self: Instance{JsonWriter}, d: Unknown}
    get   ctx#8 → walk (new)
    │ walk  app.JsonWriter.write  ctx#8
    │   call  json.dumps(...)                      main.py:2:14
    │     → External(json.dumps)
    │ end   ctx#8  Finished  returns [Plain(Ty(None))]  effects 0
    resolve: returns → [Ty(None)]
    apply:   no effects
end   ctx#7  Finished  returns [Plain(Ty(None))]  effects 0

walk  app.read_csv  ctx#9
  call  emit(CsvWriter(), 2)                       main.py:12:4
    key   emit  ctx#10 {writer: Instance{CsvWriter}, data: Data(int)}
  ...
  call  emit(JsonWriter(), 1)                      main.py:13:4
    key   emit  ctx#7  → REUSE (Ready)
```

**Log every decision, especially the negative ones.** Each skip, reuse and
limit gets a distinct marker:

```
✗ builtin name             (call_resolver.py:114)
✗ not project code         (shown: no; walked: no)
✗ no ID: in docstring      (shown: no; walked: yes)   ← the silent one
↺ reuse                    (Ready summary)
⟳ pending                  (recursion: recorded, not entered)
✂ cut                      (max_stack / contexts / contexts_per_func / deadline)
✗ fan-out cap              (union had 12 members, max 4)
⚠ annotation fallback      (h ← Handler, from the annotation)
⚠ deep write fallback      (past max_path: weak Unknown at P0.a.b.c.d.e.f.g)
```

The `no ID:` case is the one that will waste your time — a callee vanishes from
the tree with no other symptom. Make it loud in trace mode.

Also mark **env hit vs miss** on every callee and argument, and print each key.
A key that differs where you expected reuse, or matches where you expected two
contexts, is the first thing to look at.

For a recursion group, trace each fixpoint round: the members, the summaries
that changed, and the contexts that joined.

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
├── walk/
├── attributes/
├── keys/
├── calls/
├── closures-callbacks/
├── recursion/
└── core/
```

One directory per chapter's fixture list in `plan/03-call-tree/`:
[`01`](../03-call-tree/01-what-jedi-actually-does.md#the-invariants-to-test-against),
[`05`](../03-call-tree/05-the-walk.md#fixtures),
[`06`](../03-call-tree/06-binding-arguments.md#fixtures),
[`07`](../03-call-tree/07-attributes-and-self.md#fixtures),
[`08`](../03-call-tree/08-keys.md#fixtures),
[`10`](../03-call-tree/10-one-call.md#fixtures),
[`11`](../03-call-tree/11-closures-and-callbacks.md#fixtures),
[`12`](../03-call-tree/12-recursion.md#fixtures),
[`15`](../03-call-tree/15-checks-and-build-order.md#fixtures-for-the-core).
Each is a few lines of Python with its expected edges, and runs in
milliseconds. Every fixture also runs in `--no-reuse` mode.

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
([`03-call-tree/14`](../03-call-tree/14-output.md#determinism)). Run it with
the deadline off (`--limits deadline=off`). Cheap to run, and a flaky parity
suite is much harder to diagnose than a failing one.

---

## `--stats`

```
parse          1.2ms
semantic index 8.4ms   (cold)
call tree     14.1ms
─────────────────────
total         23.7ms

contexts / walks     312 / 312
reuse hits           1,904    ← the summaries paying off
groups / rounds      3 / 11   (largest group: 4)
env hits / misses    412 / 871
unknown values       94       ← precision signal
annotation fallbacks 17   types dropped 3   deep-write fallbacks 0
skipped: builtin 203  non-project 88  no-id 12
cuts: 0   cycle gaps: 0   render cuts: 0
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
| `calls` + `--stats` + `--graph` | with M5 (S2) | call tree |
| `--trace` | **with M5 (S2), before S3** | ★ do not defer |
| `--no-reuse` | with M5 (S3) | the solver |
| `--limits` | with M5 (S3) | `Limits` |
| `--watch` | with M8 | — |

`--trace` landing early is the important one. The call tree is where you will
spend most of the project, and it is where "the output is wrong and I don't
know why" is the default state.

---

## Deliverable

`pylspt-dev` covering all six commands, the fixture suites with goldens sourced
from the Python driver, `--bless` workflow, `--trace` output, `--no-reuse`, and
determinism checking — all sharing code with the server binary.

---

→ Next: [`01-wiring-cargo.md`](01-wiring-cargo.md)
