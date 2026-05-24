# CLAUDE.md — sqlite-sparql

This file guides Claude Code when working in this repository.

## Project Summary

`sqlite-sparql` is a **Rust loadable SQLite extension** that embeds the
[Oxigraph](https://github.com/oxigraph/oxigraph) RDF/SPARQL engine directly
inside SQLite. Once loaded, any SQLite connection gains native SQL functions
for inserting, querying, and serialising RDF triples, plus a read/write
virtual table (`rdf_triples`) backed by the Oxigraph in-memory store.

The primary consumer is a **Ruby on Rails application** using SQLite as its
database, where the extension is loaded via Rails 8's `extensions:` key in
`config/database.yml`.

---

## Repository Layout

```
sqlite-sparql/
├── Cargo.toml                  # Crate manifest — cdylib + rlib
├── CLAUDE.md                   # This file
├── README.md                   # User-facing documentation
├── src/
│   ├── lib.rs                  # Extension entry point (#[sqlite_entrypoint])
│   ├── error.rs                # SparqlError enum + conversions
│   ├── store.rs                # Thread-local Oxigraph Store wrapper
│   ├── functions/
│   │   ├── mod.rs
│   │   ├── rdf_triple.rs       # rdf_insert/delete/clear/count/load/dump
│   │   └── sparql_query.rs     # sparql_query/ask/construct
│   └── vtab/
│       ├── mod.rs
│       └── triples_vtab.rs     # rdf_triples virtual table (read/write)
├── tests/
│   └── integration_test.rs     # rusqlite-based integration tests
└── examples/
    └── demo.sql                # SQL demo script for the SQLite CLI
```

---

## Mac Development Setup

```bash
# 1. Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Install SQLite (for the CLI demo)
brew install sqlite

# 3. Build the extension (debug)
cargo build

# 4. Build the extension (release — smaller, faster)
cargo build --release

# 5. Run the test suite
cargo test

# 6. Try the SQL demo
sqlite3 :memory: < examples/demo.sql
# Or interactively:
sqlite3
sqlite> .load ./target/release/libsqlite_sparql
sqlite> SELECT rdf_insert('http://example.org/alice','http://www.w3.org/1999/02/22-rdf-syntax-ns#type','http://xmlns.com/foaf/0.1/Person');
sqlite> SELECT sparql_query('SELECT ?s WHERE { ?s a <http://xmlns.com/foaf/0.1/Person> }');
```

> **macOS note:** The compiled extension will be at
> `target/release/libsqlite_sparql.dylib`.  
> On Linux it will be `target/release/libsqlite_sparql.so`.

---

## Key Design Decisions

| Decision | Rationale |
|---|---|
| **Thread-local Oxigraph store** | Matches SQLite's one-connection-per-thread model; avoids cross-thread data races |
| **N-Triples encoding for term strings** | Standard, unambiguous, works as plain SQLite TEXT |
| **JSON output for SPARQL SELECT** | Easy to consume in Ruby via `JSON.parse`; works with SQLite's `json_each()` |
| **`cdylib` + `rlib` crate types** | `cdylib` for the loadable `.so/.dylib`; `rlib` enables Rust unit tests |
| **Oxigraph 0.4.x** | Stable API; 0.5.x changed the Store API significantly — upgrade carefully |

---

## Completing the Implementation

The following items are **not yet implemented** and should be finished next:

### 1. Fix API Incompatibilities
The `sqlite-loadable` crate (v0.0.5) has a slightly unstable API. Check these
areas when the first `cargo build` runs:

- `VTab::update()` signature — the `args` slice layout may differ from what
  `triples_vtab.rs` assumes. Consult
  `sqlite_loadable::table::UpdateOperation` if it exists.
- `define_virtual_table_writeable` — verify the function name in the installed
  version with `cargo doc --open`.
- `api::value_type` / `api::ValueType` — may be named differently; check
  `sqlite_loadable::api` docs.

### 2. Named Graph Support — DONE in 0.3.0
4-arg `rdf_insert`/`rdf_delete`, 1-arg `rdf_count`, `rdf_count_all`,
HIDDEN `graph` column on the `rdf_triples` vtab, and SPARQL routing
via standard `GRAPH`/`FROM`/`FROM NAMED` clauses. See
`docs/plans/PLAN_0.3.0.md`. Blank-node graphs are deliberately rejected
to keep the boundary narrow.

### 3. SPARQL UPDATE — DONE in 0.5.0
`sparql_update(query) → INTEGER` exposes `Store::update`. Returns the
signed net change in store size (Oxigraph 0.4 doesn't surface a
first-class affected-row count; the delta is the honest summary for
single-direction updates and `inserts - deletes` for mixed modifies).
See `docs/plans/PLAN_0.5.0.md`.

### 4. RDF-star / SPARQL-star — DONE in 0.7.0
Quoted-triple terms (`<< s p o >>`) round-trip through every read and
write path. The parser side (Turtle-star / N-Triples-star) and the
SPARQL-star evaluator were already provided by Oxigraph 0.4; 0.7.0
extends the SQL boundary to encode/decode the terms instead of
stubbing them. New scalars: `rdf_triple_subject`, `rdf_triple_predicate`,
`rdf_triple_object`. `rdf_term_type` returns `"triple"`. See
`docs/plans/PLAN_0.7.0.md` and `docs/research/StarExts.md`.

### 5. Batched CONSTRUCT — DONE in 0.8.0
`rdf_construct_many(queries_json TEXT) → TEXT` evaluates an array
of CONSTRUCT queries in one FFI crossing and returns a JSON array
of per-query N-Triples blobs. Per-query attribution preserved so
consumers can annotate per-rule before inserting. CONSTRUCT stays
read-only — the engine does not write results into the store.
Driver: RS PLAN_0.12.0's Shacl Rules materialise loop. See
`docs/plans/PLAN_0.8.0.md`.

### 6. Persistent Store (RocksDB backend) — DEFERRED
No consumer asks for persistence. If it lands, replace the in-memory
`Store::new()` in `store.rs` with `Store::open(path)` (Oxigraph's
RocksDB-backed persistent store) and expose the path via a
`rdf_open(path TEXT)` SQL function or an extension argument. Revive
on first consumer ask.

### 7. Rails Gem Wrapper (`sqlite-sparql-ruby`)
Create a companion Ruby gem that:
- Ships the pre-compiled `.dylib`/`.so` for common platforms
- Exposes a `SqliteSparql.load(db)` method (mirroring `sqlite-vec`'s pattern)
- Provides an ActiveRecord concern `HasRdfTriples` for model-level helpers

### 8. SPARQL Endpoint Middleware
Add a Rack/Rails middleware that exposes a `/sparql` HTTP endpoint accepting
SPARQL queries over the wire (SPARQL Protocol 1.1).

---

## SQL Function Reference

### Triple Management

```sql
SELECT rdf_insert(subject, predicate, object);   -- returns 1
SELECT rdf_delete(subject, predicate, object);   -- returns 1
SELECT rdf_clear();                              -- returns 1
SELECT rdf_count();                              -- returns INTEGER
SELECT rdf_load_turtle(turtle_text);             -- returns count loaded
SELECT rdf_load_turtle_to_graph(turtle_text, graph_iri);    -- 0.6.0; NULL graph = default
SELECT rdf_load_ntriples(ntriples_text);         -- returns count loaded
SELECT rdf_load_ntriples_to_graph(ntriples_text, graph_iri); -- 0.6.0; NULL graph = default
SELECT rdf_load_rdfxml(rdfxml_text);             -- returns count loaded
SELECT rdf_load_rdfxml_to_graph(rdfxml_text, graph_iri);    -- 0.6.0; NULL graph = default
SELECT rdf_dump_ntriples();                      -- returns TEXT
```

### Term Utilities

```sql
SELECT rdf_term_type('<http://example.org/>');   -- 'iri'
SELECT rdf_term_type('_:b0');                    -- 'blank'
SELECT rdf_term_type('"hello"');                 -- 'literal'
SELECT rdf_term_type('<< <a> <b> <c> >>');       -- 'triple'  (0.7.0)
SELECT rdf_term_value('<http://example.org/>');  -- 'http://example.org/'
SELECT rdf_term_value('"hello"@en');             -- 'hello'
-- rdf_term_value on a triple term raises a fixed-prefix error
-- ('rdf_term_value: triple terms have no scalar value; …') so
-- callers can prefix-match for refusal envelopes.

-- 0.7.0: destructure a quoted-triple term in plain SQL
SELECT rdf_triple_subject('<< <http://e/a> <http://e/p> "x" >>');   -- '<http://e/a>'
SELECT rdf_triple_predicate('<< <http://e/a> <http://e/p> "x" >>'); -- '<http://e/p>'
SELECT rdf_triple_object('<< <http://e/a> <http://e/p> "x" >>');    -- '"x"'
-- Inside SPARQL, prefer the SUBJECT / PREDICATE / OBJECT built-ins.
```

### SPARQL Querying

```sql
-- SELECT → JSON array of objects
SELECT sparql_query('SELECT ?s ?p ?o WHERE { ?s ?p ?o }');

-- ASK → 0 or 1
SELECT sparql_ask('ASK { <http://example.org/alice> ?p ?o }');

-- CONSTRUCT → N-Triples text
SELECT sparql_construct('CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }');

-- 0.8.0: batched CONSTRUCT → JSON array of per-query N-Triples blobs
SELECT rdf_construct_many(json('[
  "CONSTRUCT { ?s <http://e/q1> ?o } WHERE { ?s ?p ?o }",
  "CONSTRUCT { ?s <http://e/q2> ?o } WHERE { ?s ?p ?o }"
]'));
-- => '["<…> <q1> <…> .\n…", "<…> <q2> <…> .\n…"]'
-- Per-query attribution preserved (i-th element is the i-th query's output).
-- Pre-flight: any parse error aborts the batch with
-- "SPARQL parse error (query index N): …" before any query evaluates.
-- Non-CONSTRUCT queries error with "rdf_construct_many: query index N is not a CONSTRUCT".
```

### Virtual Table

```sql
CREATE VIRTUAL TABLE triples USING rdf_triples();

-- Read
SELECT * FROM triples;
SELECT subject FROM triples WHERE predicate = '<http://xmlns.com/foaf/0.1/name>';

-- Write
INSERT INTO triples VALUES (subject_text, predicate_text, object_text);
DELETE FROM triples WHERE subject = '...' AND predicate = '...' AND object = '...';
```

---

## Rails Integration

```yaml
# config/database.yml  (Rails 8+)
default: &default
  adapter: sqlite3
  extensions:
    - "<%= Rails.root.join('vendor/sqlite/libsqlite_sparql') %>"
```

```ruby
# app/models/concerns/has_rdf_triples.rb
module HasRdfTriples
  extend ActiveSupport::Concern

  included do
    after_create  :sync_to_rdf_store
    after_destroy :remove_from_rdf_store
  end

  def sparql(query)
    result = self.class.connection.select_value(
      "SELECT sparql_query(?)", nil, [[nil, query]]
    )
    JSON.parse(result)
  end

  private

  def sync_to_rdf_store
    # Override in model to define which triples to assert
  end

  def remove_from_rdf_store
    # Override in model to retract triples on destroy
  end
end
```

---

## Testing

```bash
# Unit + integration tests
cargo test

# With output
cargo test -- --nocapture

# Single test
cargo test test_sparql_select -- --nocapture
```

> **Footgun — release-mode tests don't rebuild the cdylib.** Integration
> tests load the compiled `.dylib` via the `SQLITE_SPARQL_CDYLIB` env
> var that `build.rs` sets to `target/{debug,release}/libsqlite_sparql.dylib`.
> The integration test crate depends on the *path*, not on the cdylib
> as a build artifact — so `cargo test --release` will happily reuse a
> stale release dylib from a previous build and produce ghost failures
> (e.g. tests asserting new behaviour against the old binary). Always
> run `cargo build --release` first, or invalidate `target/release/`,
> before `cargo test --release`.

---

## Dependencies

| Crate | Version | Purpose |
|---|---|---|
| `sqlite-loadable` | 0.0.5 | SQLite extension framework |
| `oxigraph` | 0.4 | Embedded RDF/SPARQL engine |
| `serde` / `serde_json` | 1 | JSON serialisation of SPARQL results |
| `thiserror` | 1 | Ergonomic error types |
| `rusqlite` (dev) | 0.32 | Integration test harness |
| `tempfile` (dev) | 3 | Temporary files in tests |
