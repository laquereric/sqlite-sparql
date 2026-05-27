# sqlite-sparql (Ruby)

A Ruby loader and ergonomic wrapper for the [sqlite-sparql](https://github.com/laquereric/sqlite-sparql)
SQLite extension — embedded RDF triple storage, SPARQL 1.1
(SELECT / ASK / CONSTRUCT / UPDATE), OWL 2 RL reasoning + inconsistency
detection, SHACL Core validation, and DRed-style incremental
over-deletion, all callable from any SQLite connection.

The gem mirrors [`sqlite-vec`](https://github.com/asg017/sqlite-vec)'s
`load(db)` pattern. It vendors the compiled cdylib and exposes a small
Ruby surface; raw SQL still works fine.

## Status

**0.14.0 — Ruby gem wrapper landed.** The gem currently ships only
the host-platform binary (built locally from the engine's
`cargo build --release` output). Cross-platform binary distribution
via GitHub Releases is a follow-on plan. Not yet published to RubyGems —
publishing waits on cross-platform binaries so consumers don't need a
Rust toolchain.

For now, install from source:

```bash
cd ruby
bundle install
rake native       # cargo build --release + vendor host binary
rake build        # build the .gem
gem install ./sqlite-sparql-0.14.0.gem
```

## Quick start

```ruby
require "sqlite3"
require "sqlite_sparql"

db = SQLite3::Database.new(":memory:")
SqliteSparql.load(db)

db.execute("SELECT rdf_insert(?, ?, ?)",
           ["http://example.org/alice",
            "http://xmlns.com/foaf/0.1/name",
            "\"Alice\""])

db.get_first_value("SELECT rdf_count()")
# => 1

db.get_first_value("SELECT sparql_query(?)",
                   ["SELECT ?o WHERE { <http://example.org/alice> ?p ?o }"])
# => '[{"o": {"type":"literal","value":"Alice"}}]'
```

## Ergonomic wrapper — `SqliteSparql::Store`

For the same operations without writing SQL strings:

```ruby
store = SqliteSparql::Store.new(SQLite3::Database.new(":memory:"))

store.insert("<urn:alice>", "<urn:knows>", "<urn:bob>")
store.delete("<urn:alice>", "<urn:knows>", "<urn:bob>")
store.count            # => Integer
store.clear            # => true

# Named graphs (since engine 0.3.0)
store.insert("<urn:a>", "<urn:p>", "<urn:b>", graph: "urn:g:catalogue")
store.count(graph: "urn:g:catalogue")

# Batched insert / delete (since engine 0.4.0)
store.insert_many([
  ["<urn:a>", "<urn:p>", "<urn:x>"],
  ["<urn:a>", "<urn:p>", "<urn:y>"],
])

# SPARQL — SELECT returns parsed JSON, ASK returns boolean
store.sparql("SELECT ?s WHERE { ?s ?p ?o }")     # => Array<Hash>
store.ask("ASK { <urn:alice> ?p ?o }")           # => Boolean
store.construct("CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }")  # => N-Triples
store.update("DELETE WHERE { <urn:alice> ?p ?o }")  # => Integer

# Bulk loading (since engine 0.6.0)
store.load_turtle(turtle_text)
store.load_turtle(turtle_text, graph: "urn:g:catalogue")

# OWL 2 RL reasoning (since engine 0.9.0; full coverage 0.10.0)
store.materialise(
  inferred: "urn:g:inferred",
  options: { "provenance" => true, "track_dependencies" => true }
)

# OWL 2 RL inconsistency detection (since engine 0.13.0)
store.consistent?(inferred: "urn:g:inferred")           # => Boolean
store.consistency_violations(inferred: "urn:g:inferred") # => Array<Hash>

# Native SHACL Core validation (since engine 0.11.0)
store.shacl_validate(
  shapes: "urn:g:shapes",
  report: "urn:g:report",
  data:   "urn:g:data",
)

# DRed incremental over-deletion (since engine 0.12.0)
store.dred_overdelete(
  inferred: "urn:g:inferred",
  retracted_premises: [["<urn:B>", "<rdfs:subClassOf>", "<urn:C>"]],
)
```

## Rails: `SqliteSparql::HasRdfTriples` concern

For Rails models that mirror domain rows as RDF triples:

```ruby
# config/database.yml — load the extension via Rails 8's `extensions:` key
# OR via SqliteSparql.load(db) on the AR connection at boot. The gem's
# concern auto-loads on first use of `rdf_store`.

require "sqlite_sparql/has_rdf_triples"

class Knowledge < ApplicationRecord
  include SqliteSparql::HasRdfTriples

  def sync_to_rdf_store
    rdf_store.insert(subject_iri, predicate_iri, object_iri)
  end

  def remove_from_rdf_store
    rdf_store.delete(subject_iri, predicate_iri, object_iri)
  end
end

# Lifecycle hooks fire on create/destroy
Knowledge.create!(
  subject_iri:   "<urn:alice>",
  predicate_iri: "<urn:knows>",
  object_iri:    "<urn:bob>",
)

# Class-level delegators for query/reason/validate
Knowledge.sparql("SELECT ?s WHERE { ?s a <urn:Person> }")
Knowledge.materialise(inferred: "urn:g:inferred")
Knowledge.consistent?(inferred: "urn:g:inferred")
Knowledge.shacl_validate(shapes: "urn:g:shapes", report: "urn:g:report")
```

The concern is `require`-on-demand so non-Rails consumers don't pull
ActiveRecord. The concern depends on the `sqlite3` adapter
(`connection.raw_connection` returns the `SQLite3::Database`).

## Loader internals — the filename gotcha

The Ruby `sqlite3` gem 2.x's `db.load_extension(path)` no longer accepts
an explicit entrypoint argument; it relies on SQLite's filename-based
auto-derivation. SQLite computes the entrypoint as
`sqlite3_<basename>_init` where `<basename>` is the cdylib filename
minus `lib` prefix and the file extension.

The engine's entrypoint is `sqlite3_sqlitesparql_init` (no underscore).
The vendored cdylib is therefore named `libsqlitesparql.{dylib,so,dll}`
(no underscore), not `libsqlite_sparql.*` (which is what cargo
produces). `rake native` does the rename.

For development workflows that point at the cargo build output via
`ENV["SQLITE_SPARQL_CDYLIB"]`, the loader has a dev-rewrap fallback
that hardlinks (or copies) `target/release/libsqlite_sparql.{ext}` to
a temp `libsqlitesparql.{ext}` path so auto-derivation finds the
entrypoint. Memoised per process.

## Testing

```bash
bundle install
cargo build --release       # in the engine root, one directory up
bundle exec rake test
```

Four test files: `test_loader.rb`, `test_store.rb`,
`test_has_rdf_triples.rb`, `test_version.rb` — 24 runs, 44 assertions.

## License

MIT or Apache-2.0, same as the engine.
