# 00 — Values: answers

Don't read this until you've written your own answers to [`exam.md`](exam.md).

1. `Function(f)`: a module-level `f`, named. `Closure`: a nested `def`, named,
   inside the function that defined it. `Class(C)`: `C`, named. `Instance`:
   `C()`, or a parameter holding an object. `BoundMethod`: `obj.method`, named.
   `Module(m)`: `import m`, then `m`. `Ty(t)`: a literal, a stdlib return.
   `Pending`: the value of a call into a recursion cycle that hasn't been
   resolved yet. `Unknown`: a value nobody knows, e.g. a parameter the key holds
   nothing for and that has no annotation.

2. `Type::Union` is ty's set, used as a whole. `Values` keeps the members
   separate and enumerable, so each one can produce its own calls and its own
   edges. Collapsing `Values` into a `Type::Union` rebuilds
   `ty_ide::outgoing_calls`: two objects with different state merge into one
   type, and `self.handler` no longer knows which handler it holds.

3. Empty means no value reaches this point: every path to it raised, or it
   waits on a cycle that hasn't produced anything. `[Unknown]` means a value
   exists and nobody knows what it is.

4. A `Box` makes the enum non-`Copy`, so every env clone that touches a bound
   method would allocate or chase pointers. Interning the receiver in a
   per-walk arena keeps the enum `Copy` and dedupes receivers, which repeat a
   lot.

5. `Entry(path)`: an object the caller passed in, named by its path from the
   inputs. `New { site, .. }`: an object built in this walk, at that site (or
   one a callee built, returned to this site). `Beyond`: an input object deeper
   than `max_path`, which holds no state.

6. In `main`: `New { site of the constructor }`. In `run`: `Entry(Param(0))`.
   Origins are walk-local. Each walk names objects by what it can see: `main`
   saw it built, `run` saw it arrive as parameter 0.

7. Every instance ty hands you shares the one `OPAQUE` origin. If it held
   state, a write to one would show up on all of them:
   `a = get_logger(); b = get_db(); a.handler = FileHandler(); b.handler.write()`
   would resolve to `FileHandler.write`. So writes to it are dropped, and reads
   go to the class and ty.

8. The arguments were evaluated eagerly at each call site, in that caller's
   env, and became the callee's key. `inner`'s walk starts from its key, where
   `y` is already the `JsonWriter`. The chain is flattened at every call, so
   nothing ever walks back.

9. When a parameter's fingerprint in the key is `Unknown`. It starts the
   parameter from its annotation. It can't make a traced value worse because it
   never fires when the key holds a value. It is counted in `Stats`, because an
   annotation is a claim, not a proof.

10. Both: `JsonWriter` and `CsvWriter`. Conditions are never evaluated, so every
    arm is possible; each arm starts from the state before the `if`, and the
    arms are joined after it. In the second case, the first `w.write()` sees only
    `JsonWriter` and the second only `CsvWriter`: on one path, a later assignment
    replaces an earlier one. Ignoring branches is not the same as ignoring
    assignment order.
