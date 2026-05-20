# sqlite-sparql

A [SQLite loadable extension](https://www.sqlite.org/loadext.html) that embeds
[Oxigraph](https://github.com/oxigraph/oxigraph) to provide **native RDF triple
storage and SPARQL querying** directly within SQLite — with no external server
required.

Built with [`sqlite-loadable-rs`](https://github.com/asg017/sqlite-loadable-rs),
the premier framework for writing SQLite extensions in Rust.

---

## Features

- **`rdf_insert(s, p, o)`** / **`rdf_insert(s, p, o, graph)`** — Insert
  RDF triples using N-Triples term syntax. The 4-arg form routes the
  triple into a named graph; `graph = NULL` is the default graph.
- **`rdf_delete(s, p, o)`** / **`rdf_delete(s, p, o, graph)`** — Delete
  triples; same `graph` semantics as `rdf_insert`.
- **`rdf_insert_many(json)`** / **`rdf_delete_many(json)`** — Batched
  write of a JSON array of triples. Each row is `[s, p, o]` or
  `[s, p, o, graph]`. Returns the count actually inserted / deleted
  (RDF set semantics — duplicates and no-ops don't count).
- **`rdf_clear()`** — Empty the in-memory store
- **`rdf_count()`** / **`rdf_count(graph)`** — Count triples in the
  default graph (zero-arg) or in a named graph (`NULL` = default).
- **`rdf_count_all()`** — Count triples across every graph
- **`rdf_load_turtle(text)`** — Bulk-load from Turtle format
- **`rdf_load_ntriples(text)`** — Bulk-load from N-Triples format
- **`rdf_load_rdfxml(text)`** — Bulk-load from RDF/XML format
- **`rdf_dump_ntriples()`** — Serialise all triples as N-Triples
- **`rdf_term_type(term)`** — Returns `"iri"`, `"blank"`, or `"literal"`
- **`rdf_term_value(term)`** — Extracts the plain string value from a term
- **`sparql_query(query)`** — Execute a SPARQL SELECT → JSON array. SPARQL
  1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
  clauses route the query through Oxigraph unchanged.
- **`sparql_ask(query)`** — Execute a SPARQL ASK → `0` or `1`
- **`sparql_construct(query)`** — Execute a SPARQL CONSTRUCT → N-Triples text
- **`rdf_triples` virtual table** — Read/write SQL view of the triple
  store. Columns: `subject`, `predicate`, `object`, plus a HIDDEN `graph`
  column (default graph = `NULL`). `SELECT *` and the 3-column `INSERT
  VALUES` form keep the 0.2.0 shape; name the `graph` column explicitly
  to read or write named graphs.

---

## Quick Start

### Build

```bash
# macOS
cargo build --release
# Extension: target/release/libsqlite_sparql.dylib

# Linux
cargo build --release
# Extension: target/release/libsqlite_sparql.so
```

### SQLite CLI

```sql
-- Load the extension
.load ./target/release/libsqlite_sparql

-- Insert some triples (N-Triples term syntax)
SELECT rdf_insert(
  'http://example.org/alice',
  'http://www.w3.org/1999/02/22-rdf-syntax-ns#type',
  'http://xmlns.com/foaf/0.1/Person'
);
SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/name',
  '"Alice"'
);

-- Count triples
SELECT rdf_count();  -- 2

-- SPARQL SELECT → JSON
SELECT sparql_query(
  'SELECT ?name WHERE { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> ?name }'
);
-- [{"name":"\"Alice\""}]

-- SPARQL ASK
SELECT sparql_ask('ASK { <http://example.org/alice> ?p ?o }');  -- 1

-- Virtual table
CREATE VIRTUAL TABLE triples USING rdf_triples();
SELECT * FROM triples;
```

### Named graphs

```sql
-- 4-arg form routes into a named graph
SELECT rdf_insert(
  'http://example.org/alice',
  'http://xmlns.com/foaf/0.1/name',
  '"Alice"',
  'urn:graph:bhphoto'
);

-- Count by graph
SELECT rdf_count();                       -- default graph only
SELECT rdf_count('urn:graph:bhphoto');    -- named graph
SELECT rdf_count_all();                   -- every graph

-- SPARQL routing via standard GRAPH / FROM clauses
SELECT sparql_query(
  'SELECT ?s WHERE { GRAPH <urn:graph:bhphoto> { ?s ?p ?o } }'
);

-- Virtual table: name the graph column to read or write it
INSERT INTO triples(subject, predicate, object, graph)
VALUES ('http://example.org/x', 'http://example.org/p', '"v"', 'urn:graph:bhphoto');

SELECT subject FROM triples WHERE graph = 'urn:graph:bhphoto';
```

### Batched writes

For thousands of triples in one shot, `rdf_insert_many` takes a single
JSON-array argument and loops on the Rust side via Oxigraph's bulk
loader — materially faster than N separate `rdf_insert` calls because
the FFI crossing and SQL parse happen once instead of N times.

```sql
SELECT rdf_insert_many('[
  ["http://example.org/alice", "http://xmlns.com/foaf/0.1/name", "\"Alice\""],
  ["http://example.org/bob",   "http://xmlns.com/foaf/0.1/name", "\"Bob\"",   "urn:graph:bhphoto"],
  ["http://example.org/carol", "http://xmlns.com/foaf/0.1/name", "\"Carol\"", null]
]');
-- → 3 (count of newly-inserted triples; duplicates and no-ops don't count)

SELECT rdf_delete_many('[
  ["http://example.org/alice", "http://xmlns.com/foaf/0.1/name", "\"Alice\""]
]');
-- → 1
```

A malformed row (wrong arity, non-string element, invalid IRI) aborts
the whole batch with a row-indexed error message; nothing is written.

### Bulk Load (Turtle)

```sql
SELECT rdf_load_turtle('
  @prefix foaf: <http://xmlns.com/foaf/0.1/> .
  @prefix ex:   <http://example.org/> .

  ex:bob   a foaf:Person ; foaf:name "Bob" .
  ex:carol a foaf:Person ; foaf:name "Carol" .
');
SELECT rdf_count();  -- 4
```

---

## Rails Integration (Rails 8+)

```yaml
# config/database.yml
default: &default
  adapter: sqlite3
  extensions:
    - "<%= Rails.root.join('vendor/sqlite/libsqlite_sparql') %>"
```

```ruby
# In a Rails model or service object
class KnowledgeGraph
  def self.insert(subject:, predicate:, object:)
    ActiveRecord::Base.connection.execute(
      "SELECT rdf_insert(?, ?, ?)", subject, predicate, object
    )
  end

  def self.query(sparql)
    json = ActiveRecord::Base.connection.select_value(
      "SELECT sparql_query(?)", sparql
    )
    JSON.parse(json)
  end
end
```

---

## N-Triples Term Syntax

All subject, predicate, and object arguments use N-Triples encoding:

| RDF Term | Syntax | Example |
|---|---|---|
| IRI | `<iri>` | `<http://example.org/alice>` |
| Blank node | `_:id` | `_:b0` |
| Plain literal | `"value"` | `"Hello"` |
| Language literal | `"value"@lang` | `"Bonjour"@fr` |
| Typed literal | `"value"^^<datatype>` | `"42"^^<http://www.w3.org/2001/XMLSchema#integer>` |

---

## Architecture

```
SQLite connection
      │
      │  .load libsqlite_sparql
      ▼
┌─────────────────────────────────────┐
│         sqlite-sparql extension     │
│                                     │
│  SQL functions        Virtual table │
│  ─────────────        ──────────── │
│  rdf_insert()         rdf_triples  │
│  rdf_delete()                       │
│  sparql_query()                     │
│  sparql_ask()                       │
│  sparql_construct()                 │
│             │                       │
│             ▼                       │
│   ┌──────────────────┐              │
│   │  Process-wide    │              │
│   │  Oxigraph Store  │              │
│   │  (in-memory)     │              │
│   └──────────────────┘              │
└─────────────────────────────────────┘
```

There is **one Oxigraph store per process**. Every SQLite connection on
every thread sees the same triple graph. Oxigraph 0.4's in-memory store
is internally concurrent (every mutator takes `&self`); the extension
wraps it in `OnceLock` only for lazy initialisation.

### Limitations

- **No persistence.** The store is purely in-memory — process restart
  drops every triple. The persistent RocksDB backend lands in a later
  release; until then, populate the store from a source of truth at
  boot or first access.
- **Blank-node graphs are rejected.** Oxigraph supports them; we keep
  the boundary narrow. Use IRI-named graphs.
- **RDF 1.1 only.** RDF-star quoted triples are rejected with a clear
  error.

---

## Roadmap

- [x] Named graph support (4-arg `rdf_insert`/`rdf_delete`, hidden
      `graph` column on `rdf_triples`, SPARQL `GRAPH` / `FROM`
      routing) — landed in 0.3.0
- [x] Batched insert (`rdf_insert_many` / `rdf_delete_many`) — landed
      in 0.4.0
- [ ] `sparql_update(query)` for SPARQL 1.1 Update
- [ ] Persistent store via Oxigraph's RocksDB backend
- [ ] `rdf_open(path)` to attach a persistent store
- [ ] Ruby gem wrapper (`sqlite-sparql-ruby`) with pre-built binaries
- [ ] SPARQL Protocol HTTP endpoint middleware for Rails

---

## Development

See [CLAUDE.md](CLAUDE.md) for detailed guidance on completing the
implementation with Claude Code.

```bash
cargo build          # debug build
cargo build --release  # release build
cargo test           # run tests
cargo doc --open     # browse API docs
```

---

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT License](LICENSE-MIT) at your option.
