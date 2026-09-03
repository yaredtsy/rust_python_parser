# 03 — The database

**Goal:** you can measure a salsa cache hit, prove that editing one file
invalidates exactly the right queries, and explain why your Python driver's
`lru_cache` cannot do the same thing.

This is the exercise where the "10–100× faster" claim stops being a slogan.

---

## Read first

- `tutorial/06-salsa-the-database.md` — all of it
- `plan/01-crates/02-the-salsa-db.md` — the `Db` trait stack and the three file
  handles
- `plan/00-orientation/02-why-it-is-slow.md` §1 — the problem being solved

---

## The mental model

### Inputs, queries, revisions

Salsa has three concepts and you need all three.

**Inputs** are things the world tells you: file contents, settings, the project
root. Salsa does not compute them; you set them.

**Queries** are functions marked `#[salsa::tracked]`. Salsa memoises each
result *and records which inputs it read while computing it*.

**Revisions** are a global counter. Change an input, the revision bumps. On the
next query salsa walks the dependency graph and re-runs only what transitively
depended on the thing that changed.

```
              revision 1                    revision 2  (helpers.py edited)
  ┌───────────────────────────┐      ┌────────────────────────────────┐
  source_text(main.py)               source_text(main.py)     cached
  source_text(helpers.py)            source_text(helpers.py)  CHANGED
        │                                   │
  parsed_module(main.py)             parsed_module(main.py)   cached
  parsed_module(helpers.py)          parsed_module(helpers.py) RECOMPUTED
        │                                   │
  semantic_index(main.py)            semantic_index(main.py)  cached
        │                                   │
  infer_scope(main.run)              infer_scope(main.run)    RECOMPUTED
                                             ↑ main.py imports helpers,
                                               so its types depend on it
```

Note what that last line means: invalidation follows **semantic** dependencies,
not file boundaries. `main.py` was not edited, but the *types* in `run` depend
on `shout`, so the type query re-runs while the parse query does not. Nobody
wrote that rule — it falls out of recording which queries read which inputs.

### Why your `lru_cache` cannot do this

`scanner.py` has `@lru_cache(maxsize=50)` keyed on the **full file content
string**. Three problems, and they are structural, not tunable:

1. **One keystroke is a total miss.** A new string is a new key.
2. **It caches one step.** Only the parse is cached; inference is redone.
3. **It cannot cache across files.** The key is one file's text, so nothing
   knows that `main.py`'s analysis is still valid when `models.py` changed but
   `helpers.py` did not.

Salsa fixes all three by keying on **identity plus revision** instead of on
content, and by recording the dependency graph so it knows what to keep.

### The three file handles

You *will* mix these up. It is worth learning the distinction once, properly.

| type | what it identifies | get it from |
|---|---|---|
| `File` | a path in salsa's file system | `system_path_to_file(db, path)` |
| `PythonFile<'db>` | a `File` **plus a Python version** | `program_file.python_file(db)` |
| `ProgramFile<'db>` | a `File` **plus a whole program environment** | `db.program_file(file)` |

The reason for three is stated in the source **[verified,
`ty_python_core/src/program_file.rs`]**:

> "This allows programs with the same Python version to share parsed syntax, and
> programs with equivalent resolver environments to share module resolution,
> while keeping type inference isolated."

In other words: the parse of `helpers.py` can be shared between two projects
that both target 3.11, even if their search paths differ — but their *type
inference* must not be shared, because the same import resolves to different
files. The three handles are three different cache keys, deliberately chosen so
each layer shares as much as is safe.

Once you see that, the API stops looking redundant. `parsed_module` takes
`PythonFile` because parsing depends only on version; `SemanticModel::new` takes
`ProgramFile` because inference depends on everything.

---

## The API, verified at `ac201b8`

```rust
// construct
let system   = OsSystem::new(&root);                          // needs ruff_db "os" feature
let metadata = ProjectMetadata::discover(&root, &system)?;
let db       = ProjectDatabase::use_defaults(metadata, system);

// open a file
use ruff_db::files::system_path_to_file;
let file: File = system_path_to_file(&db, "/abs/path/main.py")?;

// the three handles
use ty_python_semantic::Db as _;                     // trait must be in scope
let pf  = db.program_file(file);                     // ProgramFile
let pyf = pf.python_file(&db);                       // PythonFile
let f   = pf.file(&db);                              // back to File

// the tracked queries you will use constantly
ruff_db::source::source_text(db, file)   -> SourceText     // #[salsa::tracked]
ruff_db::source::line_index(db, file)    -> LineIndex      // #[salsa::tracked]
ruff_db::parsed::parsed_module(db, pyf)  -> ParsedModule   // #[salsa::tracked]
                              .load(db)  -> ParsedModuleRef // materialise the AST

// after an edit
File::sync_path(&mut db, &path);                     // one file
File::sync_all(&mut db);                             // everything
db.apply_changes(&changes);                          // batched watcher events

// parallelism
let snapshot = db.clone();                           // ProjectDatabase: Clone  [verified]
```

> `.load(db)` exists because ty can **drop a parsed AST under memory pressure
> and re-parse it later**. That is why you get a ref rather than the tree
> itself, and why you must not hold the `ParsedModuleRef` across unrelated work.

---

## The fixture

```
python/proj/
├── pyproject.toml ............. requires-python = ">=3.11"
└── src/app/
    ├── __init__.py
    ├── helpers.py ............. leaf. imported by main.
    ├── models.py .............. imported by main. does NOT import helpers.
    └── main.py ................ imports both.
```

The shape is deliberate: `models.py` and `helpers.py` are siblings that do not
know about each other, so editing one must not invalidate the other. That is the
property you are going to test.

---

## Build it

### Step 1 — open a file three ways

Write a command that, given the project root and a file path, prints all three
handles' debug representations plus the resolved Python version.

Then answer, from the output: which of the three would be equal for the same
file opened in two different projects that both target 3.11?

### Step 2 — measure a cache hit

Time `parsed_module(db, pyf).load(db)` twice in a row on `main.py`:

```
first call:   ??? µs
second call:  ??? µs
```

Predict the ratio before running. Most people guess an order of magnitude; the
real answer is usually much larger, because the second call is a hash lookup
against work that has already happened.

Then do the same for the whole chain: `source_text` → `parsed_module` →
(exercise 07 will add inference). Time a cold run and a warm run of your
exercise-02 node scanner over the same file.

**Write the numbers down.** They are the baseline for `plan/04-build/02`'s M8,
and "faster" without numbers is an opinion.

### Step 3 — prove invalidation is precise

This is the step that teaches the model. In one program:

1. Build the db, query `parsed_module` for **all four** files, time each.
2. Modify `helpers.py` on disk (append a blank line — actually write it).
3. `File::sync_path(&mut db, helpers_path)`.
4. Query `parsed_module` for all four again, time each.

Predict which of the four are recomputed. Then look at the timings.

You should see exactly one recomputed parse. Now do the same experiment for a
*semantic* query once you reach exercise 07, and you will see the answer change
— because `main.py` imports `helpers`, so its types depend on the edit even
though its syntax does not.

> ⚠ Step 3 requires `&mut db`. That is not a detail — see the traps below.

### Step 4 — the mutation and cancellation rule

Read `plan/01-crates/02` §"Mutation and cancellation", then answer in your own
words: why does taking `&mut db` cancel queries running on *other* threads, and
what must your RPC layer do about it?

Then look at your exercise-02 code. Anything holding a `ParsedModuleRef` or a
`&ModModule` across a possible mutation point is a compile error waiting to
happen — and the borrow checker will find it for you the moment you try step 3.
That is the design working, not fighting you.

### Step 5 — snapshots

```rust
let snapshot = db.clone();
std::thread::spawn(move || { /* read-only queries */ });
```

Clone the db, run your node scanner over all four files on four threads, and
compare the wall time to the sequential version.

This is the structural answer to the GIL problem from
`plan/00-orientation/02` — your Python driver's `run_in_threadpool` gives you
concurrency for I/O and nothing for CPU. Here the work actually runs in
parallel, sharing one cache.

Note what you may *not* do: hold a snapshot across a mutation. Snapshots are
read-only views of one revision.

### Step 6 — decide what of yours should be tracked

Read `plan/01-crates/02` §"Should *your* analysis be `#[salsa::tracked]`?" and
the quoted comment from ty's own `call_hierarchy.rs`:

> "The three entry points are deliberately not `#[salsa::tracked]` […] AST
> access goes through the salsa-cached `parsed_module`, which preserves
> incrementality without forcing the entry points themselves to be tracked."

Write down which of your functions should be tracked and which should not, with
a reason for each. You do not have to implement it — you have not added `salsa`
to your manifest and you do not need to. The **decision** is the deliverable,
and the principle ("untracked entry points, tracked primitives") is the thing to
carry forward.

---

## Traps

- **Holding a `Type<'db>` or `ParsedModuleRef` in a long-lived struct.** The
  `'db` lifetime is viral and it is telling you the truth: those values are only
  valid for one borrow of one revision. Lower to owned, serde-able types at the
  boundary. Cache `(file, range)` keys, never ty values.
- **Forgetting `sync_path` after writing a file.** Every later query serves the
  old content. Nothing errors; your IDs just never appear (this is exercise
  10's headline bug).
- **`&mut db` while a snapshot is alive.** The borrow checker stops you, and the
  fix is structural — finish the read phase, then mutate.
- **Assuming "cached" means "free forever".** Salsa also does *backdating*: if a
  query re-runs and produces an equal result, dependents are not re-run. So a
  whitespace-only edit can invalidate a parse but not the inference above it.
  Worth knowing when your timings look better than they should.
- **Benchmarking in a debug build.** With `opt-level = 1` on your code and `3`
  on dependencies (exercise 00), numbers are meaningful; at `-O0` they are not.

---

## Done when

- [ ] you can print all three file handles and say what each is keyed on
- [ ] you have cold/warm numbers for `parsed_module` on `main.py`
- [ ] you demonstrated that editing `helpers.py` reparses one file, not four
- [ ] you can explain why `&mut db` cancels other threads' queries
- [ ] you ran your scanner in parallel over db snapshots
- [ ] you have written down which of your own functions should be tracked

---

→ [`exam.md`](exam.md), then [`../04-python-version/README.md`](../04-python-version/README.md)
