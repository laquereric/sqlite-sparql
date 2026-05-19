# Changelog

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
