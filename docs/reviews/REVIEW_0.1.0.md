# REVIEW 0.1.0 — the thread-local store, in cold light

> *"Each SQLite connection (which runs on one thread) gets its own
> isolated Oxigraph store."*

This file is a reflection on what that single sentence costs and what it
buys, with a worked example at 400k triples. It is not a plan — plans
live in `docs/plans/`.

---

## What is actually persisted in 0.1.0?

**Nothing.** `src/store.rs:13–18` instantiates the per-thread store with
`Store::new()`, which is Oxigraph's pure in-memory back-end. The
triples live in heap memory owned by the thread that created them,
and they vanish when:

- the thread exits (TLS destructor drops the `Store`), or
- `rdf_clear()` is called on that thread, or
- the process exits.

`rdf_load_turtle / rdf_load_ntriples / rdf_load_rdfxml` parse text
*into the in-memory store*. They do not write to the SQLite database
file. The SQLite file on disk knows nothing about any triple that has
ever been inserted. The host application's `*.sqlite3` file and the
RDF graph are two parallel universes that share only a process.

Practical consequence: **restarting the process loses every triple**.
For Rails this means a full reboot, a deploy, or a Puma worker
recycle wipes the graph. Whoever asserted the triples has to assert
them again from a source of truth that *is* on disk (Postgres,
ActiveRecord rows, a Turtle file, …).

This is not a bug — 0.1.0 is explicit about being in-memory only, and
0.4.0 is reserved for the persistent RocksDB backend. But "no
persistence" is the kind of thing that quietly surprises people, so
spell it out.

---

## What "thread-local" really means under Rails

Rails 8's `ActiveRecord` connection pool checks a connection out per
request thread. Two requests handled concurrently by Puma threads
**A** and **B** see two different Oxigraph stores. Insert from a
controller running on thread **A**, then issue a SPARQL query from a
controller on thread **B**, and the query will return zero results.

Worse, there is no error — just absent data. The same `app.db` file is
behind both connections, but the RDF graph is not in `app.db`. There
is no shared canonical state for the graph at all in 0.1.0.

Puma's standard config compounds this. A typical `puma.rb` with
`workers 2; threads 5,5` produces 2 OS processes × 5 threads per
process = up to 10 independent Oxigraph stores in the running
application. Background jobs (Solid Queue, Sidekiq) add their own
worker threads with their own stores. The Rails console adds one
more. Every one of them sees a different graph.

The right mental model is not "the RDF store of this app" but "the
RDF store of *this thread of this process*, until that thread is
swept by the pool".

---

## 400k triples — sizing it

Take the user's hypothetical: 400 000 triples loaded into the store.

### Memory footprint

Oxigraph 0.4's in-memory `Store` uses several Arc-shared indexes
(SPO/POS/OSP, plus dictionaries that intern IRIs and literals).
Empirical numbers from the Oxigraph project itself put a "modest"
triple — short IRIs, simple literals — at roughly **150–250 bytes
amortised**, including the indexes. Skewed datasets with long literals
or many distinct strings push that higher.

For 400 000 triples, expect:

- **~60–100 MB resident per store**, in the comfortable middle of the
  range.
- **~150 MB+** if the data is literal-heavy (full names, descriptions,
  free-text strings).

That is *per thread*. With the Puma example above (2 workers × 5
threads), we are talking **600 MB – 1.5 GB of RDF working set** for
the same 400k triples, because every thread re-builds the same graph
from scratch. None of them share.

### Cold-start cost

Before any query can run on a thread, the store must be populated. If
the source of truth lives in the SQLite database (as ActiveRecord
rows) and the application is responsible for projecting it into the
graph, then each thread pays a *cold start* the first time it serves
a request that hits SPARQL.

Round-figure throughput for `rdf_load_ntriples` on commodity hardware
sits around **80 000–150 000 triples per second** for in-memory
Oxigraph, parser-limited. So a 400k graph takes **3–5 seconds** of
wall-time the first time it loads on a given thread. That latency is
spent at *first SPARQL request*, not at process boot — which is
exactly the worst time to spend it.

Puma will happily route the first SPARQL request on each of its 5
threads to a 3–5 s cold start. So the application's effective SLA
under load looks fine in steady state and miserable after a deploy.

### Steady-state query cost

Once warm, Oxigraph's in-memory store is fast for the shapes 0.1.0
exposes:

| Query shape                                | 400k store, warm |
|---|---|
| Indexed lookup by exact subject+predicate  | tens of microseconds |
| Predicate-bound scan with light filter     | low single-digit ms |
| Full triple-pattern scan `?s ?p ?o`        | tens of ms (returns 400k rows!) |
| 2–3 hop join with selective constraints    | low single-digit ms |
| Fan-out join with weak selectivity         | hundreds of ms, occasionally seconds |

The headline is: at 400k triples, **selective SPARQL is fast** —
sub-10ms for almost anything a Rails request would reasonably ask.
Pathological queries (cross products, unbounded `OPTIONAL`s, big
`UNION`s) can still saturate a CPU core for a noticeable fraction of
a second; that is true of every triple store and is on the query
author, not the engine.

### What the *function call* itself costs

There is a per-call overhead from going SQL → SQLite scalar function
→ Rust → Oxigraph for every SPARQL string. The SQL parse and the
SPARQL parse are both paid every call. For a tight 1 ms SPARQL query,
that overhead can be a meaningful fraction of total time. If a Rails
request does a handful of SPARQL hops, prefer one wider query over
ten narrow ones.

---

## The honest summary

Per the design as it stands in 0.1.0:

- Nothing is persisted. The graph is a per-thread heap structure.
- 400k triples cost roughly 60–100 MB per thread and load in 3–5 s.
- After warmup, queries are typically sub-10 ms; the engine is not
  the bottleneck for any sensible Rails-shaped workload at this size.
- The cost is paid once per thread for memory, once per thread for
  warmup, and forever for the fact that **threads do not see each
  other's writes**.

The thread-local choice is correct for the in-memory build because
`Store::new()` does not promise cross-thread safety in the way the
RocksDB store does, and because a Rails app can fully populate any
thread from authoritative storage on demand. The choice **stops being
correct the moment the application treats SPARQL as the source of
truth for cross-request state**, because at that point the model is
wrong: there is no single graph, there are N of them, and they
diverge under writes.

---

## Implications for what should follow 0.1.0

These are not commitments, just the shape of the design space that
this review opens up:

1. **Document the model loudly** in `README.md`'s Limitations
   section. The current `CHANGELOG.md` mentions it but the README
   surface — which is what users read — is where this needs to be.
2. **Process-wide store, behind a lock.** Replace
   `thread_local! { Store }` with `OnceLock<Arc<Store>>`. Oxigraph
   0.4's `Store` is `Send + Sync` and internally concurrent, so the
   Arc is enough — no extra mutex needed for the in-memory back-end.
   This collapses the N-thread memory blow-up and eliminates the
   inter-thread invisibility. It is a *non-breaking change at the SQL
   surface*. This is the highest-leverage follow-up; arguably it
   should have been the 0.1.0 design.
3. **Persistent backend (the existing 0.4.0 milestone).** Open the
   store from a file path so the SQLite file and the RDF graph can be
   restored together. With a shared process-wide store from (2), the
   path to RocksDB is much shorter.
4. **`rdf_sync(path)` / `rdf_load(path)` first.** Even before full
   RocksDB persistence, a `rdf_dump_ntriples` → file and `rdf_load`
   at boot gets most of the operational benefit (durable across
   restarts) without committing to a particular on-disk format.
5. **Bulk-load on parse, not row-by-row.** `rdf_load_ntriples`
   currently calls `store.insert(&quad)` in a loop. Oxigraph exposes
   a `bulk_loader()` that is materially faster on cold load — worth
   it for the 3–5 s startup tax above.

Items 1 and 2 together cost a few hours and remove the most
surprising behavior of the current design. They are the smallest move
that turns the thread-local choice from a footgun into a deliberate
in-memory mode.
