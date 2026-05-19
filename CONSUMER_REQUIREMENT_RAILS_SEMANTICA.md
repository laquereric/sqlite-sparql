# Consumer requirement — `rails-semantica`

> *This file is written from `vendor/rails-semantica`'s perspective.
> It declares the parts of `sqlite-sparql` that `rails-semantica`
> calls into, so this crate's contributors know which surfaces are
> load-bearing for an active downstream consumer. Anything **not**
> listed here is internal to sqlite-sparql and can change freely.
> Anything listed here is **coordinated**: a rename, removal, or
> behavioural change requires a paired bump in
> `vendor/rails-semantica/`.*

`rails-semantica` is developed in parallel to this crate under
`../rails-semantica/`. It is the **first and currently only**
production consumer. See
`../rails-semantica/docs/plans/PLAN_0.1.0.md` for the consuming
gem's own roadmap.

## How `rails-semantica` loads this crate

The consumer runs inside a Rails 8 process. At boot, its
`Semantica::Loader` does the equivalent of:

```ruby
ar_connection.raw_connection.enable_load_extension(true)
ar_connection.raw_connection.load_extension(
  "../sqlite-sparql/target/release/libsqlite_sparql.dylib"  # macOS
  # or .so on Linux, .dll on Windows
)
ar_connection.raw_connection.enable_load_extension(false)
```

The extension is then exercised by SQL on the same connection.

## Pinned surface — coordinated changes only

### 1. Artifact location + filename

`rails-semantica` probes these paths, in order, relative to the
substrate repo root (one directory above `vendor/`):

```
vendor/sqlite-sparql/target/release/libsqlite_sparql.dylib   # macOS
vendor/sqlite-sparql/target/release/libsqlite_sparql.so      # Linux
vendor/sqlite-sparql/target/release/sqlite_sparql.dll        # Windows
```

The `MM_SQLITE_SPARQL_PATH` env var overrides all three. If you
rename the cargo package, change `crate-type`, or restructure the
target dir, `Semantica::Loader::DEFAULT_PATHS` must move in
lockstep.

### 2. Entrypoint symbol

SQLite derives the default entrypoint from the filename:
`libsqlite_sparql` → `sqlite3_sqlitesparql_init`. The consumer
calls `load_extension(path)` **without** an explicit entrypoint
argument and relies on this default. If you change the
`#[sqlite_entrypoint]` symbol name, the consumer's Loader must pass
an explicit second argument — coordinate the bump.

### 3. SQL functions the consumer calls

| Function | Used by consumer | Notes |
|---|---|---|
| `rdf_count()` | `Semantica::Loader#extension_loaded?` — **sentinel probe** | Used to decide skip-vs-load. Must be callable on a fresh connection and return an integer ≥ 0 without raising. |
| `rdf_load_ntriples(text)` | `Semantica::Sparql.execute("INSERT DATA { ... }")` | The consumer translates SPARQL `INSERT DATA` bodies straight to this function. Must accept N-Triples-formatted text and return the count loaded as an integer. |
| `rdf_delete(s, p, o)` | `Semantica::Sparql.execute("DELETE DATA { ... }")` and `Semantica::Storable#retract_predicate!` | Called once per triple; arguments are N-Triples-encoded term strings. Must return without raising on a missing triple. |
| `rdf_clear()` | `Semantica::Sparql.execute("CLEAR ALL"|"CLEAR DEFAULT")` and the spec suite's per-example reset | Resets the store. Must be safe to call repeatedly. |
| `sparql_query(query)` | `Semantica::Sparql.select(query)` | **SELECT** queries. Must return a JSON-encoded string parseable by Ruby's `JSON.parse` into an `Array` of `Hash` (variable name → N-Triples-encoded term string). Empty result set returns `"[]"` (or NULL — the consumer normalises both to `[]`). |
| `sparql_ask(query)` | `Semantica::Sparql.ask(query)` | **ASK** queries. Must return integer `0` or `1`. |
| `sparql_construct(query)` | `Semantica::Sparql.construct(query)` | **CONSTRUCT** queries. Must return N-Triples-formatted text. |

The `rdf_triples` virtual table is **not** in `rails-semantica`'s
0.1.0 surface. The consumer reaches the store only via the scalar
functions above. You may evolve the vtab freely; if you remove it,
update the README only.

### 4. Term encoding contract

The consumer hands the engine — and expects to receive back —
terms in N-Triples encoding:

- IRIs: `<http://example.org/foo>` (angle-bracketed)
- Blank nodes: `_:label`
- Plain literals: `"hello"`
- Language-tagged literals: `"hello"@en`
- Typed literals: `"42"^^<http://www.w3.org/2001/XMLSchema#integer>`

`Semantica::Storable::TermSerializer` produces this format; the
consumer's `Sparql.execute` round-trip parses results in this
format back into `DELETE DATA` payloads. Changing the term grammar
on either side breaks the loop.

**Engine-internal asymmetry the consumer accommodates** (do not
"fix" without coordination): `rdf_load_ntriples` accepts full
N-Triples (IRIs wrapped in `<...>`) because it routes through
Oxigraph's parser. `rdf_delete` accepts **bare** IRIs for the
subject and predicate (no angle brackets) because `store.rs`
constructs `NamedNode::new(s)` directly. The consumer strips
brackets before calling `rdf_delete` (see
`Semantica::Sparql#delete_each_triple` + `#unwrap_iri`). If you
unify the two paths — for example, making `rdf_delete` accept
either form — the consumer's unwrap step becomes a no-op (safe).
If you change `rdf_load_ntriples` to require bare IRIs instead,
the consumer breaks; coordinate.

### 5. SPARQL query results — JSON shape

For `sparql_query`, the consumer parses the returned string with
`JSON.parse` and expects an array of objects. Each object's keys
are the SPARQL variable names; values are the bound terms in
N-Triples encoding (literals quoted, IRIs in `<>`, etc.). Example:

```sparql
SELECT ?n WHERE { <urn:mm:alice> <http://xmlns.com/foaf/0.1/name> ?n }
```

must yield (for one binding to `"Alice"`):

```json
[{"n":"\"Alice\""}]
```

The leading/trailing quote characters around `Alice` matter — the
consumer feeds the bound value back into `DELETE DATA` and relies
on it being a valid N-Triples object term as-is.

### 6. Default-graph semantics

`rails-semantica` 0.1.0 operates entirely in the **default graph**.
Triples loaded via `INSERT DATA` are read back via `SELECT` /
`ASK` / `CONSTRUCT` without specifying a graph. The current
`store.rs` enforces this already (`GraphName::DefaultGraph`); when
you add named-graph support, **don't** change default-graph
queries to require an explicit `FROM` — that would silently break
the consumer.

### 7. Thread-local store model

`Semantica::Loader` calls `load_extension` once per
ActiveRecord-connection thread. The current `STORE` design
(thread-local Oxigraph instance, lazily initialised) matches this
exactly. If you move to a shared-process store or a per-database
file, the consumer's Loader idempotency check (probe `rdf_count()`
to decide skip-vs-load) must move in lockstep — otherwise loading
twice on the same store could either no-op or error depending on
the new model.

### 8. Failure mode — errors as SQLite errors, not panics

Every documented SQL function must surface user-input errors
(invalid SPARQL, malformed N-Triples, bad IRIs) as **SQLite error
strings**, not Rust panics. The consumer's
`Semantica::Sparql.*` methods catch `ActiveRecord::StatementInvalid`
and convert to refusal envelopes; an uncaught Rust panic would
crash the host process. The current code already routes through
`SparqlError` → `sqlite_loadable::Error::new_message` — keep that
path intact across refactors.

## Surfaces NOT in the pinned set

`rails-semantica` 0.1.0 does **not** depend on:

- `rdf_insert(s, p, o)` — the consumer uses `rdf_load_ntriples`
  instead. Keep it or rename it, your call.
- `rdf_load_turtle`, `rdf_load_rdfxml`, `rdf_dump_ntriples`,
  `rdf_term_type`, `rdf_term_value` — not on any consumer call
  path.
- The `rdf_triples` virtual table.
- Any specific behavior under named graphs, SPARQL UPDATE (beyond
  `INSERT DATA` / `DELETE DATA` / `CLEAR ALL` which the consumer
  rewrites into scalar function calls), RocksDB-backed
  persistence, or HTTP endpoints.

Evolving any of these is uncoordinated — go ahead.

## Versioning + coordination

- **No public crate version pin.** The consumer is path-vendored,
  not git-pinned or crates.io-pinned. The check is "does the
  built artifact at the expected path satisfy the pinned surface
  above." If yes, no coordination needed for the consumer's specs
  to pass; if no, the consumer's `bin/check` round-trip specs go
  red and that is the signal.
- **Coordinated bumps go in `vendor/rails-semantica/CHANGELOG.md`.**
  When this crate ships a surface-changing release, open a paired
  edit to the consumer that updates the call sites + bumps the
  consumer's CHANGELOG with a "requires sqlite-sparql ≥ X" note.
- **Specs are the contract.** The consumer's
  `spec/semantica/sparql_spec.rb` and
  `spec/semantica/storable_spec.rb` exercise every pinned function
  via round-trip tests tagged `:requires_extension`. Running
  `cd vendor/rails-semantica && bin/check` against a freshly-built
  `target/release/libsqlite_sparql.{dylib,so}` is the fastest
  signal that no consumer-visible regression has landed.

## See also

- `vendor/rails-semantica/docs/plans/PLAN_0.1.0.md` — consumer's
  own roadmap to a shippable 0.1.0.
- `vendor/rails-semantica/README.md` — consumer surface map.
- `vendor/rails-semantica/lib/semantica/loader.rb` —
  authoritative source for path probing + entrypoint expectations.
- `vendor/rails-semantica/lib/semantica/sparql.rb` —
  authoritative source for which SQL functions are called +
  expected return shapes.
- `vendor/rails-semantica/lib/semantica/storable.rb` —
  authoritative source for the term-encoding round-trip
  expectations.
