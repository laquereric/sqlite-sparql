# PLAN 0.1.0 — sqlite-sparql first green build

The crate currently advertises a full surface (rdf_* functions, sparql_*
functions, `rdf_triples` virtual table) in `lib.rs` and `README.md`, but
`cargo build` fails with 15 errors. 0.1.0 is the smallest scope that gets
**a green `cargo test` on macOS and Linux** and makes the documented
surface real.

Out of scope for 0.1.0 (tracked separately, see "Post-0.1.0" below):
named graphs, SPARQL UPDATE, persistent RocksDB backend, Rails gem
wrapper, SPARQL HTTP endpoint.

---

## Goal

`cargo test` passes. The release `.dylib` / `.so` loads in `sqlite3` and
the demo script in `examples/demo.sql` runs end-to-end.

---

## Phase A — Fix the build (sqlite-loadable 0.0.5 API drift)

`src/functions/sparql_query.rs` and `src/vtab/triples_vtab.rs` were
written against an older `sqlite-loadable` shape than what 0.0.5 actually
exposes. The compile errors fall into five buckets:

| Bucket | Where | Fix |
|---|---|---|
| Non-exhaustive `Subject` / `Term` match (E0004) | `sparql_query.rs` — term-to-NT formatting | Add arms for `Subject::Triple(_)` and `Term::Triple(_)`. Reject RDF-star terms with a clear error (0.1.0 is RDF 1.1 only) |
| `RdfParser::parse_read` not found (E0599) | `rdf_triple.rs` load_turtle / load_ntriples / load_rdfxml | Replace with the 0.4.x parser API — `RdfParser::from_format(...).for_reader(...)` returns an iterator of `Result<Quad>` |
| `VTab::update` not a trait member (E0407) | `triples_vtab.rs` | 0.0.5 splits read-only and writeable vtabs. Implement `VTabWriteable::update` instead of putting `update` on `VTab` |
| `VTabWriteable` trait bound (E0277) | `triples_vtab.rs` register | Switch registration to `define_virtual_table_writeable::<RdfTriplesTable>` (verify exact name via `cargo doc --open -p sqlite-loadable`) |
| `mismatched types` (E0308) | various | Fall out of the above three; re-run after they land |

### A.1 — sparql_query.rs term formatting

- Add `Subject::Triple(_) => return Err(SparqlError::RdfStarUnsupported)` and the same for `Term::Triple(_)`.
- Define `SparqlError::RdfStarUnsupported` in `error.rs` with a message that points users at issue tracker.

### A.2 — rdf_triple.rs bulk loaders

- For each of `rdf_load_turtle`, `rdf_load_ntriples`, `rdf_load_rdfxml`:
  - Build an `RdfParser` for the format.
  - Call `.for_reader(Cursor::new(text.as_bytes()))` to get the quad iterator.
  - Bulk-insert via `store.bulk_loader()` if available, else `store.insert(&quad)` in a loop.
  - Return the inserted count.

### A.3 — triples_vtab.rs writeable vtab

- Move the body of the current `update` from `impl VTab` into `impl VTabWriteable`.
- Match the 0.0.5 signature: argv layout is `[rowid_or_null, new_rowid_or_null, col0, col1, col2]`. Decide:
  - `rowid is NULL, new is non-null` → INSERT
  - `rowid is non-null, new is NULL` → DELETE
  - both non-null → UPDATE (delete old + insert new).
- Update `register` to call `define_virtual_table_writeable`.

### Exit criteria for Phase A

```
cargo build           # 0 errors, 0 warnings (or only docs warnings)
cargo build --release # same
```

---

## Phase B — Integration tests

`tests/integration_test.rs` exists but has not been audited against the
actual surface. 0.1.0 needs at least these tests, each loading the
release `.dylib` through `rusqlite`'s `loadable_extension`:

- `test_rdf_insert_count_delete_clear` — basic round-trip via the three scalar functions.
- `test_rdf_load_dump_ntriples` — load a small Turtle blob, dump as N-Triples, count matches.
- `test_sparql_select_returns_json` — insert one triple, run a SELECT, parse the JSON, assert shape.
- `test_sparql_ask` — true and false cases.
- `test_sparql_construct` — round-trip CONSTRUCT to N-Triples.
- `test_rdf_term_type` and `test_rdf_term_value` — iri / blank / literal / langString / typed literal.
- `test_vtab_read` — INSERT via scalar, SELECT via the virtual table.
- `test_vtab_write` — INSERT and DELETE via the virtual table.
- `test_thread_local_isolation` — open two SQLite connections on different threads, confirm each has its own store (this is the design choice in `store.rs`; lock it in with a test).

### Exit criteria for Phase B

```
cargo test            # all green on macOS arm64
cargo test --release  # same
```

---

## Phase C — Demo script + README sanity

- Run `sqlite3 :memory: < examples/demo.sql` against the release build. Fix any output drift.
- Re-walk the README "Quick Start" by hand, copy-paste verifiable.
- Confirm the macOS extension path (`target/release/libsqlite_sparql.dylib`) is what loads under `.load ./target/release/libsqlite_sparql` (SQLite appends `.dylib` itself).

### Exit criteria for Phase C

A user following `README.md` from a clean clone reaches a working
SPARQL query inside `sqlite3` without reading anything else.

---

## Phase D — Tag 0.1.0

- Bump nothing — `Cargo.toml` is already at `0.1.0`.
- Add `CHANGELOG.md` with the 0.1.0 entry: scalar functions, vtab, in-memory store, RDF 1.1 only.
- `git tag v0.1.0`.

---

## Post-0.1.0 (deferred, do **not** attempt during 0.1.0)

These are listed in CLAUDE.md and are real, but each is its own plan:

- **0.2.0 — Named graphs**: add a 4th `graph` column to `rdf_triples`, plumb `GraphName::NamedNode` through `store.rs`.
- **0.3.0 — SPARQL UPDATE**: `sparql_update(query)` backed by `Store::update`.
- **0.4.0 — Persistent store**: `Store::open(path)` via `rdf_open(path)` SQL function or extension argument; decide eviction semantics for thread-local store map.
- **0.5.0 — `sqlite-sparql-ruby` gem**: ships prebuilt `.dylib`/`.so`; `SqliteSparql.load(db)` mirrors `sqlite-vec`'s pattern; `HasRdfTriples` AR concern.
- **0.6.0 — SPARQL HTTP endpoint**: Rack middleware speaking SPARQL Protocol 1.1.

---

## Risks specific to 0.1.0

- **sqlite-loadable 0.0.5 is pre-1.0**. If the writeable-vtab story in 0.0.5 turns out to be incomplete on closer inspection, fall back to a **read-only** `rdf_triples` vtab for 0.1.0 and write through the scalar `rdf_insert` / `rdf_delete` functions only. Document the limitation in README.
- **oxigraph 0.4.x → 0.5.x**. Do not upgrade during 0.1.0. The Store API changed; that's a 0.x bump on our side too.
- **Thread-local store + Rails**. Rails 8's SQLite connection pool reuses threads; each pooled thread sees its own store. This is correct for the in-memory build but will surprise users. Call it out in README under "Limitations".
