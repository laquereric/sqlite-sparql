# PLAN 0.3.0 — named graphs

> Add a fourth `graph` argument to the triple-mutation scalars and a
> `graph` column to the `rdf_triples` virtual table. Route SPARQL
> queries to the right graph(s). Keep every existing 3-arg / 3-column
> signature working unchanged.

Driver: `CONSUMER_REQUIREMENT_MM.md` § "Requested extensions" — MM's
PLAN_0_29_1 Phase B.2 cutover (delete the legacy `Triple` AR model +
`ProductTripler` service) needs the extension to read and write named
graphs, specifically the `"bhphoto"` graph that the legacy ingest
pipeline emitted into.

Depends on 0.2.0 (shared process-wide store). Anything that talks
about "the graph" in this plan means the per-process Oxigraph store.

---

## Goal

`cargo test` passes a new set of named-graph tests. MM's submodule pin
can be bumped to this rev, the legacy `Triple` AR table can be dropped,
and the `bhphoto` graph round-trips through `rdf_insert(s,p,o,graph)`
and `sparql_query('… FROM <…> …')`.

---

## Phase A — 4-arg scalar functions

Every existing 3-arg signature stays valid. The 4-arg variants are
*additive* — they register as separate SQLite scalar functions with
the same name but `nArg = 4`. SQLite dispatches on arity.

- `rdf_insert(s, p, o, graph)` — `graph` is an IRI string (no `<>`
  wrapping; same shape as `s` for IRI subjects). `NULL` means default
  graph; an empty string is rejected.
- `rdf_delete(s, p, o, graph)` — same shape.
- `rdf_count(graph)` — optional 1-arg form. Counts within the named
  graph. `NULL` means default graph. Zero-arg form keeps its current
  meaning of "count across the default graph".
- `rdf_count_all()` — new zero-arg form that counts across **every**
  graph including the default. Distinct from `rdf_count()` to keep
  the existing zero-arg semantics stable.

### Implementation notes

- `store.rs` — add `insert_triple_in_graph(s, p, o, graph)` and
  `delete_triple_in_graph` parallel to the existing 3-arg helpers.
  Internal: `GraphName::NamedNode(NamedNode::new(graph)?)` when
  `graph` is `Some`, `GraphName::DefaultGraph` when `None`.
- `functions/rdf_triple.rs` — register the 4-arg variants via
  `define_scalar_function(..., 4, ...)`. The existing 3-arg
  registrations stay.
- Validate graph IRIs at the boundary: same `NamedNode::new` check
  as for subjects. Reject `_:blank` graphs with `InvalidArgument`
  — Oxigraph 0.4 supports blank-node graphs but MM does not need
  them and they complicate the URL-encoded API surface.

### Exit criteria for Phase A

```
cargo build              # 0 errors, 0 warnings
SELECT rdf_insert('http://e/s', 'http://e/p', 'http://e/o', 'urn:g:bhphoto');
SELECT rdf_count(NULL);      -- default graph
SELECT rdf_count('urn:g:bhphoto'); -- named graph
```

---

## Phase B — SPARQL routing

Oxigraph's SPARQL engine already implements `FROM <graph>`,
`FROM NAMED <graph>`, and `GRAPH <g> { ... }` in 1.1. The extension
just needs to pass the query through unmodified — no shimming.

- Sanity-test that `sparql_query('SELECT ?s WHERE { GRAPH <urn:g:bhphoto> { ?s a ?o } }')`
  returns rows from the `bhphoto` graph only.
- Sanity-test that a `FROM` clause restricts the default dataset.
- `sparql_ask` and `sparql_construct` work the same way — no
  per-function change.

### Exit criteria for Phase B

A SPARQL query with `GRAPH <…>` returns only rows from that graph; a
query without restricts to the default graph (current behaviour).

---

## Phase C — `rdf_triples` virtual table

Two new things:

- `graph` column appended (column 3). The DDL becomes
  `CREATE TABLE x(subject TEXT, predicate TEXT, object TEXT, graph TEXT)`.
- Reads expose `graph = NULL` for default-graph quads and the IRI
  string for named-graph quads.
- Writes: 4-column `INSERT` puts the quad in the specified graph; the
  old 3-column form is **still accepted via SQLite's column-default
  filling** and writes to the default graph. (SQLite virtual-table
  writes that omit a column pass `NULL`; the vtab `update` handler
  treats `NULL` graph as default.)

### Backward compatibility constraint

Existing `INSERT INTO triples VALUES (s, p, o)` calls must keep
working without `graph`. SQLite's xUpdate gives us all declared
columns in `argv`, so a missing `graph` arrives as `NULL` — the
existing test (`test_virtual_table`) keeps passing without
modification.

### Exit criteria for Phase C

```
INSERT INTO triples VALUES ('http://e/s','http://e/p','http://e/o', 'urn:g:bhphoto');
SELECT * FROM triples WHERE graph = 'urn:g:bhphoto';
```

returns the row, and the existing 3-column INSERT keeps working
unchanged.

---

## Phase D — tests

- `test_rdf_insert_4arg_named_graph` — insert into `urn:g:bhphoto`,
  `rdf_count(NULL) = 0`, `rdf_count('urn:g:bhphoto') = 1`,
  `rdf_count_all() = 1`.
- `test_sparql_query_graph_clause` — insert one triple into each of
  two graphs, query with `GRAPH <…>`, expect one row.
- `test_sparql_query_default_graph` — confirm that an unqualified
  query still returns only default-graph triples after named-graph
  triples exist alongside.
- `test_vtab_named_graph_round_trip` — insert via 4-column vtab
  INSERT, read with `WHERE graph = …`.
- `test_vtab_default_graph_compat` — keep the existing 3-column
  INSERT path green.
- `test_rdf_delete_named_graph` — write to `urn:g:a`, delete from
  `urn:g:a`, count goes to 0; verify the same triple written to the
  default graph is not affected.

All tests start with `rdf_clear()` (pattern established by 0.2.0).

### Exit criteria for Phase D

```
cargo test              # all green, count rises by ~6
cargo test --release    # same
```

---

## Phase E — docs

- `README.md` — extend the SQL function table to list the 4-arg
  variants and the `graph` column on `rdf_triples`. Add a small
  "Named graphs" section pointing at the new signatures.
- `CHANGELOG.md` — 0.3.0 entry.
- `CLAUDE.md` § "Completing the Implementation" — mark "Named Graph
  Support" closed.
- `CONSUMER_REQUIREMENT_MM.md` — graduate the named-graph section
  from "Requested" to "SQL surfaces MM consumes" and remove the
  `(toward 0.3.0)` tag.

---

## Phase F — tag 0.3.0

- Bump `Cargo.toml` and `VERSION` to `0.3.0`.
- `cargo test` green at the bump.
- `git tag v0.3.0` and push.
- Bump submodule pin in MM + open the MM-side PR per the acceptance
  signal in `CONSUMER_REQUIREMENT_MM.md`.

---

## Out of scope

- Blank-node graph names — Oxigraph supports them; MM does not need
  them; rejecting at the boundary keeps the API simple.
- Graph-level metadata (creator, timestamp, etc.) — that is a layer
  above RDF and belongs in the consuming app, not the engine.
- Per-graph access control. SPARQL has no concept of it; impose at
  the Rails layer.
- `sparql_update(query)` — that is 0.5.0 work, even though it would
  naturally express named-graph mutations. For now, MM mutates via
  `Storable` lifecycle hooks calling `rdf_insert`/`rdf_delete` with
  the 4-arg form.

---

## Risks

- **Oxigraph's named-graph SPARQL semantics under `FROM` vs default
  dataset.** SPARQL 1.1's dataset construction rules are subtle:
  without `FROM` the default dataset is the union of named graphs
  *only if the engine is so configured*. Oxigraph defaults to
  "default graph is the actual default graph, named graphs require
  explicit `FROM NAMED` or `GRAPH`". Confirm via Phase D's test
  `test_sparql_query_default_graph` — if it fails, document the
  observed behaviour in README and decide whether to expose a knob.
- **SQLite scalar-function arity overloading.** SQLite allows
  multiple registrations with the same name but different `nArg`.
  Confirm that `sqlite-loadable` 0.0.5 doesn't silently overwrite
  one with the other; if it does, fall back to `rdf_insert_g(s, p,
  o, graph)` as a separate name. The CONSUMER doc's specific signature
  `rdf_insert(s, p, o, graph)` is preferred, but a rename is
  acceptable if forced by the FFI.
- **Vtab column-default behaviour.** A 3-column INSERT into a
  4-column vtab passes `NULL` for the missing column on most SQLite
  versions, but the contract isn't airtight. Verify with the
  existing `test_virtual_table` after the column is added; if it
  breaks, we expose a named `triples_g` vtab and leave `rdf_triples`
  3-column for compatibility.
