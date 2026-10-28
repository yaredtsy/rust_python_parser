# 03.05 — Writes from anywhere

**You build:** almost nothing: step 03's write rule already covers every
case here. This step proves it on the shapes that matter, and fixes one
thing it exposes: an expression that picks between two objects must keep
both, not ask ty.
**Fixture:** `python/writes.py`
**Plan:** `plan/03-call-tree/07-attributes-and-self.md` § "Write:
`bind_attribute`", `05-the-walk.md` § "`eval`"

---

## The fixture

```python
def install(p, h): p.handler = h
def maybe(p, h, c):
    if c: p.handler = h

def outside():
    p = Pipeline(CsvHandler())
    p.handler = JsonHandler()       # replaces
    p.run()                         # JsonHandler.process
def in_another_method():
    c = Controller()
    c.setup()                       # self.h = JsonHandler()
    c.go()                          # JsonHandler.process
def through_a_function():
    p = Pipeline(CsvHandler())
    install(p, JsonHandler())
    p.run()                         # JsonHandler.process
def on_one_path(c):
    p = Pipeline(CsvHandler())
    maybe(p, JsonHandler(), c)
    p.run()                         # JsonHandler.process, CsvHandler.process
def two_candidates(c):
    a = Pipeline(CsvHandler())
    b = Pipeline(CsvHandler())
    p = a if c else b
    p.handler = JsonHandler()       # a OR b got it
    a.run()                         # JsonHandler.process, CsvHandler.process
```

## Predict

Run it with your step 04 code first.

1. Which blocks pass already? For each, which line of step 03's code made it
   pass?
2. `two_candidates` probably fails. What does `p` hold after `p = a if c else
   b`, with your current `eval`?
3. In `on_one_path`, the write happens inside `maybe`, in one arm of an `if`.
   Where does "old or new" come from: `bind_attribute`, or somewhere else?

---

## Build

### Decision 1: one write rule, whatever the receiver

`self.h = X` in `setup`, `p.handler = X` in `outside`, `p.handler = h` in
`install`: all three are `bind_attribute` on whatever object the receiver
expression evaluates to. `self` is just the case where the receiver is the
first parameter. There is no `__init__` special case, and nothing knows
whether the write is "inside the class".

`in_another_method` and `through_a_function` pass without new code: the write
lands on the object's origin, and the state flows out of the call (step 03,
decision 4).

### Decision 2: assignment order is respected

`outside` writes `handler` twice: in `__init__` (`CsvHandler`), then directly
(`JsonHandler`). One object, so the second write replaces. `run` sees only
`JsonHandler`. Conditions are never evaluated, but statements in sequence are
still in sequence.

### Decision 3: a write on one path is "old or new" after the join

In `maybe`, the write is strong inside its arm (one object), and the arm that
skips it keeps the old value. `Env::join` unions them: `[JsonHandler,
CsvHandler]`. That is the join of step 01.09, applied to `attrs`. Nothing in
`bind_attribute` knows about it.

### Decision 4: one of several objects is a weak write

In `two_candidates`, `p` is `a` or `b`. Python wrote one of them. Which one is
a condition the walker never evaluates, so both get "old or new": the weak
write of step 03, decision 3. `a.run()` shows both handlers.

### Decision 5: an expression that picks between values keeps them

With step 02's `eval`, `a if c else b` falls to the fallback arm: evaluate the
children, then ask ty for the whole expression. ty says "a `Pipeline`", which
lifts to an `OPAQUE` instance. The two objects are lost, and so is the write.

```rust
Expr::If(e) => {
    self.eval(s, &e.test, env);                 // evaluated, ignored
    let mut v = self.eval(s, &e.body, env);
    v.extend(self.eval(s, &e.orelse, env));     // both possible
    dedup(&mut v);
    v
}
Expr::BoolOp(b) => {
    let mut v = Values::new();
    for e in &b.values { v.extend(self.eval(s, e, env)); }   // `a or b`: either operand
    dedup(&mut v);
    v
}
```

**Why it didn't matter before:** for functions, ty's answer for `a if c else b`
was the union of two function literals, and `lift` kept both. For objects,
ty's answer is a class, and a class is not an object. Every expression that
can carry an object through unchanged needs an arm that keeps the walker's
values, or the object turns into `OPAQUE` there. `If` and `BoolOp` are the two
that do it in ordinary code. (`await` is the third, since step 01.10.)

**[check]** `x or default` evaluates `default` only if `x` is falsy. Both
arms are kept, which over-approximates, as every branch does.

### Yours

- The `Expr::If` and `Expr::BoolOp` arms in `eval`.

<details><summary>Reference: this step's code</summary>

The two arms above are the whole change. Put them before the fallback arm.

</details>

---

## Run

```rust
#[test]
fn p03_05_writes() {
    check("03-classes/python/writes.py");
}
```

Then every test, both modes, from parts 01 and 02: the new `If` arm also
changes how `a if c else b` evaluates for functions, and the trees must not
change.

---

## Where you are

`eval` keeps the walker's values through `x if c else y` and `x or y`.
Every write, wherever it happens, is one rule plus the join.

---

## Exercise

1. `p = a or b`, then `p.handler = X`. Strong or weak? What does Python do?
2. `install(p, h)` is called with `p` being two objects. What does `install`'s
   write do?
3. Which other expression kinds could carry an object through unchanged? Which
   of them would you add, and which would you leave to ty?

<details><summary>Answers</summary>

1. Weak: `p` is `[a, b]`, two candidates. Python writes `a` if `a` is truthy,
   else `b`, a condition the walker doesn't evaluate.
2. Inside `install`, `p` holds both objects, so the write is weak: each gets
   its old handler or `h`. The exits carry that back to the caller.
3. A walrus (`(p := a)`, already an arm since step 01.03), a parenthesised
   expression (no node in the AST), a starred expression in a call (a splat:
   `Unknown` by design), and subscripts of containers (`[a, b][0]`: containers
   are not modelled, so ty, on purpose). The rule: add an arm only with a
   fixture that shows a lost edge.

</details>

---

→ Next: [`06-static-class-property.md`](06-static-class-property.md)
