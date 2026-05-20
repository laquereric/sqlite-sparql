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
| `rdf_load_ntriples(text TEXT) → INTEGER` | `Semantica::Sparql.execute("INSERT DATA { ... }")` | Accepts N-Triples-formatted body. Returns count loaded. IRIs **with** angle brackets; literals as `"..."`. |
| `rdf_delete(subject TEXT, predicate TEXT, object TEXT) → 1` | `Semantica::Sparql.execute("DELETE DATA { ... }")` and `Semantica::Storable#retract_predicate!` | Called once per triple. **Subject + predicate** must be **bare IRIs without angle brackets** (see asymmetry note below); object retains its N-Triples form. Returns without raising when the triple is absent. |
| `rdf_clear() → 1` | `Semantica::Sparql.execute("CLEAR ALL"|"CLEAR DEFAULT")` and spec-suite per-example reset | Resets the store. Safe to call repeatedly. |

RS does **not** consume `rdf_insert`, `rdf_load_turtle`,
`rdf_load_rdfxml`, `rdf_dump_ntriples`, `rdf_term_type`, or
`rdf_term_value`. Renames / removals of any of those are
uncoordinated — go ahead.

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

## Requested extensions (toward engine v0.x)

RS's `docs/plans/PLAN_0.2.0.md` Phase D depends on the items
below. Until they land upstream, RS ships `:engine_unsupported`
refusal envelopes for the `graph:` kwarg paths.

### 1. Named graph support — INSERT path

Today `rdf_load_ntriples` parses an N-Triples body into the
**default graph** unconditionally (see `functions/rdf_triple.rs`
forcing `GraphName::DefaultGraph` per quad). RS's PLAN_0.2.0
Phase D needs the gem to be able to write to a named graph:

```sparql
INSERT DATA { GRAPH <urn:mm:graph:bhphoto> { <s> <p> <o> . } }
```

Either:

- Extend `rdf_load_ntriples` to detect an enclosing `GRAPH <iri>
  { ... }` and write quads (not triples) accordingly, **or**
- Expose a new scalar:
  `rdf_load_ntriples_to_graph(body TEXT, graph_iri TEXT) → INTEGER`
  — RS routes graph-tagged INSERT DATA there.

Either landing is acceptable; clarity is the only constraint.

### 2. Named graph support — DELETE path

`rdf_delete(s, p, o)` is graph-blind today (acts on the default
graph). RS needs:

- `rdf_delete_in_graph(s, p, o, graph_iri TEXT) → 1`, **or**
- A 4-arg `rdf_delete(s, p, o, graph_iri TEXT) → 1` overload —
  but SQLite scalar arity is fixed, so a separate function is
  probably cleaner.

### 3. Named graph support — SPARQL query path

`sparql_query` / `sparql_ask` / `sparql_construct` already accept
arbitrary SPARQL — including `GRAPH <iri> { ... }` patterns. RS's
PLAN_0.2.0 Phase D wires a `graph:` kwarg on the facade methods
that rewrites the query to inject the `GRAPH` wrapper. **No engine
change should be needed here** — RS just needs the existing
`store.update(query)` / `store.query(query)` paths to honour
`GRAPH` patterns correctly per SPARQL 1.1.

Confirming spec on the engine side: a query like
`SELECT ?s WHERE { GRAPH <urn:mm:graph:bhphoto> { ?s ?p ?o } }`
must return only triples written to that named graph; the same
pattern with a different graph IRI must return nothing.

### 4. Batched insert — `rdf_insert_many`

MM is asking the engine for an array-argument batched-insert path
via its own `./CONSUMER_REQUIREMENT_MM.md`. RS doesn't need to
duplicate the engine-side ask — but the gem-side facade
(`Semantica::Sparql.bulk_insert` / `bulk_delete`) that consumes it
is scoped in RS's
[`PLAN_0.2.0.md`](https://github.com/laquereric/rails-semantica/blob/main/docs/plans/PLAN_0.2.0.md)
Phase E.

When the engine ships `rdf_insert_many` (or whatever shape lands),
RS:

1. Implements `Semantica::Sparql.bulk_insert(rows)` /
   `bulk_delete(rows)` against the engine surface.
2. `Storable` lifecycle hooks adopt the bulk path automatically
   (runtime-probe; falls back to the per-call path if the engine
   surface isn't present).
3. Removes the `:engine_unsupported` stub from the bulk methods.

Coordination signal: when the engine ships, ping RS so PLAN_0.2.0
Phase E opens.

### 5. SPARQL UPDATE — LANDED in 0.5.0

The upstream side is complete as of `v0.5.0`. The contract differs
slightly from the originally-proposed wording:

- `sparql_update(query TEXT) → INTEGER` — ships. ✓
- The return is the **signed net delta in store size**, not "count
  of affected triples" — Oxigraph 0.4's `Store::update` doesn't
  expose an affected-row count, and computing one for mixed
  `DELETE/INSERT` operations would require re-evaluating the WHERE
  pattern. The delta is honest for single-direction updates; mixed
  ops should be observed via `rdf_count` / `sparql_ask` rather than
  the delta.
- Errors split into `SPARQL parse error: …` (Oxigraph's
  `EvaluationError::Parsing` — bad SPARQL syntax) and `SPARQL
  evaluation error: …` (everything else). RS pattern-matches the
  prefix for refusal envelopes.

RS-side adoption: RS PLAN_0.3.0 routes any UPDATE-not-DATA form
through `sparql_update`. The existing `INSERT DATA` / `DELETE DATA`
/ `CLEAR ALL` special cases can be retained for return-value
ergonomics (they translate naturally to the +N / -N delta) or
collapsed into one path that always calls `sparql_update`.

### Acceptance signal

When items #1 + #2 + (verified) #3 land — gating items for RS
PLAN_0.2.0 Phase D — RS:

1. Bumps `Gemfile.lock` to a new gem rev that opens Phase D.
2. Removes the `:engine_unsupported` stub from `Semantica::Sparql`.
3. Adds round-trip specs covering graph-scoped reads + writes.
4. Updates this file: items #1–#3 graduate from "Requested" into
   "SQL surfaces RS consumes."

#4 (rdf_insert_many) graduates RS PLAN_0.2.0 Phase E independently
of #1–#3; it can land first, last, or in parallel.

#5 (sparql_update) **upstream complete** in `v0.5.0`. RS-side work
is on RS's PLAN_0.3.0 (not yet written) — independent acceptance
signal from #1–#4.

## Contact

For questions about RS's consumption pattern, see
[`rails-semantica`'s `docs/plans/PLAN_0.1.0.md`](https://github.com/laquereric/rails-semantica/blob/main/docs/plans/PLAN_0.1.0.md)
and [`PLAN_0.2.0.md`](https://github.com/laquereric/rails-semantica/blob/main/docs/plans/PLAN_0.2.0.md),
or open an issue on the RS repo.
