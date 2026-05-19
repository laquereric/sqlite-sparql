# sqlite-sparql

A [SQLite loadable extension](https://www.sqlite.org/loadext.html) that embeds
[Oxigraph](https://github.com/oxigraph/oxigraph) to provide **native RDF triple
storage and SPARQL querying** directly within SQLite — with no external server
required.

Built with [`sqlite-loadable-rs`](https://github.com/asg017/sqlite-loadable-rs),
the premier framework for writing SQLite extensions in Rust.

---

## Features

- **`rdf_insert(s, p, o)`** — Insert RDF triples using N-Triples term syntax
- **`rdf_delete(s, p, o)`** — Delete triples
- **`rdf_clear()`** — Reset the in-memory store
- **`rdf_count()`** — Count triples in the store
- **`rdf_load_turtle(text)`** — Bulk-load from Turtle format
- **`rdf_load_ntriples(text)`** — Bulk-load from N-Triples format
- **`rdf_load_rdfxml(text)`** — Bulk-load from RDF/XML format
- **`rdf_dump_ntriples()`** — Serialise all triples as N-Triples
- **`rdf_term_type(term)`** — Returns `"iri"`, `"blank"`, or `"literal"`
- **`rdf_term_value(term)`** — Extracts the plain string value from a term
- **`sparql_query(query)`** — Execute a SPARQL SELECT → JSON array
- **`sparql_ask(query)`** — Execute a SPARQL ASK → `0` or `1`
- **`sparql_construct(query)`** — Execute a SPARQL CONSTRUCT → N-Triples text
- **`rdf_triples` virtual table** — Read/write SQL view of the triple store

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
│   │  Thread-local    │              │
│   │  Oxigraph Store  │              │
│   │  (in-memory)     │              │
│   └──────────────────┘              │
└─────────────────────────────────────┘
```

The store is **per-thread** to match SQLite's connection model. Each SQLite
connection (which runs on one thread) gets its own isolated Oxigraph store.

---

## Roadmap

- [ ] Named graph support (4th column on `rdf_triples`)
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
