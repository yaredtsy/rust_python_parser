# 01.01 — Calling a plain function

The base case. Everything else in this folder is a variation on exactly
this trace — read it slowly once, and the rest of the folder goes faster.

**Fixture:** `python/basic.py`. **Reading:**
[`plan/03-call-tree/03-the-abstract-interpreter.md`](../../../plan/03-call-tree/03-the-abstract-interpreter.md)
§"The main loop".

---

## The shape

```python
def emit(writer, payload):
    writer.write(payload)

def main():
    emit(JsonWriter(), 1)
```

One call site, one function, no returns, no defaults, no branching. If your
machinery can't get this right, nothing built on top of it can either.

## The trace

Walk `resolve_call(emit(JsonWriter(), 1), env=<main's env>, frame=<main>)`
step by step, matching `plan/03-call-tree/03`'s pseudocode line for line:

```
1. budget.tick()                              — one global counter, no exceptions
2. builtin fast-path?                         — `emit` isn't a builtin name, skip
3. eval_callee(emit, env, model)
     env.lookup("emit")  → miss (nothing bound this name yet)
     ty: Name "emit" → Type::FunctionLiteral(emit)
     lift(...) → AbstractValue::Function(emit)
4. eval_arguments((JsonWriter(), 1), env, model)   — EAGER, in the CALLER's env
     arg 0: Expr::Call(JsonWriter()) → eval_call
            → Callable::Class(JsonWriter) → fresh OriginId → Instance{JsonWriter, O1}
     arg 1: Expr::NumberLiteral(1) → no env hit → lift(ty's answer) → Ty(int)
     args = { positional: [Instance{JsonWriter,O1}, Ty(int)] }
5. for callee in [Function(emit)]:
     is_project_code(emit)?           → yes
     qname(emit)                      → "basic.emit"
     seen.insert("basic.emit")        → not seen yet, continue
     docstring_id(emit)               → "f1000000-0001-..." → present, continue
     frame.is_ancestor("basic.emit")  → false, continue
     child = frame.add_child("basic.emit", "FunctionSchema/f1000000-0001-...")
     callee_env = bind_parameters(emit, args, receiver=None)
         locals: { writer → Instance{JsonWriter,O1}, payload → Ty(int) }
         scope:  emit's body scope
         parent: None (emit has no enclosing function)
     walk_body(emit, callee_env, child)
         finds `writer.write(payload)`
         resolve_call(writer.write(payload), callee_env, child)   ← recurse
```

That inner `resolve_call` is where you'll land in
[`02-classes/02-self-and-method-binding.md`](../02-classes/02-self-and-method-binding.md) —
`writer.write` is an attribute access on a value that **is** in `env`, which
is exactly the case `00-values/03-env-and-scope.md`'s two-line rule was
built for: `writer` hits `env`, so this never has to ask `ty` what kind of
thing `writer` is.

## Why the cross-scope fixture, not the same-frame one

`basic.py` also has `entry()`, which calls `read_json()` and `read_csv()` —
two *different* functions, each calling `emit` with a different writer. This
is deliberately **not** `main()` calling `emit` twice with two writers from
one frame — that version would merge into a single `emit` node
(`00-values/05`'s `(parent, qname)` rule) and look almost identical to what
a path-*free* tool would produce, proving nothing. `entry()`'s version
produces **two** `emit` nodes, each with exactly one correct leaf — the
shape that only a context-sensitive interpreter gets right.

## Exercise

1. Draw the tree `main()` produces, by hand, labelling each node with its
   qname and its single leaf.
2. Draw the tree `entry()` produces. How many `emit` nodes, and what's under
   each one?
3. In step 4 of the trace above, which argument touches `lift()`, and which
   one never does? Why the difference?

<details><summary>Answers</summary>

1. `main → emit → JsonWriter.write`. One node per level, no branching.
2. Two `emit` nodes: `entry → read_json → emit → JsonWriter.write` and
   `entry → read_csv → emit → CsvWriter.write`. Each `emit` has exactly one
   leaf — the writer *that path* constructed.
3. `payload = 1` touches `lift()` — it's a literal, `env` has nothing to say
   about it, so it falls straight through to `ty`'s inference and back.
   `JsonWriter()` never touches `lift()` at all — constructing a class is
   handled directly by `eval_call`'s case (a) (minting a fresh
   `OriginId`), which is cheaper and more precise than asking `ty` what
   `JsonWriter()` returns and then lifting that answer back.

</details>

---

→ Next: [`02-defaults-and-keywords.md`](02-defaults-and-keywords.md)
