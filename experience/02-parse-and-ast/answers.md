# Answers 02 — Parse and AST

---

**1.** **Losslessness.** A CST can be printed back to the exact original bytes;
ruff's AST cannot, because whitespace, comments and formatting are not
represented. Consequence: anything that *rewrites* a file must use a CST — which
is why exercise 10 keeps libcst for ID injection even though ruff parses the
same file three lines earlier. Ruff does this itself: `ruff_linter` pulls in
`libcst` for exactly this reason **[verified]**.

**2.** You can still recover: the **source text** of any node (slice the file
with `node.range()`), and the **exact positions** of everything. You cannot
recover anything *between* nodes — comments, blank lines, whether the author
wrote `x=1` or `x = 1`, trailing commas. Trivia is not in the tree.

**3.** Because the AST stores structure, not text. `ExprCall.func` is an
expression *node*; the characters `obj.render` exist only in the source buffer.
This is a feature — no string is copied during parsing, which is a large part of
why ruff parses so much faster than parso.

**4.** `Skip` tells the walker not to descend into this node's children. It
replaces the early `return` at `parser.py:78-80`, which stops `_scan_children`
when it meets a nested `Class` or `Function` so that their calls become children
of *them*, not of the enclosing scope.

---

**5.**

```
ExprCall                              call_index 2   (.strip)
└── func: ExprAttribute .strip
          └── value: ExprCall         call_index 1   (.render)
                     └── func: ExprAttribute .render
                               └── value: ExprCall   call_index 0   (build)
                                          └── func: ExprName "build"
```

All three share `line` and `column` — the chain's start, i.e. the `b` of
`build`. They differ in `end_line`/`end_column` (each call ends at its own `)`)
and in `call_col_pos` (each has its own `(`).

That shared start is not an accident of the port: parso produces it too, because
its `position` comes from the enclosing `atom_expr`. Reproduce it.

**6.**

```rust
let open_paren: TextSize = call.arguments.range().start();
```

> "The `Arguments` node would span from the left to right parentheses
> (inclusive), and contain …" — **[verified, `ruff_python_ast/src/nodes.rs:3483`]**

**7.**

| descend through | tested by |
|---|---|
| `Expr::Attribute` → `.value` | `chained`, `deep_chain` |
| `Expr::Call` → itself (`f()()`) | `call_of_call` |
| `Expr::Subscript` → `.value` | `subscripted` |

**8.** **Three** call nodes, each with `call_index: 0`.

`wrap(...)` is a chain of length one. Each `build()` in the arguments is its own
independent chain, also length one. `call_index` counts position *within a
chain*, not within a statement or a line — so a file can be full of index-0
calls and that is correct.

If you answered "one", you forgot to keep walking into `arguments` after
handling the outer call. That is the most common bug in this exercise and it
loses calls silently.

---

**9.** Ruff starts at **`@`**; parso starts at **`def`**. **[verified]** —
`parse_decorators` takes `start = self.node_start()` before the `@` and hands
that same offset to `parse_function_definition`
(`ruff_python_parser/src/parser/statement.rs:2894, 3021`).

Effect: every decorated function gets a `position` whose `line`/`column` point
at the decorator. On any real codebase — `@property`, `@staticmethod`,
`@pytest.fixture`, `@app.route` — that is a large fraction of all methods, so it
would show up immediately in the M2 byte-identical gate.

**There is a second consequence, and it is worse.** `decorator_list` is a field
*of* `StmtFunctionDef`, so a visitor that walks the def node walks its
decorators too. A call inside a decorator — `@functools.wraps(build)` in
`edges.py` — then becomes a **child of the function it decorates**. In parso the
decorator lives in a `decorated` wrapper node *outside* the funcdef, so it lands
in the enclosing scope instead.

So when you recurse into a def, skip `decorator_list` explicitly. Nothing warns
you about this one; you find it by diffing against the Python driver, or by
reading the struct definition and noticing the field.

**10.** It disappears. Ruff has no `async_stmt` wrapper node — `is_async` is a
`bool` field on `StmtFunctionDef`, and the range already starts at the `async`
keyword **[verified, `statement.rs:2853`]**, because the parser threads
`async_start` into `parse_function_definition`. The parso special case existed
only to reach up to a wrapper node that ruff does not have.

Note the asymmetry with answer 9: ruff includes `async` (which you want) *and*
decorators (which you do not). Same mechanism, opposite outcomes. Neither is
"the ruff way is better" — they are just different, and the contract is parso's
output.

**11.** **One** — the `build()` call.

`lambda x: log(x)` produces no node for the lambda (quirk 8) **and no node for
`log`**, because parso's `Lambda` is a subclass of `Function`, so `_visit_node`
dispatches to `_visit_function`, which returns `None`, and `parser.py:80` then
returns without scanning children. The entire subtree is dropped.

"One call node for `log`" is what you get if you drop the lambda but keep
walking — a reasonable-looking implementation that does not match the
specification.

**12.** They land **under the function**, in both implementations — parso's
`Function` node includes its parameter list, and ruff's `StmtFunctionDef`
includes `parameters`. So walking the def finds them either way, and the outputs
agree.

Worth noticing that this is *semantically* wrong: default arguments are
evaluated in the enclosing scope, at definition time, so a call tree that puts
them inside the function misrepresents when they run. It matches today's
behaviour, so preserve it — and note it as a known quirk rather than a thing to
fix. (Contrast with the decorator case, where the two implementations *disagree*
and you have to choose.)

---

**13.**

| function | ID? | why |
|---|---|---|
| `not_first_statement` | **no** | the string is the second statement. A docstring is the *first* statement of the body — a string anywhere else is a no-op expression |
| `implicit_concat` | yes | `.value.to_str()` concatenates implicitly-adjacent literals for you; the key spans the second part |
| `raw` | yes | `to_str()` handles the `r` prefix. Note the docstring's *content* still contains a literal backslash-n, which is correct |
| `single_quotes` | yes | single quotes are still a string literal, still a docstring |
| `id_like_text` | yes — the **UUID**, not `but` | see below |

`id_like_text` is the one that separates implementations. The prose reads
`ID: but as prose`, so a `(\S+)\s*:\s*(\S+)` scan matches `("ID", "but")` first
and `("ID", "cccc…")` second. Your Python builds a dict from the pairs, so the
later match **overwrites** the earlier and the UUID wins. If your Rust returns
the first match, you produce `but` and the node gets a corrupt ID.

Preserve last-wins. It is not obviously right — it is just what the current
implementation does, and the contract is the output.

**14.** `[3:-3]` assumes exactly three quote characters at each end.

- `r"""…"""` — the prefix is four characters (`r` + `"""`), so slicing three
  leaves a stray `"` at the front of your content.
- `'ID: …'` — single-quoted, one character each side; slicing three eats real
  content from both ends.
- `"""a""" """b"""` — implicit concatenation; the slice keeps the internal
  quotes and the gap.

`.value.to_str()` gives the decoded, concatenated value with no prefix or quote
handling on your side at all.

**15.** They serve different consumers.

`parse_file` is the *structural* view: v-noc needs to see the def exists, and
seeing an `id: null` is precisely what triggers injection. Drop those nodes and
you deadlock — the file never reports functions, so injection never runs, so no
IDs ever appear.

The call tree is the *relational* view, and `target_id` is the join key into the
rest of v-noc. A node with no ID cannot be joined to anything, so it is dropped
along with everything below it (quirk 4). That is lossy on purpose, and it is
the "silent one" the plan warns about in `04-build/00-dev-cli.md`: a callee
vanishes with no other symptom. Log it loudly.

---

**16.** The lambda skip is worth a permanent test. When you remove it, the call
count rises by exactly the number of calls inside lambda bodies — one in
`edges.py`. A count-based assertion catches this class of regression cheaply;
you do not need full snapshot output to notice that a number moved.

**17.** The usual suspects, roughly in order of how often they are wrong on a
first attempt: decorator ranges (9), decorator *contents* landing inside the
function (9), lost calls in arguments (8), and lambda subtrees (11). If all four
are right, check position dedup (quirk 10) with two identical call expressions
on one line.
