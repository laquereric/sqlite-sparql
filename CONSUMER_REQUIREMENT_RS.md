# Consumer requirements — `rails-semantica`

This file records the surface
[`rails-semantica`](https://github.com/laquereric/rails-semantica)
(the Rails-ecosystem gem; "RS" hereafter) consumes from
`sqlite-sparql`. It exists so upstream changes can be checked
against a written consumer expectation — **drift** between this
file and the extension's actual behaviour signals work that needs
to land in both repos lockstep.

RS is the **direct** consumer of this extension — it loads the
compiled `.dylib`/`.so` into an ActiveRecord connection at boot and
exercises the SQL surface from Ruby. MM (the substrate) consumes
the extension only through RS; see
[`./CONSUMER_REQUIREMENT_MM.md`](./CONSUMER_REQUIREMENT_MM.md) for
the substrate-level expectations that ride on top of these.

- RS repo: <https://github.com/laquereric/rails-semantica>
- RS plan that pinned today's surface: `docs/plans/PLAN_0.1.0.md`
- RS plan asking for engine evolution: `docs/plans/PLAN_0.2.0.md`
  (Phase D — named graphs — is engine-gated here)
- Intermediate consumer downstream: MM (its
  `CONSUMER_REQUIREMENT_MM.md` covers the substrate-level surface)

## How RS loads the extension

RS does not bundle the compiled artifact. The Loader probes the
filesystem at AR-connection-init time:

```ruby
# Semantica::Loader probes (in order):
ENV["MM_SQLITE_SPARQL_PATH"]                                   # absolute override
"<repo-root>/vendor/sqlite-sparql/target/release/libsqlite_sparql.dylib"   # macOS
"<repo-root>/vendor/sqlite-sparql/target/release/libsqlite_sparql.so"     # Linux
"<repo-root>/vendor/sqlite-sparql/target/release/sqlite_sparql.dll"       # Windows
```

The Loader then calls:

```ruby
ar_connection.raw_connection.enable_load_extension(true)
ar_connection.raw_connection.load_extension(path)
ar_connection.raw_connection.enable_load_extension(false)
```

and probes `SELECT rdf_count()` to confirm the entry-point bound.
SQLite derives the entry-point symbol from the filename:
`libsqlite_sparql.dylib` → `sqlite3_sqlitesparql_init`. RS depends on
that default — no explicit entry-point arg is passed.

If the artifact filename, entry-point symbol, or crate name
changes upstream, RS's `Semantica::Loader::DEFAULT_PATHS` +
`extension_loaded?` sentinel must update lockstep.

## SQL surfaces RS consumes

Documented here in the order RS exercises them. Renames or
behaviour changes against these surfaces require a coordinated bump
in `rails-semantica`'s `Gemfile.lock` + a graduation note in this
file.

### Sentinel — `rdf_count()`

```sql
SELECT rdf_count();  -- => INTEGER ≥ 0
```

Used by `Semantica::Loader#extension_loaded?` to decide skip-vs-load
on a connection. Must:

- Be callable on a fresh connection immediately after
  `load_extension`.
- Return an integer ≥ 0 without raising.
- Return `0` on a fresh thread-local store (the Loader assumes a
  freshly-loaded store is empty).

If `rdf_count` is renamed, RS's `SENTINEL_QUERY` constant moves
with it.

### Triple management

| Function | RS call site | RS expectation |
|---|---|---|
| `rdf_load_ntriples(text TEXT) → INTEGER` | `Semantica::Sparql.execute("INSERT DATA { ... }")` (default-graph payload) | Accepts N-Triples-formatted body. Returns count loaded. IRIs **with** angle brackets; literals as `"..."`. |
| `rdf_load_ntriples_to_graph(text TEXT, graph TEXT) → INTEGER` (from 0.6.0) | `Semantica::Sparql.execute("INSERT DATA { GRAPH <iri> { ... } }")` | Same body grammar as the 1-arg form. `graph` is a bare IRI (no angle brackets), `NULL` for the default graph. Blank-node graph IRIs (`_:label`) are rejected with `blank-node graphs are not supported` — RS prefix-matches this for its refusal envelope. |
| `rdf_delete(subject TEXT, predicate TEXT, object TEXT) → 1` | `Semantica::Sparql.execute("DELETE DATA { ... }")` and `Semantica::Storable#retract_predicate!` | Called once per triple. **Subject + predicate** must be **bare IRIs without angle brackets** (see asymmetry note below); object retains its N-Triples form. Returns without raising when the triple is absent. |
| `rdf_delete(subject TEXT, predicate TEXT, object TEXT, graph TEXT) → 1` (from 0.3.0) | `Semantica::Sparql.execute("DELETE DATA { GRAPH <iri> { ... } }")` and the graph-scoped retract paths in Storable | Same subject/predicate asymmetry as the 3-arg form. `graph = NULL` is equivalent to the 3-arg form (default graph). Blank-node graphs rejected as for the loader. |
| `rdf_insert_many(json TEXT) → INTEGER` (from 0.4.0) | `Semantica::Sparql.bulk_insert(rows)` | JSON array of rows; each row is `[s, p, o]` or `[s, p, o, graph]`. Same N-Triples term grammar as the single-row `rdf_insert` (pinned by `test_insert_many_parser_parity_with_single`). Returns the post-dedup count of newly inserted quads. |
| `rdf_delete_many(json TEXT) → INTEGER` (from 0.4.0) | `Semantica::Sparql.bulk_delete(rows)` | Symmetric; rows not present in the store are silent no-ops and don't count. |
| `rdf_clear() → 1` | `Semantica::Sparql.execute("CLEAR ALL"|"CLEAR DEFAULT")` and spec-suite per-example reset | Resets the store. Safe to call repeatedly. |

RS does **not** consume `rdf_insert`, `rdf_load_turtle`,
`rdf_load_turtle_to_graph`, `rdf_load_rdfxml`,
`rdf_load_rdfxml_to_graph`, `rdf_dump_ntriples`, `rdf_term_type`, or
`rdf_term_value`. Renames / removals of any of those are uncoordinated
— go ahead.

#### Named-graph SPARQL query path

`sparql_query` / `sparql_ask` / `sparql_construct` accept arbitrary
SPARQL — including `GRAPH <iri> { … }` patterns and `FROM <iri>` /
`FROM NAMED <iri>` dataset clauses — and route them straight through
to Oxigraph. RS exercises this via the `graph:` kwarg on its facade,
which rewrites the query to inject a `GRAPH` wrapper before calling
the engine. Confirming fixtures live in the engine's
`tests/integration_test.rs`:

- `test_sparql_query_graph_clause` — pins that a `GRAPH <urn:g:bhphoto> { … }`
  query returns only that graph's triples.
- `test_sparql_query_default_dataset_isolates` — pins that an
  unqualified `?s ?p ?o` query returns only the default graph, not
  the union of every graph.

If either of those starts failing upstream, the gem-level facade's
graph-routing assumptions break too — coordinate.

### SPARQL querying

| Function | RS call site | RS expectation |
|---|---|---|
| `sparql_query(query TEXT) → TEXT` | `Semantica::Sparql.select(query)` | Returns a JSON-encoded string parseable by Ruby's `JSON.parse` into an `Array<Hash>`. Keys are SPARQL variable names. Values are bound terms in **N-Triples encoding** (IRIs in `<>`, literals quoted). Empty result set returns `"[]"` or NULL — RS normalises both to `[]`. |
| `sparql_ask(query TEXT) → INTEGER` | `Semantica::Sparql.ask(query)` | Returns `0` or `1`. RS coerces to `true`/`false`. |
| `sparql_construct(query TEXT) → TEXT` | `Semantica::Sparql.construct(query)` | Returns N-Triples-formatted text. RS passes through unchanged. |
| `sparql_update(query TEXT) → INTEGER` (from 0.5.0) | `Semantica::Sparql.execute(any_update)` | Runs any SPARQL 1.1 UPDATE form. Returns **signed net delta** in store size (`+N` insert / `-N` delete / `inserts - deletes` for mixed). Errors split into `SPARQL parse error: …` and `SPARQL evaluation error: …` prefixes; RS pattern-matches the prefix for its refusal envelopes. |

The leading/trailing quote/bracket characters in `sparql_query`'s
bound values **matter**. RS feeds those values back into
`DELETE DATA` payloads verbatim (after the bracket-strip step
below), so a switch to bare values would break the read-replace
loop inside `Semantica::Storable`.

### Term encoding contract

RS hands the engine — and expects to receive back — terms in
N-Triples encoding:

- IRIs: `<http://example.org/foo>` (angle-bracketed)
- Blank nodes: `_:label`
- Plain literals: `"hello"`
- Language-tagged literals: `"hello"@en`
- Typed literals: `"42"^^<http://www.w3.org/2001/XMLSchema#integer>`

`Semantica::Storable::TermSerializer` produces this format on
write; result-set parsing on read expects the same. Changing the
term grammar on either side breaks the loop.

### Engine-internal asymmetry RS accommodates

`rdf_load_ntriples` routes through Oxigraph's parser and accepts
full N-Triples (IRIs wrapped in `<...>`). `rdf_delete` calls
`NamedNode::new(s)` directly on the subject and predicate, which
expects **bare IRIs** (no angle brackets).

RS strips brackets before calling `rdf_delete` — see
`Semantica::Sparql#delete_each_triple` + `#unwrap_iri`. **Do not
"fix" this without coordination.** Concretely:

- If you unify the two paths so `rdf_delete` also accepts
  `<...>`-wrapped form, the consumer's strip step becomes a no-op
  (safe — no coordinated bump needed).
- If you change `rdf_load_ntriples` to require bare IRIs instead,
  the consumer breaks. Coordinate.

### Failure mode

Every documented function must surface user-input errors (invalid
SPARQL, malformed N-Triples, bad IRIs) as **SQLite error strings**
— not Rust panics. RS catches `ActiveRecord::StatementInvalid` and
converts to refusal envelopes (`{ ok: false, reason:, because: }`);
an uncaught Rust panic would crash the host Rails process. The
current code routes through `SparqlError` →
`sqlite_loadable::Error::new_message` — keep that path intact
across refactors.

## Behaviours RS does NOT depend on

Free to evolve without coordination:

- **The `rdf_triples` virtual table** — RS reaches the store only
  via the scalar functions above.
- **Internal Oxigraph version** — RS tolerates Oxigraph bumps as
  long as the SPARQL semantics RS exercises stay stable.
- **The thread-local-store layout** — RS only depends on
  `rdf_count()` being a valid sentinel for "was this connection
  initialised."
- **Internal sqlite-loadable API churn** — as long as the SQL
  surface above holds, RS doesn't care what's under it.
- **Persistence backend** (in-memory today; RocksDB someday) — RS
  is store-agnostic. If a future engine release defaults to
  per-process persistence or per-file persistence, that's
  observable to RS only as "store contents persist across
  process restarts" — which RS handles fine (the sentinel + Loader
  idempotency already cover this case).

## Drift signals

A drift between this file and the extension's behaviour is
detectable in these places:

- RS's `bin/check` — locates the engine artifact and runs
  `bundle exec rspec`. Round-trip specs (`:requires_extension`)
  fail when the SQL surface drifts.
- RS's `spec/semantica/sparql_spec.rb` round-trip layer — fails
  when `sparql_query` JSON shape, `sparql_ask` return values, or
  `sparql_construct` N-Triples output changes incompatibly.
- RS's `spec/semantica/storable_spec.rb` lifecycle integration —
  fails when `rdf_load_ntriples` / `rdf_delete` / `rdf_clear`
  semantics drift.

When drift is detected, the fix path is:

1. Open an upstream PR in `laquereric/sqlite-sparql` with the
   corrected behaviour + a new upstream spec.
2. Land it; record the new SHA.
3. In MM's substrate, bump the `vendor/sqlite-sparql` submodule
   pin to the new SHA. Re-run `vendor/rails-semantica/bin/check`
   against the freshly-built artifact.
4. If the consumer expectation changed, update this file.

Never fix drift by patching the extension from within RS or MM.
The boundary stays bright in both directions.

## Previously requested extensions — now landed

> **History.** This section previously listed five engine asks. The
> upstream side of every one is now live; the live contracts are in
> the "SQL surfaces RS consumes" section above. The historical
> notes are kept below as the paper trail for the milestone-spanning
> work — and for the RS-side acceptance signals that may still be
> open on the gem.

### 1. Named graph support — INSERT path — LANDED in 0.6.0

`docs/plans/PLAN_0.6.0.md`. `rdf_load_ntriples_to_graph(body, graph)`
ships (plus Turtle / RDF/XML siblings for surface symmetry). RS routes
`INSERT DATA { GRAPH <iri> { … } }` through the 2-arg form; the 1-arg
loader is unchanged for default-graph payloads. The alternative shape
(teaching the 1-arg loader to honour an enclosing `GRAPH { … }`
wrapper) was deliberately rejected — N-Triples grammar has no graph
syntax, so a separate scalar names the operation honestly. See the
plan for the full reasoning.

### 2. Named graph support — DELETE path — LANDED in 0.3.0

`rdf_delete(s, p, o, graph)` ships as a 4-arg overload — see the
"Triple management" table above for the live contract. SQLite's
scalar-arity model accommodates overloads despite the wording in the
original ask. Same subject/predicate bare-IRI asymmetry as the 3-arg
form.

### 3. Named graph support — SPARQL query path — LANDED in 0.3.0

No engine change was needed; Oxigraph 0.4 honours `GRAPH { … }` and
`FROM <iri>` / `FROM NAMED <iri>` patterns directly. The confirming
spec lives in `tests/integration_test.rs` as
`test_sparql_query_graph_clause` (graph-scoped query returns only
that graph's triples) and `test_sparql_query_default_dataset_isolates`
(unqualified `?s ?p ?o` returns only the default graph, not the union
of every graph). Both pinned at upstream and named in the live
"Named-graph SPARQL query path" subsection above.

### 4. Batched insert — `rdf_insert_many` — LANDED in 0.4.0

`rdf_insert_many(json) → INTEGER` and the symmetric
`rdf_delete_many(json)` ship. Each row is `[s, p, o]` or
`[s, p, o, graph]`; the term parser is shared with the single-row
`rdf_insert` (pinned by `test_insert_many_parser_parity_with_single`).
Return is the post-dedup count of newly inserted (or actually deleted)
quads. Live contract is in the "Triple management" table above.

### 5. SPARQL UPDATE — LANDED in 0.5.0

`sparql_update(query) → INTEGER` ships. The return is the **signed
net delta in store size**, not "count of affected triples" —
Oxigraph 0.4's `Store::update` doesn't expose an affected-row count,
and computing one for mixed `DELETE/INSERT` operations would require
re-evaluating the WHERE pattern. The delta is honest for
single-direction updates; mixed ops should be observed via
`rdf_count` / `sparql_ask` rather than the delta. Errors split into
`SPARQL parse error: …` and `SPARQL evaluation error: …` prefixes;
RS pattern-matches the prefix for refusal envelopes. Live contract
is in the "SPARQL querying" table above.

## Acceptance signals — RS-side adoption

Each engine landing opens a corresponding RS-side adoption task.
They are independent and can move in any order:

- **#1 + #2 + #3** (named graphs) — RS PLAN_0.2.0 Phase D opens.
  Drop the `:engine_unsupported` refusal envelopes from the
  `graph:` kwarg paths. Route `INSERT DATA { GRAPH <iri> { … } }`
  through `rdf_load_ntriples_to_graph`; route graph-scoped
  `DELETE DATA` through 4-arg `rdf_delete`. Add round-trip specs
  covering graph-scoped reads + writes.
- **#4** (batched insert) — RS PLAN_0.2.0 Phase E opens. Implement
  `Semantica::Sparql.bulk_insert(rows)` / `bulk_delete(rows)`
  against `rdf_insert_many` / `rdf_delete_many`. Storable lifecycle
  hooks adopt the bulk path; remove the `:engine_unsupported` stub
  from the bulk methods.
- **#5** (SPARQL UPDATE) — RS PLAN_0.3.0 opens. Route any
  UPDATE-not-DATA form through `sparql_update`. The existing
  `INSERT DATA` / `DELETE DATA` / `CLEAR ALL` special cases can be
  retained for return-value ergonomics or collapsed into one path
  that always calls `sparql_update`.

## Contact

For questions about RS's consumption pattern, see
[`rails-semantica`'s `docs/plans/PLAN_0.1.0.md`](https://github.com/laquereric/rails-semantica/blob/main/docs/plans/PLAN_0.1.0.md)
and [`PLAN_0.2.0.md`](https://github.com/laquereric/rails-semantica/blob/main/docs/plans/PLAN_0.2.0.md),
or open an issue on the RS repo.
