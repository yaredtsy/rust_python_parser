# 01.04 — Imports and modules

**You build:** module values, and `member`: the function that answers `x.name`
by looking at what `x` holds. Imports inside a function rebind their names.
**Fixtures:** `python/imports.py`, `python/imports_partial.py` (they import
`python/util.py` and `python/pkg/sub/mod.py`)
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Module chains",
`03-design.md` § "Where ty gets consulted"

---

## The fixture

```python
import pkg.sub.mod
import util
import util as u
from pkg.sub import mod
from util import helper
from util import helper as h2

def direct():            helper()             # util.helper
def via_module():        util.helper()        # util.helper
def via_alias():         u.helper(); h2()     # util.helper, util.helper
def deep():              pkg.sub.mod.work()   # pkg.sub.mod.work
def from_package():      mod.work()           # pkg.sub.mod.work
def module_in_a_local(): m = util; m.helper() # util.helper

def local_import():
    other = a
    from util import other
    other()                                    # util.other, not a
```

```python
# imports_partial.py
import pkg

def no_submodule():
    pkg.sub.mod.work()     # pkg/__init__.py imports nothing: no edge
```

## Predict

1. Which of these blocks pass with your step 03 code, unchanged? (Run it.)
2. In `local_import`, what does your env hold for `other` just before the call?
3. `deep` and `no_submodule` have the same last line. Why may only one of them
   have an edge?

---

## Build

### Decision 1: ty resolves imports. You write no import resolution

`from util import helper` at module level binds a module global. Inside
`direct`, `helper` is not a local, so the env has nothing and ty answers: it
resolved the import (search paths, packages, `__init__.py`, relative imports,
stubs) and gives the function literal of `util.helper`.

**Why:** module-level bindings don't depend on the path. ty already resolves
them correctly, and caches the answer. A second resolver would disagree with
ty exactly where ty is right.

Run predict question 1: most blocks pass already. Two need code: the stale
binding in `local_import`, and a module value that reaches an attribute
through your env.

### Decision 2: an import inside a function binds its names to ty's answer

```rust
Stmt::Import(_) | Stmt::ImportFrom(_) => {
    for alias in import_aliases(stmt) {
        let ty = self.sem.type_of_import(&self.frame.model, alias);   // HasType for ast::Alias
        let v = self.lift(ty);
        env.bind(bound_name(alias), v);
    }
}
```

`bound_name` is the `as` name if there is one; for `import a.b.c` without
`as`, it is `a`, the first component, which is what Python binds.

Without this, `local_import`'s env still says `other = a` after the import,
and the tree shows `a`, an edge Python never takes. This is step 03's
decision 5 again: a binding you don't resolve yourself gets ty's answer, and
is never left stale.

### Decision 3: a module is a value, and it keeps ty's module type

```rust
pub enum AbstractValue<'db> {
    Function(Definition<'db>),
    Module(Type<'db>),        // ← new: ty's module literal type
    Ty(Type<'db>),
    Unknown,
}
```

`sem::shape` gains `Shape::Module` for `Type::ModuleLiteral`.

**Why keep the `Type` and not just the module:** whether `pkg.sub` is an
attribute of `pkg` depends on which file imported what. `import pkg.sub.mod`
makes `pkg.sub` reachable. `import pkg` alone does not, unless
`pkg/__init__.py` imports it. ty's module literal type records the importing
file for exactly this reason, and its member lookup uses it. That is the
difference between `deep` (edge) and `no_submodule` (no edge).

**Rejected:** `Module(Module<'db>)`, the module alone. It drops the importing
file, so a lookup on it can't tell `deep` from `no_submodule`.

### Decision 4: `x.name` is answered by what `x` holds

```rust
Expr::Attribute(attr) => {
    let recvs = self.eval(&attr.value, env);       // a.b.c evaluates a.b, which evaluates a
    let mut out = Values::new();
    for recv in recvs {
        out.extend(self.member(recv, &attr.attr.id, attr, env));
    }
    dedup(&mut out);
    out
}

fn member(&mut self, recv: AbstractValue<'db>, name: &Name, attr: &ExprAttribute,
          env: &mut Env<'db>) -> Values<'db> {
    match recv {
        AbstractValue::Module(m) => match self.sem.member(m, name, self.frame.file) {
            Some(t) => self.lift(t),
            None => smallvec![AbstractValue::Unknown],       // not an attribute: no edge
        },
        // Everything else, for now: ty's type for the whole attribute expression.
        // Part 03 adds instances.
        _ => self.lift(self.sem.type_of(&self.frame.model, attr.into())),
    }
}
```

`sem.member` is the `vnoc::member` wrapper you already have
(`types/vnoc.rs`): `getattr(<value of this type>, name)`, as ty resolves it.

**Why dispatch on the receiver's value, when ty can type `util.helper`
directly?** Because the receiver's value may come from somewhere ty can't see.
In step 07, `def run(m): m.helper()` is called as `run(util)`. ty sees `m` as
`Unknown`, and its type for `m.helper` is `Unknown` too. Your env knows
`m = Module(util)`, and `member` turns that into `util.helper`. That fixture is
in step 07. You build `member` now because this is where modules are the
topic.

**Why each hop is one `member` call:** `pkg.sub.mod.work` is three attribute
expressions inside each other. `eval` on the outer one evaluates the inner one
first, so depth needs no special code. The same will hold for
`self.a.b.c` in part 03.

### Yours

- `Shape::Module`, and `sem.member(ty, name, file)` over `vnoc::member`.
- `AbstractValue::Module`, the `Expr::Attribute` arm, `member`.
- `sem.type_of_import(model, alias)`. `Stmt::Import` / `Stmt::ImportFrom` bind their names to it.
- A `dedup(&mut Values)` helper: keeps the first of each value, in order.

<details><summary>Reference: this step's code</summary>

```rust
// src/sem.rs (added)
use ty_python_semantic::types::vnoc;

pub enum Shape<'db> {
    Function(Definition<'db>),
    Module,                      // ← new. The value keeps the Type itself
    Union(Vec<Type<'db>>),
    Unknown,
    Other,
}

// in shape():
//     Type::ModuleLiteral(_) => Shape::Module,

/// `getattr(<value of type ty>, name)`, as ty resolves it.
pub fn member(&self, ty: Type<'db>, name: &str, file: ProgramFile<'db>) -> Option<Type<'db>> {
    let env = ProgramEnvironment::from_file(file);
    vnoc::member(self.db, &env, ty, name)
}
```

```rust
// src/interp/value.rs: lift gains one arm
Shape::Module => smallvec![AbstractValue::Module(ty)],

pub fn dedup(vals: &mut Values<'_>) {
    let mut seen = Vec::with_capacity(vals.len());
    vals.retain(|v| {
        if seen.contains(v) { false } else { seen.push(*v); true }
    });
}
```

```rust
// src/sem.rs (added)
/// The type an import statement binds for `alias`, as ty sees it.
pub fn type_of_import(&self, model: &SemanticModel<'db>, alias: &ast::Alias) -> Type<'db> {
    alias.inferred_type(model).unwrap_or(Type::unknown())
}
```

```rust
// src/interp/walk.rs: new statement arms
Stmt::Import(s) => {
    for alias in &s.names {
        let name = match &alias.asname {
            Some(as_name) => as_name.id.clone(),
            None => Name::new(alias.name.id.split('.').next().unwrap()),
        };
        let ty = self.sem.type_of_import(&self.frame.model, alias);
        let v = self.lift(ty);
        env.bind(&name, v);
    }
}
Stmt::ImportFrom(s) => {
    for alias in &s.names {
        let name = alias.asname.as_ref().unwrap_or(&alias.name).id.clone();
        let ty = self.sem.type_of_import(&self.frame.model, alias);
        let v = self.lift(ty);
        env.bind(&name, v);
    }
}
```

**[check]** what ty's `inferred_type` on an `Alias` returns for
`import a.b.c` without `as`: the binding is `a`, so it should be `a`'s module
type. If it gives `a.b.c`'s, ask for the first component's type instead.

```rust
// src/interp/member.rs
impl<'a, 'db> Walk<'a, 'db> {
    pub fn eval_attribute(&mut self, attr: &ExprAttribute, env: &mut Env<'db>) -> Values<'db> {
        let recvs = self.eval(&attr.value, env);
        let mut out = Values::new();
        let mut asked_ty = false;
        for recv in recvs {
            match recv {
                AbstractValue::Module(m) => match self.sem.member(m, &attr.attr.id, self.frame.file) {
                    Some(t) => out.extend(self.lift(t)),
                    None => out.push(AbstractValue::Unknown),
                },
                _ if !asked_ty => {
                    asked_ty = true;
                    let ty = self.sem.type_of(&self.frame.model, ExprRef::from(attr));
                    out.extend(self.lift(ty));
                }
                _ => {}
            }
        }
        dedup(&mut out);
        out
    }
}
```

`eval` routes `Expr::Attribute(a) => self.eval_attribute(a, env)`.

</details>

---

## Run

```rust
#[test]
fn p01_04_imports() {
    check("01-functions/python/imports.py");
    check("01-functions/python/imports_partial.py");
}
```

**[check]** `no_submodule`. If it shows `pkg.sub.mod.work`, ty is treating
`pkg.sub` as visible from `imports_partial.py`. Find out why before working
around it, since that would mean ty's rule differs from the one this step
assumes.

---

## Where you are

```
AbstractValue = Function | Module(Type) | Ty | Unknown
member(recv, name):  Module → vnoc::member;  anything else → ty's type for the expression
```

---

## Exercise

1. `import util` inside a function, followed by `util.helper()`. Trace it:
   which arm of `eval` handles `util`, and what value does it produce?
2. Why does `member` return `[Unknown]` for a missing module attribute, and not
   an empty `Values`?
3. Name one thing ty's import resolution handles that you would have had to
   write yourself with a resolver of your own.

<details><summary>Answers</summary>

1. `Stmt::Import` binds `util` in the env to ty's type for the import, a
   module literal, lifted to `Module(ty)`. The `Name` arm finds it in the env.
   Then `member` asks `vnoc::member` for `helper` on it.
2. Empty `Values` means "no value reaches this point": every path to it raised.
   A missing attribute is a value we know nothing about (in Python, the line
   raises, but the analysis keeps walking). `Unknown` gives no edge and no
   guess. Step 10 is where empty `Values` start to matter.
3. Any of: search paths and site-packages discovery, `__init__.py` and
   namespace packages, relative imports (`from .x import y`), stub files
   (`.pyi`) shadowing sources, `__all__`, conditional imports.

</details>

---

→ Next: [`05-following-a-call.md`](05-following-a-call.md)
