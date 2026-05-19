# Changelog

## 0.2.0 — shared process-wide store

Replaces the per-thread Oxigraph store from 0.1.0 with a single
process-wide store wrapped in `OnceLock<Store>`. Every SQLite connection
on every thread now sees the same triple graph.

### Behaviour change

- A triple inserted on one SQLite connection is **visible** from every
  other connection in the same process, including connections on other
  threads. This is the headline fix for the "insert-on-thread-A-
  invisible-from-thread-B" footgun called out in
  `docs/reviews/REVIEW_0.1.0.md`.
- The SQL surface is unchanged. No function was renamed, added, or
  given a new signature in this release.
- `rdf_clear()` now empties the existing store in place (via
  `Store::clear`) rather than replacing it with a fresh instance. The
  observable behaviour is identical for callers (count → 0; subsequent
  inserts continue to work).
- Internal: `store::with_store_mut` was removed (it was always a
  misnomer — Oxigraph's `Store` mutates through `&self`). `with_store`
  takes its place at every call site. This is not a public API.

### Concurrency

Oxigraph 0.4's in-memory `Store` is internally concurrent — every
mutator takes `&self` and the storage layer uses `DashMap` plus
`RwLock` for synchronisation. The extension wraps the store in
`OnceLock` only for lazy initialisation; no additional `Mutex` or
`RwLock` is layered on top.

Downstream consumers like `rails-semantica` should be aware that
concurrent HTTP requests (Puma threads) can now interleave reads and
writes against the shared graph — which is the right correctness
story, but is a new concurrency surface compared to 0.1.0.

### Tests

- Dropped `test_thread_local_isolation` — it pinned the old, buggy
  invariant.
- Added `test_cross_thread_visibility` — proves the new invariant
  across threads.
- Added `test_shared_store_across_connections` — proves it across
  connections on the same thread.
- Added `serial_test` as a dev-dependency and marked every
  integration test `#[serial]`. The shared store means parallel tests
  would otherwise race; `cargo test` is now serialised at the
  integration-test layer only.

### Roadmap shift

PLAN_0.1.0 tentatively numbered "named graphs" as 0.2.0. That work
moves to **0.3.0** (`docs/plans/PLAN_0.3.0.md`); batched insert
(`rdf_insert_many`) was newly broken out as **0.4.0**
(`docs/plans/PLAN_0.4.0.md`). The MM consumer document
(`CONSUMER_REQUIREMENT_MM.md`) has been re-labelled to match.

## 0.1.0 — first green build

Initial release. SQLite loadable extension embedding the Oxigraph RDF/SPARQL
engine. The thread-local Oxigraph store is in-memory and resets when the
thread exits.

### SQL surface

- Scalar functions: `rdf_insert`, `rdf_delete`, `rdf_clear`, `rdf_count`,
  `rdf_load_turtle`, `rdf_load_ntriples`, `rdf_load_rdfxml`,
  `rdf_dump_ntriples`, `rdf_term_type`, `rdf_term_value`.
- SPARQL: `sparql_query` (SELECT → JSON), `sparql_ask` (ASK → 0/1),
  `sparql_construct` (CONSTRUCT → N-Triples).
- Virtual table: `rdf_triples` — read scans the default graph; INSERT
  writes through to the store. DELETE and UPDATE on the vtab are not
  supported in 0.1.x (use `rdf_delete(s,p,o)` or a SPARQL DELETE).

### Scope

- RDF 1.1 only — RDF-star quoted triples are rejected with a clear error.
- All triples live in the default graph; named graphs land in 0.2.0.
- In-memory store only; the persistent RocksDB backend lands in 0.4.0.

### Known limitations

- Thread-local store: Rails 8's SQLite connection pool reuses threads, so
  each pooled thread sees its own store. Acceptable for the in-memory
  build; revisit when the persistent backend lands.
