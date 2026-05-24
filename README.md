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
- **`rdf_load_turtle(text)`** / **`rdf_load_turtle_to_graph(text, graph)`** —
  Bulk-load from Turtle format. The 2-arg form routes every parsed triple
  into the named graph `graph` (`NULL` → default graph, identical to the
  1-arg form).
- **`rdf_load_ntriples(text)`** / **`rdf_load_ntriples_to_graph(text, graph)`** —
  Bulk-load from N-Triples format, same graph routing convention.
- **`rdf_load_rdfxml(text)`** / **`rdf_load_rdfxml_to_graph(text, graph)`** —
  Bulk-load from RDF/XML format, same graph routing convention.
- **`rdf_dump_ntriples()`** — Serialise all triples as N-Triples
- **`rdf_term_type(term)`** — Returns `"iri"`, `"blank"`, or `"literal"`
- **`rdf_term_value(term)`** — Extracts the plain string value from a term
- **`sparql_query(query)`** — Execute a SPARQL SELECT → JSON array. SPARQL
  1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
  clauses route the query through Oxigraph unchanged.
- **`sparql_ask(query)`** — Execute a SPARQL ASK → `0` or `1`
- **`sparql_construct(query)`** — Execute a SPARQL CONSTRUCT → N-Triples text
- **`sparql_update(query)`** — Execute any SPARQL 1.1 UPDATE form
  (`INSERT DATA`, `DELETE DATA`, `INSERT/DELETE … WHERE`, `CLEAR`,
  `CREATE`, `DROP`). Returns the **signed net change** in store size:
  `+N` for inserts, `-N` for deletes, `inserts - deletes` for mixed
  modifies (so a balanced mixed UPDATE may return `0`).
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

### SPARQL UPDATE

For arbitrary SPARQL 1.1 UPDATE — anything beyond `INSERT DATA` /
`DELETE DATA` that the scalar surface and `rdf_insert_many` can
already express — use `sparql_update`:

```sql
SELECT sparql_update(
  'INSERT { ?p <http://example.org/derived_at> ?nowstr }
   WHERE  { ?p a <http://xmlns.com/foaf/0.1/Person>
            BIND(STR(NOW()) AS ?nowstr) }'
);
-- → +N  (one new triple per matching person)

SELECT sparql_update('CLEAR GRAPH <urn:graph:bhphoto>');
-- → -N  (count cleared)
```

Return value: signed net change in store size. Positive for
insert-only, negative for delete-only, `inserts - deletes` for mixed
operations (so a balanced mixed UPDATE can return `0` even though
both halves ran). Observe the store with `rdf_count` / `sparql_ask`
when you need to assert state rather than delta.

### Batched CONSTRUCT (since 0.8.0)

For fixpoint workloads (SHACL Rules, OWL 2 RL reasoning) that
issue many CONSTRUCTs per iteration, `rdf_construct_many` runs an
array of CONSTRUCT queries in one FFI crossing and returns a JSON
array of per-query N-Triples blobs:

```sql
SELECT rdf_construct_many(
  json('[
    "CONSTRUCT { ?p mm:tier mm:VIP }
       WHERE  { ?p mm:total_orders ?n . FILTER(?n > 100) }",
    "CONSTRUCT { ?p mm:availability \"in_stock\" }
       WHERE  { ?p mm:inventory ?n . FILTER(?n > 0) }"
  ]')
);
-- => '["<urn:p:1> <…> <…> .\\n…", "<urn:p:7> <…> <…> .\\n…"]'
```

Per-query attribution is preserved (the `i`-th element of the
returned array is the output of the `i`-th input query), so
consumers can attach `:derivedBy <rule_iri>` annotations rule by
rule before inserting. CONSTRUCT is read-only — the engine does
not insert results into the store; the caller decides where each
blob lands. Pre-flight: any parse error aborts the whole batch
with the prefix `SPARQL parse error (query index N):` before any
query evaluates.

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
| Quoted triple (RDF-star, since 0.7.0) | `<< <s> <p> <o> >>` | `<< <http://e/bob> <http://e/name> "Bob" >>` |

> **Caveat for `rdf_insert`/`rdf_delete`/the vtab:** subject and object
> positions take a *bare* IRI (no angle brackets) — `'http://e/alice'`,
> not `'<http://e/alice>'`. Angle brackets only appear *inside* a
> quoted-triple term (`'<< <http://e/a> <http://e/p> "x" >>'`). The
> dump and SPARQL-result outputs use full N-Triples encoding (with
> brackets).

---

## RDF-star / SPARQL-star

Quoted-triple terms (the RDF-star
[CG report](https://w3c-cg.github.io/rdf-star/cg-spec/2021-12-17.html))
round-trip through every read and write path since 0.7.0. The
substrate is Oxigraph 0.4, which already accepts Turtle-star /
N-Triples-star input and evaluates SPARQL-star — the SQL surface
just stopped throwing the terms away.

```sql
-- Load a Turtle-star body with annotation shorthand
SELECT rdf_load_turtle('
  @prefix : <http://example.org/> .
  :bob :name "Bob" {| :statedBy :alice ; :confidence "0.9" |} .
');
-- → 3 (one asserted triple + two annotation triples)

-- Insert a quoted triple as subject
SELECT rdf_insert(
  '<< <http://e/bob> <http://e/name> "Bob" >>',
  'http://e/statedBy',
  'http://e/alice'
);

-- Query it with SPARQL-star
SELECT sparql_query('
  PREFIX : <http://example.org/>
  SELECT ?val ?stater WHERE {
    :bob :name ?val {| :statedBy ?stater |} .
  }
');

-- Destructure a quoted-triple term in plain SQL
SELECT rdf_triple_subject('<< <http://e/a> <http://e/p> "x" >>');
-- → '<http://e/a>'
SELECT rdf_term_type('<< <a> <b> <c> >>');
-- → 'triple'
```

Surface delta from 0.6.x:

- **All write paths** (`rdf_insert`, `rdf_delete`, `rdf_insert_many`,
  `rdf_delete_many`, `rdf_triples` vtab `INSERT`) accept `<< s p o >>`
  in subject and object position. Predicate position stays
  IRI-only — RDF doesn't extend star to predicates.
- **All read paths** (`rdf_dump_ntriples`, `sparql_construct`,
  `sparql_query` JSON bindings, `rdf_triples` vtab `SELECT`) emit
  `<< s p o >>` for quoted-triple terms.
- **SPARQL-star** flows straight through to Oxigraph — annotation
  shorthand `{| |}`, explicit `<<>>` patterns, and the
  `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE`
  built-ins all work without any SQL-side wrapping.
- New helper scalars (since 0.7.0):
  - `rdf_term_type(term)` returns `"triple"` for a quoted triple.
  - `rdf_triple_subject(term)` / `rdf_triple_predicate(term)` /
    `rdf_triple_object(term)` extract the parts of a quoted triple
    in plain SQL. (Inside SPARQL, use the `SUBJECT` / `PREDICATE` /
    `OBJECT` built-ins.)
  - `rdf_term_value(term)` on a quoted triple raises an error with
    the prefix `rdf_term_value: triple terms have no scalar value`
    — quoted triples have three parts, not one scalar value.

Nesting (`<< << s p o >> p o >>`) round-trips through every path.

For background on why this matters (statement-about-statement
provenance, the Conformer pattern), see
`docs/research/StarExts.md`.

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
- **`LOAD <iri>` inside `sparql_update`** would make Oxigraph fetch
  the IRI over HTTP from inside the database. The default Oxigraph
  build has no HTTP support, so `LOAD` returns an evaluation error.
  If you build Oxigraph with HTTP enabled, sandbox the database
  process accordingly.

---

## Roadmap

- [x] Named graph support (4-arg `rdf_insert`/`rdf_delete`, hidden
      `graph` column on `rdf_triples`, SPARQL `GRAPH` / `FROM`
      routing) — landed in 0.3.0
- [x] Batched insert (`rdf_insert_many` / `rdf_delete_many`) — landed
      in 0.4.0
- [x] `sparql_update(query)` for SPARQL 1.1 Update — landed in 0.5.0
- [x] Graph-scoped bulk loading (`rdf_load_*_to_graph`) — landed in 0.6.0
- [x] RDF-star / SPARQL-star round-trip — landed in 0.7.0
- [x] Batched CONSTRUCT (`rdf_construct_many`) — landed in 0.8.0
- [ ] Ruby gem wrapper (`sqlite-sparql-ruby`) with pre-built binaries
- [ ] SPARQL Protocol HTTP endpoint middleware for Rails
- [ ] Persistent store via Oxigraph's RocksDB backend — *deferred,
      no consumer pressure; revive on first ask*

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
