# Object 1 — `Def` and `child_defs`

The one rule for which defs exist, and the two passes that must not disagree
about it.

---

## What it is

Two types and one function. The function is the whole object; the types exist so
the function can return something better than `AnyNodeRef`.

```rust
use ruff_python_ast::statement_visitor::{StatementVisitor, walk_body, walk_stmt};
use ruff_python_ast::{Stmt, StmtClassDef, StmtFunctionDef};
use ruff_text_size::TextRange;

#[derive(Clone, Copy)]
pub enum Def<'a> {
    Function(&'a StmtFunctionDef),
    Class(&'a StmtClassDef),
}

impl<'a> Def<'a> {
    pub fn body(self) -> &'a [Stmt] {
        match self {
            Def::Function(f) => &f.body,
            Def::Class(c) => &c.body,
        }
    }

    pub fn name(self) -> String {
        match self {
            Def::Function(f) => f.name.to_string(),
            Def::Class(c) => c.name.to_string(),
        }
    }

    pub fn kind(self) -> DefKind {
        match self {
            Def::Function(_) => DefKind::Function,
            Def::Class(_) => DefKind::Class,
        }
    }

    /// Ruff's own range — decorators INCLUDED. See exercise 02 object 3.
    pub fn range(self) -> TextRange {
        match self {
            Def::Function(f) => f.range,
            Def::Class(c) => c.range,
        }
    }
}

/// ★ The one rule for "which defs exist".
///
/// Looks inside `if`, `for`, `while`, `try`, `with` and `match`. Stops at each
/// def — call again on `def.body()` for the nested ones.
pub fn child_defs<'a>(body: &'a [Stmt]) -> Vec<Def<'a>> {
    struct Finder<'a> {
        out: Vec<Def<'a>>,
    }

    impl<'a> StatementVisitor<'a> for Finder<'a> {
        fn visit_stmt(&mut self, stmt: &'a Stmt) {
            match stmt {
                Stmt::FunctionDef(f) => self.out.push(Def::Function(f)),
                Stmt::ClassDef(c) => self.out.push(Def::Class(c)),
                _ => walk_stmt(self, stmt),
            }
        }
    }

    let mut finder = Finder { out: Vec::new() };
    walk_body(&mut finder, body);
    finder.out
}
```

That is all of it. Eighteen lines carry a rule that two passes, three RPCs and
every node in the graph depend on.

---

## ★★ Why it is one function and not two walks

Collection (exercise 02, file 8) builds the outline. Injection (object 3) plans
the edits. Both have to answer "what are this scope's defs?", and the answer has
to be the same answer.

Give each its own walk and the failure is this:

> Collection looks inside `if TYPE_CHECKING:`. Injection does not.
>
> The def appears in the outline with `id: null` — **forever**. It never becomes
> a graph node. Nothing errors. The file on disk is correct, the outline is
> well-formed, and the def is simply invisible to v-noc.

Note which way the asymmetry cuts. If *injection* over-reaches and collection
under-reaches, you get a stray ID in a docstring nobody reads — ugly, harmless.
The other direction is a permanently missing node, and the only symptom is
absence. There is nothing to grep for.

So the rule is extracted, not duplicated. `python/shared_rule.py` is the
regression test: sixteen defs, each in a different statement position.

```
only_when_type_checking      ← if TYPE_CHECKING:
in_try  in_except  in_else  in_finally
in_with
in_for  in_for_else
in_while  in_while_else
in_match_case  in_match_default
outer
  nested_in_if               ← if inside a function body
  NestedClass
    method_in_if             ← if inside a CLASS body
```

Sixteen, verified with `ast` on the fixture. Several of them never *execute* — a
`match` runs one case, a successful `try` skips its handler — which is exactly
the point: **"which defs exist" is a syntactic question**, and the two passes
are both syntactic.

---

## Why `StatementVisitor` is the right trait

```rust
// ruff_python_ast/src/statement_visitor.rs:6   [verified]
pub trait StatementVisitor<'a> {
    fn visit_body(&mut self, body: &'a [Stmt]) { walk_body(self, body) }        // :7
    fn visit_stmt(&mut self, stmt: &'a Stmt)   { walk_stmt(self, stmt) }        // :10
    fn visit_except_handler(&mut self, h: &'a ExceptHandler) { … }              // :13
    fn visit_elif_else_clause(&mut self, c: &'a ElifElseClause) { … }           // :16
    fn visit_match_case(&mut self, c: &'a MatchCase) { … }                      // :19
}
```

Five methods instead of `SourceOrderVisitor`'s thirty-three, and no `enter_node`
to get wrong. `walk_stmt` has **eight arms** — `FunctionDef`, `ClassDef`, `For`,
`While`, `If`, `With`, `Match`, `Try` — then `_ => {}`
**[verified]** (`statement_visitor.rs:30`–`:78`).

The three extra `visit_*` methods are what make the fixture pass without you
writing anything: `walk_stmt`'s `Try` arm visits `body`, then each handler, then
`orelse`, then `finalbody`; its `If` arm visits `body` then each
`elif_else_clause`; its `Match` arm visits each case. All four of `in_try`,
`in_except`, `in_else`, `in_finally` are found by the default walk.

### The `_ => {}` arm is complete, not a shortcut

`Stmt` has **25 variants** (`generated.rs:125` **[verified]**) and exactly eight
of them own a body. The other seventeen — `Return`, `Assign`, `Import`,
`Expr`, `Pass`, `TypeAlias`, … — contain no statements, so there is nothing
below them to visit.

### ★ And an expression-blind walk misses nothing

> **No `def` or `class` can live inside an expression.** Python has exactly one
> definition form that is an expression — `lambda` — and a lambda's body is an
> expression, so it holds no statements and no docstring.

```python
(lambda: "ID: fake-uuid").__doc__   # None
```

A string in that position is the lambda's **return value**. So no ID can live
there, and `child_defs` is *complete* for every ID-bearing definition rather
than merely adequate. This is also why exercise 02's quirk 8 (`ExprLambda =>
Skip`) has no counterpart here: there is no expression walk to skip out of.

---

## Rust notes

**`Def` instead of `AnyNodeRef`.** Exercise 02's `node_for_definition` ends with

```rust
_ => unreachable!("scanner only pushes defs"),
```

because `AnyNodeRef` has 94 variants and only two of them are legal there. `Def`
has two variants, so the arm disappears: **the invalid state is
unrepresentable**, and `body()`/`name()`/`kind()`/`range()` are total functions.
That is worth the twenty lines of boilerplate, because `body()` is called from
both passes and from `id_from_body`.

**`#[derive(Clone, Copy)]`.** `Def<'a>` is a tag plus a pointer. Pass it by
value; never by reference.

**`body` is a `ThinVec`, not a `Vec`.** `StmtFunctionDef.body` and
`StmtClassDef.body` are both `thin_vec::ThinVec<Stmt>` (`generated.rs:9310`,
`:9323`; `pub type Suite = ThinVec<Stmt>` at `nodes.rs:3734`) **[verified]**.
`&f.body` coerces to `&'a [Stmt]` at the return position through `Deref`, so
`body()` compiles as written — but a *signature* that says `&Vec<Stmt>` will
not. Take and return `&[Stmt]` everywhere.

**The nested `struct Finder` inside the function.** The visitor has no
callers outside `child_defs` and no state worth naming, so it does not need to
be a module-level type. Declaring it in the function body keeps the API to one
public function and makes it impossible to call the walk without the wrapper.

**`Vec<Def<'a>>` allocates per scope.** On a 200-file corpus that is one small
`Vec` per def, which is noise next to the parse. A callback form
(`for_each_child_def(body, |def| …)`) avoids it if you ever measure a reason;
the returned `Vec` is easier to read and to test, so start there.

---

## Example — the outline, and the edit plan, from one rule

Collection builds a tree:

```rust
fn outline(body: &[Stmt]) -> Vec<DefNode> {
    child_defs(body)
        .into_iter()
        .map(|d| DefNode {
            id: id_from_body(d.body()),     // exercise 02, object 6
            name: d.name(),
            kind: d.kind(),
            range: d.range(),
            children: outline(d.body()),    // ★ recursion on the same rule
        })
        .collect()
}
```

Injection plans edits:

```rust
pub fn plan_ids(body: &[Stmt], source: &str, new_id: &mut impl FnMut() -> String) -> Vec<Edit> {
    let mut edits = Vec::new();
    for d in child_defs(body) {
        if id_from_body(d.body()).is_none() {
            edits.push(id_edit(d, source, new_id()));   // object 2 + object 3
        }
        edits.extend(plan_ids(d.body(), source, new_id));
    }
    edits
}
```

Same iteration, same recursion, different payload. Object 3 finishes
`plan_ids`; what matters here is that **neither function decides what a def
is**.

### ⚠ The shared rule includes the *reader*

`child_defs` answers "which defs exist". `id_from_body` answers "does this one
already have a key". Both halves must be shared, and the second is the one
people forget.

`python/idempotent.py:prose_mentions_id` has no `ID:` key — only prose that
mentions one:

```python
def prose_mentions_id():
    """The text says ID: nothing-real, and there is no real key below.

    Owner: platform
    """
```

Measured, with the Python driver's own regex: `ID` → `'nothing-real,'` (comma
included). So `id_from_body` returns `Some`, and injection **skips this def**.
If injection asked a stricter question — "is there a line that *starts* with
`ID:`?" — it would inject a second key, and then the reader's last-wins rule
would pick whichever came last, and which def owns which graph node would depend
on the order of two things that were never meant to be ordered.

Whether `prose_mentions_id` *should* be skipped is a product question. That both
passes must give it the same answer is not.

---

## Exercise

**A.** Print the sixteen defs of `shared_rule.py` with their nesting. Compare
against the list in this file.

**B.** Delete the `_ => walk_stmt(self, stmt)` arm (return instead). How many of
the sixteen survive? Name the arm each lost def needed.

**C.** Now make the def arms call `walk_stmt` **as well as** pushing. Run on
`shared_rule.py:outer` and explain the duplicates.

**D.** Point `definitions` (exercise 02, file 8) at `child_defs` and diff its def
list against `plan_ids`' — on all ten fixtures. They must be identical. Write
the diff as a test; it is the regression test for this whole object.

**E.** Write the `if TYPE_CHECKING:` disagreement on purpose: give injection a
walk that skips `Stmt::If`. Run both passes on `shared_rule.py`, then describe
the symptom a v-noc user would report.

**F.** Prove the lambda claim three ways: `(lambda: "x").__doc__`, a
`child_defs` run over a file whose only "definition" is a lambda, and an attempt
to write a `def` inside a lambda body.

**G.** `Def` is `Copy`. Try `&Def<'a>` in `body()`'s signature instead and see
what the compiler says about returning `&'a [Stmt]` from it.

---

## Exam

**1.** What is the one rule this object owns, and who calls it?

**2.** Collection and injection disagree about `if TYPE_CHECKING:`. Describe the
symptom in each direction, and say which one is dangerous.

**3.** `walk_stmt` has eight arms. Name them, and say why the ninth case
(`_ => {}`) is complete rather than a gap.

**4.** `in_except` and `in_finally` are found without you writing any code. What
finds them?

**5.** Why is a statement-only walk complete for ID injection? Name the one
exception Python has and say why it does not matter.

**6.** `Def` has two variants where `AnyNodeRef` has 94. What does that buy, in
terms of the code you do *not* write?

**7.** `child_defs` does not descend into a def it has found. Why not, and what
does the caller do instead?

**8.** Half the defs in `shared_rule.py` never execute. Why is the fixture still
correct?

**9.** `id_from_body` is the other half of the shared rule. What goes wrong if
injection asks a stricter question than collection?

---

## Answers

**1.** "Which defs exist in this scope" — `child_defs`. Collection calls it to
build the outline; injection calls it to plan edits. Nothing else is allowed to
decide what a def is.

**2.** If collection sees the def and injection does not, the def sits in the
outline with `id: null` forever, never becomes a graph node, and emits no error
— permanent and silent. If injection sees it and collection does not, you get an
unread ID in a docstring: untidy, harmless. The first is dangerous because the
only symptom is a missing node.

**3.** `FunctionDef`, `ClassDef`, `For`, `While`, `If`, `With`, `Match`, `Try` —
the eight `Stmt` variants that own a body. `Stmt` has 25 variants; the other 17
contain no statements, so there is nothing below them to visit.

**4.** `walk_stmt`'s `Try` arm, which visits `body`, then every handler via
`visit_except_handler`, then `orelse`, then `finalbody`. All three extra
`visit_*` methods on the trait exist for exactly this: handlers, elif/else
clauses and match cases are not `Stmt`s, so they need their own hook.

**5.** Because no `def` or `class` can appear inside an expression. The one
expression-form definition is `lambda`, whose body is an expression — it cannot
hold a statement, and a string there is its return value, not a docstring
(`(lambda: "x").__doc__` is `None`). No ID can live in a lambda, so an
expression-blind walk misses nothing that matters.

**6.** The `unreachable!()` arm. With `AnyNodeRef`, `body()` must match 94
variants and panic on 92 of them; with `Def`, `body()`, `name()`, `kind()` and
`range()` are total. The invalid state is unrepresentable rather than asserted
against.

**7.** Because the caller needs the nesting. `child_defs` returns one scope's
direct children; the caller recurses on `def.body()` and decides what to do with
the structure — a tree of `DefNode`s for collection, a flat `Vec<Edit>` for
injection. Descending inside `child_defs` would force one of those shapes on
both callers.

**8.** Because both passes are syntactic. `child_defs` reads the AST; it has no
idea which branch runs. A def inside `case _:` exists in the file, needs an ID
in the file, and will be read back from the file — execution never enters it.

**9.** Injection would inject a *second* key into a docstring collection already
considers keyed. The reader takes the last pair for a key, so which value wins
depends on where the new one landed — and the graph node a def owns becomes a
function of insert position. Both halves of the rule, `child_defs` **and**
`id_from_body`, have to be the same code.

---

**Next:** [`02-object-slot.md`](02-object-slot.md) — where, exactly, the key
goes.
