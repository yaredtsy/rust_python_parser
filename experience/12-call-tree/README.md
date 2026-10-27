# 12 — The call tree analysis

**Goal:** you can look at any Python call site — a plain call, a method call, a
callback, a factory, a chained attribute, a recursive call — and say which
contexts it creates, what each callee's summary says, which edges it produces,
and how the tree v-noc shows is rendered from them. By the end,
`plan/03-call-tree/` reads like a description of a machine you already
understand.

This is the ~70% of the port `plan/README.md` budgets the most effort for. It is
taught the same hands-on way as exercises 00–11, before you write `src/interp/`:
fixtures, worked traces, and the real Jedi driver as ground truth.

---

## The one idea to hold onto

> Each function body is walked **once per distinct context**. A call is answered
> by the callee's **summary** for that context. The per-path tree is
> **rendered** from the summaries at the end.

Everything in this exercise is either what happens inside one walk (values,
environments, binding, attributes), or how walks are tied together (keys,
summaries, the solver, recursion, rendering).

---

## How this exercise is organised

Four folders. Read them in order.

```
12-call-tree/
├── 00-values/ ....... what flows through one walk: AbstractValue, origins,
│                      Env, lift, the annotation fallback
│                      covers: plan/03-call-tree/03, 04, 05
│
├── 01-summaries/ .... how walks are tied together: the key, the summary,
│                      one call (resolve, then write), the solver, the graph
│                      and the rendered tree
│                      covers: plan/03-call-tree/08, 09, 10, 13, 14
│
├── 02-functions/ .... every shape a plain function call takes: defaults,
│                      *args, closures, returns, callbacks, curry, decorators,
│                      recursion
│                      covers: plan/03-call-tree/06, 10, 11, 12
│
└── 03-classes/ ...... every shape a method call takes: construction, self,
                       attribute state, two instances, writes anywhere and
                       deep writes, static/class/property, chains and the MRO
                       covers: plan/03-call-tree/07, 10
```

| | folder | what it gives you |
|---|---|---|
| 1 | [`00-values/`](00-values/README.md) | the vocabulary of one walk |
| 2 | [`01-summaries/`](01-summaries/README.md) | ★ the idea that makes the analysis scale |
| 3 | [`02-functions/`](02-functions/README.md) | the function-call cases, one file per case |
| 4 | [`03-classes/`](03-classes/README.md) | the method-call cases, one file per case |

Then [`exam.md`](exam.md), across all four.

---

## Read first

- `plan/03-call-tree/01-what-jedi-actually-does.md` and
  `02-why-ty-alone-cannot.md`: the problem, once, before any mechanism
- `plan/03-call-tree/03-design.md`: the design on one page. You'll come back to
  it after every folder
- `experience/08-classes-and-mro/README.md`: the `vnoc::member` wrapper that
  `03-classes/07` leans on is already built and tested in this repo

---

## There is no Rust to run yet

`src/` has no `interp/` module yet. That is what you will write, and this
exercise comes before it. So "predict, then run" means:

1. **Predict** the contexts, summaries and edges by hand, using the rules here.
2. **Check** against the real Jedi driver:
   `/Users/yared/Documents/Programing/ide/v-noc/src/lsp/py/vnoc_lsp_python/call_resolver.py`
   is runnable. Remember the differences on purpose: Jedi drops recursive calls
   (pylspt shows them as `Recursive` leaves), and Jedi's tree is already merged
   (pylspt sends one node per call site, and v-noc merges on save).
3. **Compare** both with the fixtures in `plan/03-call-tree/`. Several fixtures
   here are those, as runnable Python.

When you build `src/interp/`, the exams here are your first integration tests.

---

## Done when

- [ ] you've finished all four folders' own checklists
- [ ] you can draw the cross-function fixture (`read_json` / `read_csv` /
      `emit`) as contexts, as a graph, and as a rendered tree, and say why the
      same-function version proves nothing
- [ ] you can state the three rules and say what breaks without each one
- [ ] you can trace `Box.replace` and explain why phase 1 reads the snapshot
      before phase 2 writes
- [ ] you can explain how a parser's cycle is resolved with each body walked
      once, and why the answer doesn't depend on which function was called first
- [ ] you can say what `vnoc::member` answers and what it leaves to you
- [ ] you can state, unprompted, that conditions are never evaluated — every
      arm is joined — and that assignment order is still respected

---

→ Start: [`00-values/README.md`](00-values/README.md)
