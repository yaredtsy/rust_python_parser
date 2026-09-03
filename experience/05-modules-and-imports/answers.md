# Answers 05 — Modules and imports

---

**1.** Jedi does the search-path walk 50 times — once per `Script`, and
`jedi_manager.py` builds a fresh `Project` per request, so even the project
setup repeats. ty does it once per distinct `(module name, resolver
environment)`, cached across all 50 requests and all 500 files. The 50th request
resolves imports for free.

**2.** `ResolverFile` is a file plus its **resolver environment** (search paths,
platform) — `program_file.resolver_file(db)`.

Resolution keys on it because the answer genuinely depends on the environment:
`import config` resolves to different files in two projects with different
search paths. Keying on `File` alone would either give wrong answers across
projects or force a cache flush per project. Keying on `ResolverFile` lets two
projects with equivalent environments **share** the resolution cache safely —
the same reasoning as the three file handles in exercise 03.

**3.**

- **Namespace packages** — a directory with no `__init__.py` is a real module
  with no file.
- **Unresolvable or partially-resolved modules**, and modules that exist only as
  a search-path entry.

Do not `unwrap`. A `None` here is a normal outcome, not an error.

**4.** To a `.pyi` stub inside ty's **vendored typeshed** (`ty_vendored`), which
lives in a virtual filesystem compiled into your binary. It is not on your disk
and has no system path.

Two consequences: stdlib inference works with no interpreter installed, and any
code assuming `File → system path` meets its first counterexample here. Handle
it before it appears in a stack trace.

**5.**

| predicate | meaning | call tree |
|---|---|---|
| `is_first_party()` | found via a project/src root | **descend** |
| `is_standard_library()` | typeshed stub | skip — and there is no body to descend into anyway |
| `is_site_packages()` | installed dependency | skip (quirk 2) |

---

**6.** The effective rule is: **the callee's file path starts with the project
path — and nothing else matters.**

The `site-packages`, `lib/python` and `is_stdlib` checks sit above an
unconditional `return False` at `call_resolver.py:310` and are unreachable.

The dead code matters because it tells you the *intent* differs from the
*behaviour*. Someone meant to classify dependencies more finely. If you port the
intent, you change the output — a `.venv` inside the project directory is
excluded today by the prefix test alone, and would be excluded by the reachable
checks too, but the two rules diverge for editable installs (see 16). **Port the
behaviour. Note the intent in a comment.**

**7.**

| input | prefix test | `is_first_party()` | want | ship |
|---|---|---|---|---|
| first-party file outside the configured `root` | project code | not first-party | prefix | prefix |
| source file inside `<project>/.venv/` | **project code** | site-packages | ty's | prefix |
| editable install pointing back at your tree | depends on which path resolves | first-party | ty's | prefix |

The `.venv` row is the one that bites in practice: a virtualenv inside the
project directory means every dependency passes the prefix test, so your call
tree descends into third-party code. That is today's behaviour, so it is the
contract — but it is also a plausible explanation if anyone has ever complained
the tree was enormous.

**8.** A project that defines its own `list` (or `type`, `filter`, `id` — all
common in real code) would suddenly be **descended into**, because inference
would resolve the name to the user's function rather than to the builtin, and
the post-filter would not match a builtin.

So the ordering is observable: skip-by-name drops the user's function too. Quirk
3 says preserve that.

**9.** Today's driver skips it — it never even looks at what the name resolves
to. Whether that is a bug depends on who you ask; it certainly loses real
information.

It is not yours to fix. `MEMORY.md`: the contract is the output. Preserve, mark
with a `// PARITY:` comment, and if someone later wants it fixed it is a
two-line change in a known place.

---

**10.**

| file | module name |
|---|---|
| `src/pkg/__init__.py` | `pkg` |
| `src/pkg/core.py` | `pkg.core` |
| `src/pkg/sub/deep.py` | `pkg.sub.deep` |

`src` is **not** part of the name — it is a search path root, not a package.
That is exactly what `[tool.ty.environment] root = ["src"]` declares.

**11.** With the config removed, `src` is no longer a first-party root, so the
names change or resolution fails outright (`pkg` is no longer importable from
where ty is looking).

What it tells you: "first-party" is a **configured** fact, not a discovered one.
Two projects with identical layouts can classify the same file differently. Any
project-code filter built on module classification inherits that, which is
another argument for shipping the prefix test for parity.

**12.** Resolving from `core.py`:

| import | resolves | on disk? |
|---|---|---|
| `json` | typeshed stub | **no** — vendored |
| `os.path` | typeshed stub | no |
| `typing` | typeshed stub | no |
| `collections` | typeshed stub | no |
| `pkg.sub.deep` | your file | yes |
| `.sub.deep` | the same file | yes |
| `tomllib` | typeshed stub (3.11+; check what happens if the target version is below that) | no |
| `definitely_not_a_real_module_xyz` | `None` | — |

`resolve_module` returns `None`; nothing panics, nothing is logged unless you
log it. That is the behaviour you want (quirk 13) — but log it at debug level,
because "the call tree is missing a subtree" and "that import does not resolve"
are the same bug seen from two ends.

**13.** In `main`:

| call | verdict | rule |
|---|---|---|
| `load(path)` | descend | first-party |
| `transform(data)` | descend | first-party |
| `descend(shaped)` | descend | first-party |
| `json.dumps(shaped)` | skip | not project code |
| `len(text)` | skip | **builtin, by name, before inference** |
| `list(blob)` | skip | builtin, by name |

Add a fourth possible verdict you will meet constantly on real input: **no-ID**
(quirk 4). A first-party callee whose docstring has no `ID:` is dropped *and*
not descended into. On this fixture every def has an ID, so it never fires —
which is exactly why real code surprises you.

---

**14.** Nothing about the *tree* would break immediately, but identity would
become ambiguous: the same function reached as `pkg.sub.deep.descend` from one
call site and as `pkg.descend` from another would produce two different
`target_qname`s, so `add_child`'s dedup-by-qname (quirk 6) would fail to merge
them, and downstream joins on `target_id` would see two nodes where v-noc
expects one.

Concretely: `entry.main` calls `descend(shaped)` directly, and `core.load` calls
`descend(raw)`. If identity followed the import path, one of those becomes
`pkg.descend` — and the `call_count` merge behaviour changes.

Identity must follow the **definition**, not the route taken to it.

**15.** Jedi's `get_qualified_names(True)` follows the definition, so
`pkg.load` reports **`pkg.core.load`** — the module where the `def` lives, not
`pkg/__init__.py` where the name was re-exported.

That is the answer you want, and it is the same principle as 14. Exercise 06
makes you reproduce it in ty, where the equivalent question is "which file does
the `Definition` live in" — and where re-exports and stub files add wrinkles
(`map_stub_definition` exists for exactly this reason).

**16.** The prefix test says: it depends which path the resolver returned. If it
resolved through `site-packages`, the prefix test says **not** project code and
the subtree is dropped — even though it is literally your own source file.

`is_first_party()` says first-party, because ty classifies by search path and
the editable install points at your root.

ty's answer is right. The prefix test is what ships. This is the single best
argument for the disagreement log in step 3: an editable install is common in
real Python projects, and "why does my call tree stop at my own code" is a
question you want the log to answer for you.

**17.** On the fixture: zero disagreements, which proves only that the fixture
has no `.venv`, no editable install, and no first-party code outside `root`.

Zero on a *real* project is a stronger signal, but still not a proof — it says
your project layout does not currently exercise the difference. Keep the log on.
It costs one comparison per callee and it is the cheapest parity-bug detector
you will ever write.
