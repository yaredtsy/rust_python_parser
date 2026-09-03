# 02.02 — `id_injector.py` → libcst (the Rust one)

Porting UUID injection. **Use the `libcst` crate.** It is a near-direct port of
the Python you already have.

---

## Why libcst, and not hand-rolled text edits

libcst has a Rust implementation published on crates.io as **`libcst`**
(the Rust module is named `libcst_native`). It is the same library your Python
uses, so the port is mechanical.

**Ruff already depends on it** — **[verified]**:

```toml
# ruff/Cargo.toml:132
libcst = { version = "1.8.4", default-features = false }
```

used by `ruff_linter` and `ruff_dev`:

```rust
// crates/ruff_linter/src/fix/codemods.rs
use libcst_native::{Codegen, CodegenState, Expression, ImportNames, ...};
```

So this is not an exotic choice. It is what ruff itself does when it needs a
lossless rewrite, for exactly the reason you need one: **ruff's own AST cannot
round-trip source text.**

| | libcst | hand-rolled text edits |
|---|---|---|
| Preserves formatting | **guaranteed, by construction** | you must not break it |
| `r"""` / `b"""` prefixes | handled | you must handle it |
| One-line bodies (`def f(): return 1`) | handled | you must detect it |
| Indentation (tabs, 2-space) | handled | you must compute it |
| Port effort | translate the Python | write and debug new logic |
| Extra dependency | already in ruff's tree | none |
| Speed | slower parse | faster |

The speed column is the only one favouring text edits, and the next section
removes even that.

---

## The design that keeps libcst off the hot path

The one real objection is parsing each file twice. Fix it by splitting
**detection** from **modification**:

```
parse_file(path, content)
   │
   ├─ ruff AST (already parsed, salsa-cached)
   │     └─ does every def/class have an `ID:` in its docstring?
   │            │
   │       yes ─┴──► done. no libcst, no write, no second parse.   ← ~99% of calls
   │
   └─ no ──► libcst: parse, inject, codegen, write file, sync db   ← cold path
```

**Detection is free** — you have already walked the ruff AST to build the node
tree, and the docstring check is the same `ID:` lookup you do for
`BaseNode.id`. If every ID is present, you never touch libcst.

Injection only happens on files that are new or newly edited. After the first
pass over a project, this is close to never.

> This also fixes the "three parsers per file" problem from
> [`00-orientation/02`](../00-orientation/02-why-it-is-slow.md#5-three-parsers-per-file).
> Today `parse_file` always runs libcst, always runs parso, and sometimes runs
> parso again inside Jedi. The Rust version runs ruff's parser once (cached),
> and libcst only when it is actually going to change the file.

---

## The port

### Dependency

```toml
# Option A (workspace member) — inherit ruff's pin
libcst = { workspace = true }

# Option B (standalone)
libcst = { version = "1.8.4", default-features = false }
```

**Keep `default-features = false`.** The default features build the PyO3 Python
extension module, which you do not want. **[check]** the current feature list at
your version — ruff sets it false, so follow that.

Latest published is **1.8.6** (Nov 2025, actively maintained). Ruff pins
**1.8.4**. Under Option A use ruff's pin so there is one copy in the tree.

### Parsing

```rust
use libcst_native::{parse_module, Codegen, CodegenState};

let module = libcst_native::parse_module(&source, None)
    .map_err(|e| /* log and give up — never crash */)?;
```

The second argument is the encoding. `None` means UTF-8. This mirrors
`cst.parse_module(content)` in your Python.

### Walking and modifying

Your Python uses a `CSTTransformer` with `leave_ClassDef` / `leave_FunctionDef`.
The Rust crate does not have that visitor API — you walk and mutate the tree
directly, the way `codemods.rs` does:

```rust
use libcst_native::{Statement, CompoundStatement, FunctionDef, ClassDef};

fn inject_into_module(module: &mut libcst_native::Module) -> bool {
    let mut modified = false;
    for stmt in module.body.iter_mut() {
        modified |= inject_into_statement(stmt);
    }
    modified
}

fn inject_into_statement(stmt: &mut Statement) -> bool {
    match stmt {
        Statement::Compound(CompoundStatement::FunctionDef(f)) => {
            let changed = add_id_to_body(&mut f.body);
            // recurse: nested defs and classes also need IDs
            changed | walk_suite(&mut f.body)
        }
        Statement::Compound(CompoundStatement::ClassDef(c)) => {
            let changed = add_id_to_body(&mut c.body);
            changed | walk_suite(&mut c.body)
        }
        Statement::Compound(CompoundStatement::If(i))
        | Statement::Compound(CompoundStatement::With(_))
        | Statement::Compound(CompoundStatement::Try(_)) => {
            // defs can hide inside these — recurse
            walk_nested(stmt)
        }
        _ => false,
    }
}
```

**[check]** the exact enum and field names at your version — `Statement`,
`CompoundStatement`, `FunctionDef.body`, `Suite`. Use `ruff_dev`'s
`print_cst.rs` to dump a real tree and read the shapes:

```bash
cd /Users/yared/Documents/Programing/ruff
cargo run -p ruff_dev -- print-cst some_file.py
```

That command exists **[verified]** (`crates/ruff_dev/src/print_cst.rs`) and is
the fastest way to learn the CST shape. Use it before writing the walk.

### Generating the text back

```rust
let mut state = CodegenState {
    default_newline: "\n",     // or "\r\n" — see below
    default_indent: "    ",    // or "\t" — see below
    ..Default::default()
};
module.codegen(&mut state);
let new_source = state.to_string();
```

> ⚠ **`default_newline` and `default_indent` only apply to nodes you create.**
> Existing nodes keep their own whitespace exactly. Since you are inserting a
> brand-new docstring statement, these two settings decide how it looks.
>
> Get them from the file itself, not from a constant. Ruff has a helper for this
> — `ruff_python_codegen::Stylist` — and `codemods.rs` shows the glue
> **[verified]**:
>
> ```rust
> let mut state = CodegenState {
>     default_newline: stylist.line_ending().as_str(),
>     default_indent: stylist.indentation(),
>     ..Default::default()
> };
> ```
>
> Copy that. Otherwise a CRLF file or a tab-indented file gets a mismatched
> line inserted, and a formatter check will fail on the user's next commit.

---

## What you still port by hand

libcst handles the *syntax*. These are your logic and translate directly:

### `_extract_metadata`
```python
pairs = re.findall(r"(\S+)\s*:\s*(\S+)", docstring)
```
Hand-roll it. Scan for `:`, take the non-space run before and after. Twenty
lines, no regex dependency, and it runs on every def in the project.

### `_build_docstring`
```python
pattern = rf"(^|(?<=\s)){re.escape(key)}\s*:\s*\S+(?=\s|$)"
```
⚠ Rust's `regex` crate **does not support lookbehind**. Rewrite as a manual
scan: find `key`, check the char before is whitespace or start-of-string, then
consume `\s*:\s*\S+` and check the char after. Same twenty-line style.

### The two pre-existing bugs

Reproduce them, do not fix them — you asked for no logic changes. Mark each
with a `// PARITY:` comment so the choice is visible:

1. **Prefix loss.** `id_injector.py:70` emits `f'"""{content}"""'`
   unconditionally, dropping an `r` prefix. `r"""...\n..."""` becomes
   `"""...\n..."""`, changing the string's meaning.
2. **`"""` inside content.** If the original docstring contains `"""`, the
   rebuilt literal is broken.

Both are two-line fixes whenever you decide you want them.

---

## `file_folder_ids.py`

Same machinery at module level:

- Module docstring = first statement of `module.body`.
- `read_or_inject_folder_id` **creates `__init__.py` if missing** — preserve, it
  is load-bearing for v-noc's folder identity.
- Keys are `FileID` / `FolderID`; results are prefixed `FileSchema/` /
  `FolderSchema/`.
- On any error the Python returns a **fresh random UUID with
  `modified=False`** (`file_folder_ids.py:22`) — a throwaway ID that is never
  persisted. Odd, but reproduce it.

---

## ⚠ The salsa write ordering

This bug does not exist in Python and **will** catch you:

```
1. compute the new content        (libcst)
2. write it to disk
3. File::sync_path(&mut db, &path)     ← DO NOT SKIP
```

Skip step 3 and every later query serves the pre-injection source. Your node IDs
stay `None` forever and nothing looks broken.

Because `&mut db` cancels in-flight queries on other threads
([`01-crates/02`](../01-crates/02-the-salsa-db.md)), batch it: collect all
injections for a request, write them all, then sync once.

---

## Testing

- **Idempotent.** Run twice; the second run makes zero edits and returns
  `modified: false`. Without this, files grow an ID block per request.
- **Byte-identical to the Python driver** on a corpus containing: no-docstring
  defs, existing-docstring defs, `r"""` prefixes, one-line bodies
  (`def f(): return 1`), tab-indented files, CRLF files, nested defs, and
  non-ASCII content.
- **Detection is correct.** A file where every ID is present must take the fast
  path — assert libcst is never invoked. Add a counter and check it.

That last test is the one that protects your performance. Without it, a
regression that makes libcst run on every file would show up only as "it got
slow", with no obvious cause.

---

> **Optional, out of scope, worth writing down:** content-addressed IDs
> (`blake3(module_path + qualified_name)`) would remove file writes entirely,
> remove the `&mut db` sync, and make IDs stable across clones and rebases.
> You asked for no logic changes, so this is a note, not a proposal.

---

→ Next: [`03-jedi-mro-to-ty-mro.md`](03-jedi-mro-to-ty-mro.md)
