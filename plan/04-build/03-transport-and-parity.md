# 04.03 — Transport and proving parity

Making it a drop-in replacement, and proving it is one.

---

## The transport contract

From `server.py` **[verified]**:

```
argv:    --host 127.0.0.1  --port 9002      (port 0 = pick a free one)
stdout:  READY port=<n>\n                   (flushed immediately)
serving: POST /rpc, JSON-RPC 2.0
```

v-noc's process manager reads that `READY` line to learn the port. **Emit it
before binding does anything slow**, and flush. In Rust, stdout is line-buffered
when attached to a terminal but **block-buffered when piped** — which is exactly
how a process manager runs you. Explicit flush or you deadlock the parent:

```rust
println!("READY port={port}");
std::io::Write::flush(&mut std::io::stdout())?;
```

This is the single most likely "it works when I run it manually but hangs under
v-noc" bug.

## Stack

`axum` + `tower` for HTTP, `serde_json` for the payloads. Don't reach for a
JSON-RPC framework — the surface is six methods and the fastapi-jsonrpc error
shape is easier to match by hand than to configure.

```rust
#[derive(Deserialize)]
struct Request { jsonrpc: String, method: String, params: Value, id: Value }

#[derive(Serialize)]
#[serde(untagged)]
enum Response {
    Ok    { jsonrpc: &'static str, result: Value, id: Value },
    Error { jsonrpc: &'static str, error: RpcError, id: Value },
}
```

**Match `fastapi-jsonrpc`'s error codes exactly.** Capture a few real error
responses from the Python driver during M0 and assert against them. v-noc may
branch on them.

## The `shutdown` quirk

```python
# rpc.py:39-42  [verified]
class ShutdownParams(BaseModel):
    """JSON-RPC ``shutdown`` may send ``params: {}``."""
    model_config = ConfigDict(extra="ignore")
```

Accept `params` absent, `null`, `{}`, or anything with extra fields. Return
`{"status": "ok"}`. Use `#[serde(default)]` + ignore unknown fields (serde's
default) and it falls out.

## Concurrency model

Python: every method wrapped in `run_in_threadpool` — no real parallelism (GIL).

Rust, replacing it:

```rust
struct State {
    db: Arc<RwLock<ProjectDatabase>>,
}

// Read path (parse_file, resolve_calls, MRO): take a read lock, clone a
// snapshot, drop the lock, analyse off-lock.
let snapshot = { state.db.read().await.clone() };
let result = tokio::task::spawn_blocking(move || analyse(&snapshot)).await?;

// Write path (ID injection, file sync): brief write lock. Cancels in-flight
// queries — expected; see 01-crates/02.
{ let mut db = state.db.write().await; File::sync_path(&mut db, &path); }
```

`spawn_blocking` matters: analysis is CPU-bound and will stall the tokio
reactor otherwise. That's the direct analogue of `run_in_threadpool`, except
here it buys actual parallelism.

**Handle cancellation.** A `&mut db` from a concurrent write unwinds in-flight
queries. Catch it and retry once:

```rust
match std::panic::catch_unwind(AssertUnwindSafe(|| analyse(&snapshot))) {
    Ok(r) => r,
    Err(e) if is_salsa_cancellation(&e) => { /* re-snapshot, retry once */ }
    Err(e) => std::panic::resume_unwind(e),
}
```

This has no Python analogue and is easy to forget until it shows up as
intermittent 500s under editing load.

## Logging

`tracing` + `tracing-subscriber` to **stderr** — stdout carries the `READY`
line and must stay clean. Log at `initialize`:

```
pylspt 0.1.0  ruff@ac201b8
project        = /path/to/project
python_version = 3.12  (source: pyproject requires-python)
search_paths   = [...]
limits         = depth=24 nodes=100000 deadline=10s
```

Per request at debug: method, duration, node count, budget consumed, and every
truncation counter from
[`03-call-tree/08`](../03-call-tree/08-termination-and-cycles.md).

---

## Proving parity

### The differ

```
corpus/*.py ──┬──► python driver ──► responses_py/*.json ──┐
              └──► rust driver   ──► responses_rs/*.json ──┴──► diff
```

Normalise before comparing:
- **UUIDs.** IDs are random. Either pre-inject IDs into the corpus so both
  drivers read the same ones (**preferred** — keeps the comparison honest), or
  canonicalise UUIDs to sequential placeholders per file.
- **Key order.** Compare parsed `serde_json::Value`, not strings.
- **`children` order.** Should match (source order); if it doesn't, that's a
  finding, not something to normalise away.

### Tiers

| Tier | Compares | Must be |
|---|---|---|
| 1 | `parse_file` nodes, IDs pre-injected | **exact** |
| 2 | `base_classes` (MRO) | exact modulo documented qname rules |
| 3 | `resolve_calls` on fixtures | **exact** |
| 4 | `resolve_calls` on real corpus | ≥95% node-set overlap; every diff triaged |

Tier 4 will not be 100%, and chasing 100% is the wrong goal. ty and Jedi
genuinely disagree on some inference. What matters is that **every difference is
explained and categorised**:

```
DIVERGENCE LOG
  ty resolves, jedi didn't ......... 47   ← improvement, keep
  jedi resolved, ty didn't ......... 12   ← investigate each
  different callee resolved .........  3  ← investigate URGENTLY
  ordering .......................... 0
```

The third row is the dangerous one. The first is a feature.

### Determinism test

```rust
#[test]
fn deterministic() {
    let a = run_request(&req);
    for _ in 0..10 { assert_eq!(a, run_request(&req)); }
}
```

Catches hash-map iteration order and rayon merge order. Run it in CI from M5.
→ [`03-call-tree/09`](../03-call-tree/09-path-identity.md#determinism-)

### Performance comparison

Same corpus, same machine, both drivers, cold and warm:

| | Python | Rust cold | Rust warm |
|---|---|---|---|
| `parse_file` p50 | | | |
| `parse_file` p95 | | | |
| `resolve_calls` p50 | | | |
| `resolve_calls` p95 | | | |
| full project scan | | | |

"Warm" means the salsa db has already seen the project — the case that matters
in an editor, and where the win should be largest. If warm `resolve_calls` isn't
dramatically better, the memoisation work in M8 is where to look.

---

## Rollout

1. Ship behind a v-noc config flag: `python_driver = "jedi" | "pylspt"`.
2. Run both in shadow mode if you can afford it — serve Jedi's answer, log the
   diff.
3. Flip the default once the divergence log is empty of "different callee".
4. Keep the Python driver in-tree for one release as a fallback and as the
   parity oracle.

Do not delete `vnoc_lsp_python` when the Rust one ships. It is the only
independent implementation of the spec you have, and you will want it the first
time someone reports a tree that looks wrong.

---

→ Reference: [`05-reference/api-cheatsheet.md`](../05-reference/api-cheatsheet.md)
