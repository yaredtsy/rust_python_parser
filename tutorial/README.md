# Tutorial: how Ruff and ty work

This is a learning guide, not a plan. The plan (in `../plan/`) tells you **what
to build**. This tutorial teaches you **how the tools work**, so the plan makes
sense when you read it.

---

## Who this is for

You already know Jedi very well. You know how it:

- loads modules and builds a tree
- makes *values* from that tree
- uses *contexts* to know where you are
- resolves definitions, classes, and attributes

You are new to Ruff and ty. You are also brushing up on Rust.

So this tutorial does two things at the same time:

1. Teaches you a Ruff/ty idea.
2. Compares it to the Jedi idea you already know.

And along the way, it teaches the Rust you need to read the code.

---

## How to read this

Read in order. Each chapter uses words from the chapter before it.

If you only have one hour, read chapters **1**, **4**, and **10**. That gives
you the shape of the whole system.

```
tutorial/
│
├── 01-big-picture.md ............. What happens when ty looks at a file.
│                                   Start here. No code, just ideas.
│
├── 02-rust-refresher.md .......... The Rust you need. Enums, match, Option,
│                                   traits, borrowing. With small examples.
│
├── 03-rust-in-ty-code.md ......... Rust patterns you will actually meet in
│                                   ty: 'db lifetimes, &dyn Db, salsa macros.
│
├── 04-positions-and-text.md ...... How ty points at code. Byte offsets
│                                   instead of line/column. Easy and useful.
│
├── 05-parser-and-ast.md .......... Text becomes a tree. parso vs Ruff,
│                                   side by side.
│
├── 06-salsa-the-database.md ...... Why ty is fast. The cache that Jedi
│                                   does not have.
│
├── 07-files-and-modules.md ....... Finding files, following imports.
│                                   Jedi's Script and Project vs ty's Db.
│
├── 08-scopes-and-definitions.md .. Where names live. Jedi's Context vs
│                                   ty's Scope and Definition.
│
├── 09-types-and-inference.md ..... Jedi's Value vs ty's Type. The most
│                                   important difference in this whole port.
│
├── 10-worked-example.md .......... One small Python file, traced through
│                                   Jedi and ty step by step.
│
└── 11-reading-the-source.md ...... How to find things in 54 crates
                                    without getting lost.
```

---

## A note about words

I use simple English on purpose. The ideas are not simple, but the sentences
should be.

When I use a new technical word, I explain it the first time. If you see a word
you do not know and I did not explain it, that is my mistake — the
[glossary in the plan](../plan/05-reference/glossary.md) may help.

Two words I will use a lot:

- **crate** — a Rust package. Like a Python package, but compiled. Ruff has 54
  of them in one folder.
- **query** — a function whose answer ty remembers. Ask it twice, and the
  second time is free.

---

## The one sentence version

> Jedi is a lazy interpreter that pretends to run your code.
> ty is a fast type checker that never pretends anything.
>
> Jedi can tell you *"on this path, this value is here"*.
> ty can tell you *"in all runs, this expression has this type"*.
>
> Your tool needs the first one. ty gives you the second.
> That gap is why the plan exists, and why chapter 9 is the important one.

---

→ Start: [`01-big-picture.md`](01-big-picture.md)
