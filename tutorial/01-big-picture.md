# 1. The big picture

No code in this chapter. Just the shape of the system.

---

## What Ruff actually is

"Ruff" is one Git repository. Inside it there are **54 Rust packages**
(Rust calls them *crates*). Together they build **two different programs**:

| Program | Built from | What it does |
|---|---|---|
| **ruff** | the `ruff_*` crates | a linter and formatter. Finds style problems. **No types.** |
| **ty** | the `ty_*` crates | a type checker and language server. **This is what you need.** |

So when people say "Ruff is fast", they usually mean the linter. The part you
care about is **ty**, which is built on top of the same low-level crates.

> **Important:** there is a crate called `ruff_python_semantic` and another
> called `ty_python_semantic`. The names look almost the same. They are
> completely different.
>
> - `ruff_python_semantic` = for the linter. It knows about names and scopes.
>   It does **not** know types.
> - `ty_python_semantic` = for the type checker. This one knows types.
>
> You want `ty_python_semantic`. Many people get this wrong.

---

## The pipeline

Here is what happens, from a file on disk to an answer about a type.

```
   your file on disk
   ────────────────
   app.py
        │
        │  (1) read the text
        ▼
   "def f():\n    return 1\n"          ← just a String
        │
        │  (2) parse
        ▼
   AST                                 ← a tree of Rust structs
   ModModule
     └── StmtFunctionDef "f"
           └── StmtReturn
                 └── ExprNumberLiteral 1
        │
        │  (3) build the semantic index
        ▼
   Scopes and Definitions              ← "the name f is defined here"
   module scope
     ├── definition: f  (a function)
     └── scope of f
        │
        │  (4) infer types
        ▼
   Types                               ← "f has type () -> int"
```

Four steps. Each step is a **query**. Each query remembers its answer.

Jedi does the same four steps. The difference is in *when* and *how often*.

---

## Jedi's shape vs ty's shape

### Jedi

```
Script(path)  ──► parso parse ──► lazy inference on demand
   │
   └── every Script is a new starting point
```

Jedi is **lazy**. It parses, then does almost nothing. When you ask a question,
it walks the tree and builds `Value` objects *just for that question*. It
throws most of that work away.

This is good for one question. It is bad for ten thousand questions, which is
what your call tree needs.

### ty

```
Database  ──► query: source_text(file)
          ──► query: parsed_module(file)
          ──► query: semantic_index(file)
          ──► query: infer_scope_types(scope)
   │
   └── ONE database for the whole process.
       Every answer is stored. Ask again = free.
```

ty is **eager but cached**. The first time you ask about a scope, it infers the
whole scope. That costs more than Jedi's one-question answer. But the second,
third, and thousandth question about that scope are free.

> This is the single biggest reason ty is faster for your use case. Your
> current driver builds a new `jedi.Script` **for every call site**
> (`call_resolver.py:74-77`). ty builds one database for the whole process.

---

## The five ideas you need

Everything in ty is built from five ideas. Here they are, next to the Jedi
words you already know.

| # | ty idea | Jedi word | One sentence |
|---|---|---|---|
| 1 | **`TextRange`** | `start_pos` / `end_pos` | Where something is in the file, as byte numbers. |
| 2 | **AST** | parso tree | The shape of the code, as Rust structs. |
| 3 | **Database (salsa)** | *(nothing)* | A cache that remembers every answer. |
| 4 | **`Definition`** | `Name` | The place where a name was created. |
| 5 | **`Type`** | `Value` | What an expression can be. **Not the same as Value.** |

Idea 3 has no Jedi equivalent at all. Idea 5 looks the same but is not — and
that difference is the whole reason the plan is long.

Chapters 4 to 9 take one idea each.

---

## Why idea 5 is different (the short version)

This is the thing to understand. The long version is chapter 9.

```python
def emit(writer, data):
    writer.write(data)
```

**Ask Jedi:** *"what is `writer`?"*
Jedi says: *"depends. Who called `emit`? Let me look at the context."*
If `emit` was called as `emit(JsonWriter(), x)`, Jedi answers `JsonWriter`.
If it was called as `emit(CsvWriter(), x)`, Jedi answers `CsvWriter`.

**Ask ty:** *"what is `writer`?"*
ty says: *"it has no type annotation, so: Unknown."*
And it gives the **same answer every time**, no matter who called `emit`.

ty is not broken. It is answering a different question:

- Jedi answers: **"what would be here if the program ran this way?"**
- ty answers: **"what is true for every possible run?"**

Your tool needs the first question. That is why the plan says you must build an
*interpreter* on top of ty, instead of just calling ty and printing the result.

---

## What you will build

```
┌─────────────────────────────────────────────┐
│  YOUR CODE                                  │
│  walks the tree, keeps track of which       │  ← this is the work
│  value each name holds on this path         │
└──────────────┬──────────────────────────────┘
               │  "I do not know this one"
               ▼
┌─────────────────────────────────────────────┐
│  ty                                         │
│  parsing, imports, classes, MRO, types      │  ← you get this for free
└──────────────┬──────────────────────────────┘
               ▼
┌─────────────────────────────────────────────┐
│  salsa                                      │
│  remembers every answer                     │  ← this is the speed
└─────────────────────────────────────────────┘
```

You ask ty when you do not know. You answer yourself when you do know.

---

## Check yourself

Before moving on, you should be able to answer:

1. What is the difference between `ruff` and `ty`?
2. Which crate has type information: `ruff_python_semantic` or
   `ty_python_semantic`?
3. Why is ty faster than Jedi for ten thousand questions?
4. Why does ty give the same answer for `writer` no matter who called `emit`?

If number 4 is unclear, read the "Why idea 5 is different" section again. It is
the most important idea in this tutorial.

---

→ Next: [`02-rust-refresher.md`](02-rust-refresher.md)
