# Changelog

## 0.4.0 — batched insert / delete

Adds `rdf_insert_many(json)` and `rdf_delete_many(json)` for writing
many triples in a single FFI crossing, collapsing the SQL-parse +
function-dispatch overhead of N separate `rdf_insert` calls down to
one.

### New SQL surface

- `rdf_insert_many(json) → INTEGER` — single JSON-array argument.
  Each row is `[s, p, o]` (default graph) or `[s, p, o, graph]`
  (named graph; `null` means default). Uses Oxigraph's `bulk_loader`
  internally. Returns the count of *newly* inserted quads; duplicates
  collapse under RDF set semantics and don't count.
- `rdf_delete_many(json) → INTEGER` — mirror. Per-row removal; no-ops
  (rows not present in the store) don't count toward the return value.

### Behaviour

- Empty array `'[]'` returns `0`, no error.
- Malformed input — non-array JSON, row of wrong arity, non-string
  element, invalid IRI, blank-node graph — aborts the *whole* batch
  before any write touches the store. Error messages include the
  failing row index (e.g. `row 7: subject: …`).
- Term encoding matches the single-row `rdf_insert(s, p, o)` parser
  exactly. Pinned by `test_insert_many_parser_parity_with_single`.

### Internal

- `store::{build_quad, parse_named_or_blank, parse_term,
  parse_graph_name}` are now `pub(crate)` so the bulk module reuses
  the single-row parser. This keeps the two write paths bit-identical
  in their handling of the term grammar (the risk the plan called
  out).

### Tests

Seven new tests (27 + 1 ignored perf-smoke, up from 20):
`test_insert_many_3_arg_rows`,
`test_insert_many_mixed_arities`,
`test_insert_many_dedup_return_value`,
`test_insert_many_malformed_aborts_batch`,
`test_insert_many_empty_array`,
`test_insert_many_parser_parity_with_single`,
`test_delete_many_partial`,
`test_insert_many_perf_smoke` (release-only, `#[ignore]` — run with
`cargo test --release -- --ignored insert_many_perf_smoke`; 1000-row
batch under 100 ms).

## 0.3.0 — named graphs

Adds named-graph support across the full SQL surface. All existing
zero- and three-argument signatures keep their 0.2.0 behaviour;
named-graph variants ride alongside.

### New SQL surface

- `rdf_insert(s, p, o, graph)` — 4-arg form routes into a named graph.
  `graph = NULL` is the default graph (same as the 3-arg form).
  Blank-node graphs (`_:…`) are rejected with a clear error.
- `rdf_delete(s, p, o, graph)` — mirror of insert.
- `rdf_count(graph)` — 1-arg form counts quads in a named graph;
  `NULL` is the default graph (same as `rdf_count()`).
- `rdf_count_all()` — counts across every graph, default included.
- `rdf_triples` virtual table now has a HIDDEN `graph` column:
  - `SELECT *` still returns three columns
  - `INSERT INTO triples VALUES (s, p, o)` still works (default graph)
  - `INSERT INTO triples(subject, predicate, object, graph) VALUES (…)`
    writes to a named graph
  - `WHERE graph = 'urn:g:…'` / `WHERE graph IS NULL` filter on graph

### SPARQL routing

SPARQL 1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
clauses go straight through to Oxigraph — no extra plumbing needed.
The default dataset for an unqualified `?s ?p ?o` query remains the
default graph only; named-graph triples never leak in without an
explicit `FROM` or `GRAPH` clause (pinned by
`test_sparql_query_default_dataset_isolates`).

### Backward compatibility

Every 0.1.0 / 0.2.0 caller keeps working unchanged. The 3-arg forms,
zero-arg `rdf_count()`, and the 3-column `SELECT * FROM triples` /
`INSERT INTO triples VALUES (…)` shapes are unchanged in syntax and
semantics.

### Tests

Six new integration tests (20 total, up from 13):
`test_rdf_insert_4arg_named_graph`,
`test_rdf_delete_4arg_named_graph`,
`test_rdf_insert_4arg_rejects_blank_graph`,
`test_sparql_query_graph_clause`,
`test_sparql_query_default_dataset_isolates`,
`test_vtab_named_graph_round_trip`,
`test_vtab_default_graph_compat`.

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
