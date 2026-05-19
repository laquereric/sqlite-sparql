# Consumer requirements — MagenticMarket substrate

This file records the surface [MagenticMarket](https://github.com/laquereric/magentic-market-ai)
(the substrate; "MM" hereafter) consumes from `sqlite-sparql`. It exists so
upstream changes can be checked against a written consumer expectation —
**drift** between this file and the extension's actual behaviour signals
work that needs to land in both repos lockstep.

MM consumes `sqlite-sparql` **indirectly**, through the
[`rails-semantica`](https://github.com/laquereric/rails-semantica) gem. So
in practice, drift on this extension's surface tends to surface as failing
specs in `rails-semantica` first, and only after that as failing specs in
MM. Both layers care about this file.

- MM repo: <https://github.com/laquereric/magentic-market-ai>
- MM plan that introduced the dependency: `docs/plans/PLAN_0_29_1.md`
- Intermediate consumer: `rails-semantica` (its
  `CONSUMER_REQUIREMENT_MM.md` covers the gem-level surface)

## How MM gets the extension

MM does not ship the compiled `.dylib` / `.so`. Operators build it from a
pinned rev of this repo:

```bash
# From MM's repo root (sqlite-sparql checked out as a submodule under vendor/)
cd vendor/sqlite-sparql
cargo build --release
# Extension at: target/release/libsqlite_sparql.{dylib,so}

# MM's config/database.yml reads MM_SQLITE_SPARQL_PATH (operator-set):
export MM_SQLITE_SPARQL_PATH="$PWD/target/release/libsqlite_sparql.dylib"
```

MM's `vendor/sqlite-sparql/` submodule pin is the rev of record. The pin
is checked in CI against the rev `rails-semantica`'s `Gemfile.lock` was
tested against, so a single bump moves both layers lockstep.

## SQL surfaces MM (indirectly) consumes

MM exercises these via `Semantica::Sparql` and `Semantica::Storable`. If
the upstream renames or removes any of them, the consuming gem breaks +
MM breaks downstream.

### Triple management

- `rdf_insert(subject TEXT, predicate TEXT, object TEXT) → 1` — idempotent;
  re-inserting the same triple is a no-op.
- `rdf_insert(subject TEXT, predicate TEXT, object TEXT, graph TEXT) → 1`
  (from 0.3.0) — same shape, routes into a named graph. `graph = NULL`
  is the default graph.
- `rdf_delete(subject TEXT, predicate TEXT, object TEXT[, graph TEXT]) → 1`
  — symmetric.
- `rdf_count() → INTEGER` — default-graph count, used by MM's
  `bin/mm-smoke` semantica step.
- `rdf_count(graph TEXT) → INTEGER` (from 0.3.0) — count within a named
  graph; `NULL` is the default graph.
- `rdf_count_all() → INTEGER` (from 0.3.0) — count across every graph.

### SPARQL querying

- `sparql_query(query TEXT) → TEXT` — JSON array of binding objects.
  `Semantica::Sparql.select` parses the JSON; MM observes the parsed
  envelope, not the JSON shape directly. **But:** the JSON shape MM
  ultimately sees is `[{ "var": "value", ... }, ...]` where values are
  bare strings (IRIs stripped of `<>`, literals stripped of quotes). If
  this contract changes upstream, the gem must adapt — and the gem's
  envelope must remain stable to MM.
- `sparql_ask(query TEXT) → 0 | 1`.
- `sparql_construct(query TEXT) → TEXT` — N-Triples serialization.

### Virtual table

- `rdf_triples` virtual table — `(subject, predicate, object)` columns,
  read + write. From 0.3.0 the vtab also exposes a HIDDEN `graph`
  column: `SELECT *` and 3-column `INSERT VALUES` are unchanged, and
  the graph is readable/writeable when named explicitly
  (`SELECT subject, graph FROM triples`,
  `INSERT INTO triples(subject, predicate, object, graph) VALUES (…)`).
  MM does **not** read this directly today, but `rails-semantica` may
  use it for bulk operations; if so, it's named in `rails-semantica`'s
  consumer requirement.

### Term encoding

N-Triples term syntax throughout — `<http://...>` for IRIs, `"value"` for
literals, `"value"@lang` / `"value"^^<datatype>` for tagged literals,
`_:b0` for blank nodes. MM expects the gem-level Storable concern to handle
serialization; the extension's job is to accept N-Triples-shaped TEXT
arguments.

## Build + load expectations

- `cargo build --release` produces a single shared library:
  `target/release/libsqlite_sparql.{dylib,so}`. MM's documentation hard-codes
  this path shape; renames break MM's QuickStart.
- The library is a SQLite **loadable extension** — loaded via
  `SELECT load_extension('path')` or Rails 8's `extensions:` key in
  `config/database.yml`. MM uses the Rails 8 native path; if the extension
  ever requires special init arguments beyond a plain `load_extension` call,
  the gem-level Loader must absorb the difference.
- Build target: `cdylib` crate type. MM does not consume the `rlib`.

## Thread / connection model

MM is a Rails 8 app with the default async ActionCable adapter in dev (no
Redis) and Solid Queue for background jobs. AR connection pool churns under
load. MM depends on:

- **Per-connection extension load.** Each new SQLite connection in the AR
  pool gets the extension loaded; the gem's Loader handles this. If the
  extension changes its thread / connection assumptions (currently
  thread-local Oxigraph store, per `CLAUDE.md`), the gem must adapt — and
  MM must see no behavioural change at the `Semantica::Sparql` envelope.
- **Per-connection store isolation is acceptable for v0.29.x.** MM's V0.29.x
  scope does not require cross-connection store sharing. If upstream adds a
  shared / persistent store later, MM will adopt opportunistically.

## Oxigraph semantics MM depends on

These are Oxigraph features `sqlite-sparql` re-exports that MM exercises.
Upstream Oxigraph version bumps are tolerable as long as these stay stable:

- SPARQL 1.1 `SELECT` with `FILTER(CONTAINS(LCASE(?x), "..."))` — MM's
  `OntologyResolver` Tier 0 traversal uses this shape.
- SPARQL 1.1 `ASK`.
- SPARQL 1.1 `CONSTRUCT` returning N-Triples.
- Blank-node round-tripping (subject in an insert, retrievable in a select).

Upstream Oxigraph features MM does NOT exercise (so version bumps that
break them are tolerable from MM's POV but the gem must still pass its
own specs):

- SPARQL UPDATE (intentionally not exposed via `Semantica::Sparql` — MM
  mutates via `Storable` lifecycle hooks).
- Named graph support (MM uses the default graph only).
- RDF/XML loading (MM loads via Storable, not bulk loaders).

## Behaviours MM does NOT depend on

So upstream is free to change these without notifying MM:

- The exact Cargo dependency versions, as long as the SQL surface above
  stays stable.
- Internal module layout (`src/functions/`, `src/vtab/`, etc.).
- The error enum's variant names (`SparqlError`) — MM never sees these
  directly; the gem-level Loader maps to `Semantica::ExtensionMissing`.
- The `examples/demo.sql` script — MM doesn't run it.

## Drift signals

A drift between this file and the extension's behaviour is detectable in
these places:

- `rails-semantica`'s own spec suite (gem-level Loader / Sparql specs)
  fails first. That's the primary tripwire.
- MM's `server/spec/integration/semantica_roundtrip_spec.rb` —
  `Product.create!` → `Sparql.select` round-trip goes red.
- MM's `bin/mm-smoke` — semantica step goes red.

When drift is detected, the fix path is:

1. Open an upstream PR on `sqlite-sparql` with the corrected behaviour +
   integration test coverage.
2. Land it; bump `rails-semantica`'s pin (if the gem also changes); record
   the new SHA in MM's submodule + `Gemfile.lock`.
3. Update this file if the consumer expectation changed.

Never fix drift by patching the extension from within MM, and never by
patching the gem from within MM. Both boundaries stay bright.

## Requested extensions

> **Re-numbering note (2026-05-19).** This section previously labelled
> the two requested extensions as "toward 0.2.0". The 0.2.0 slot was
> retasked for the shared process-wide store correctness fix surfaced
> in `docs/reviews/REVIEW_0.1.0.md` — see `docs/plans/PLAN_0.2.0.md`.
> The consumer-requested features below ship as **0.3.0** (named
> graphs, `docs/plans/PLAN_0.3.0.md`) and **0.4.0** (batched insert,
> `docs/plans/PLAN_0.4.0.md`). The functional contracts below are
> unchanged; only the version labels move.

### Named graph support — LANDED in 0.3.0

This section previously documented MM's requested surface. It is now
satisfied — `docs/plans/PLAN_0.3.0.md` is implemented, see § "Triple
management" and § "Virtual table" above for the live contract. The
previously-stated MM expectations all hold:

- `rdf_insert(subject, predicate, object, graph)` — 4th arg is the
  graph IRI (`NULL` for default). ✓
- `rdf_delete(…)` — same shape. ✓
- `sparql_query`, `sparql_ask`, `sparql_construct` — accept
  `FROM <graph>` / `FROM NAMED <graph>` / `GRAPH <graph> { … }`. ✓
  (passes straight through to Oxigraph 0.4)
- `rdf_triples` vtab — gains a HIDDEN `graph` column readable by name
  and writeable through a 4-column `INSERT`. ✓
- `rdf_count(graph)` — counts within a named graph; `NULL` is the
  default. `rdf_count_all()` covers the cross-graph case. ✓

Backward compatibility holds: every 0.1.0 / 0.2.0 caller works unchanged.

### Acceptance signal (named graph) — UPSTREAM READY

The upstream side of this acceptance signal is complete as of `v0.3.0`.
The MM-side work remains:

1. Bump the `sqlite-sparql` submodule SHA in MM to `v0.3.0` + the
   matching `rails-semantica` SHA (which surfaces the graph parameter
   through its Storable DSL — see
   [`rails-semantica/CONSUMER_REQUIREMENT_MM.md`](https://github.com/laquereric/rails-semantica/blob/main/CONSUMER_REQUIREMENT_MM.md#5-named-graph-parameter)).
2. Migrate the data: re-emit `ProductTripler`'s existing output into
   the `"bhphoto"` graph; delete the legacy `triples` AR table.
3. The "Triple management" + "Virtual table" sections above already
   list the named-graph surface as live.

### Array-argument batched insert (`rdf_insert_many`, toward 0.4.0)

Current write paths (`rdf_insert(s, p, o)` per call; SPARQL `INSERT
DATA { ... }`; `rdf_load_ntriples(text)`; the `rdf_triples` virtual
table) all work — but each puts the per-triple loop on the Ruby side
of the FFI boundary, either as N separate SQL calls or as Ruby
string-building work that the engine then re-parses. For PLAN_0_29_1
Phase B.1's copy migration (one-shot, thousands of triples) and for
`Semantica::Storable`'s per-save lifecycle hooks (every Product save
re-emits multiple predicates), Rust-side batching beats per-row work.

Proposed function:

```sql
SELECT rdf_insert_many(
  '[
    ["urn:mm:product:EPET2850", "schema:name", "\"Epson EcoTank\""],
    ["urn:mm:product:EPET2850", "schema:category", "\"printer\""],
    ["urn:mm:product:EPET2850", "schema:gtin", "\"01234567890123\""]
  ]'
);
-- → INTEGER (count inserted)
```

Semantics:

- Single argument: a JSON array of triple rows. Each row is a JSON
  array of 3 or 4 N-Triples-encoded terms (`[s, p, o]` or `[s, p, o, graph]`
  once named graphs ship). Strings carry their own `<>` / `""` / `^^<>`
  wrapping per N-Triples conventions — same as the existing
  `rdf_insert` scalar's arguments.
- Loops in Rust; one Oxigraph-store transaction for the whole batch.
- Returns the count actually inserted (post-dedup, since RDF is set
  semantics — re-inserting an existing triple is a no-op).
- Symmetric `rdf_delete_many(json_array)` would be natural too; same
  shape, same return value.

Backward compatibility: additive. `rdf_insert(s, p, o)` keeps its
current shape; the batched variant rides alongside.

Why JSON-arg over varargs or virtual-table-only: keeps the FFI
surface narrow (one TEXT param), matches existing SQLite extension
conventions (e.g. `sqlite-vec`'s vector functions accept JSON
arrays), and `Semantica::Sparql` can hand a single string across the
boundary without per-row prepared-statement bind overhead.

### Acceptance signal (batched insert)

When this lands, MM:

1. Bumps the `sqlite-sparql` submodule SHA in MM + the matching
   `rails-semantica` SHA (which exposes a `Semantica::Sparql.bulk_insert`
   convenience over the batched function — see
   [`rails-semantica/CONSUMER_REQUIREMENT_MM.md`](https://github.com/laquereric/rails-semantica/blob/main/CONSUMER_REQUIREMENT_MM.md#6-batched-write-convenience-sparqlbulk_insert)).
2. Rewrites the PLAN_0_29_1 Phase B.1 copy migration to call
   `Semantica::Sparql.bulk_insert` once per ~1000-triple batch (instead
   of N per-triple `INSERT DATA` calls).
3. `Semantica::Storable`'s per-save lifecycle hook batches all
   declared predicates for a record into a single batched call.
4. Updates this file: the batched-insert surface graduates from
   "Requested" into "SQL surfaces MM consumes."

## Contact

For questions about MM's consumption pattern, see MM's
`docs/architecture/Semantica.md` or open an issue on the MM repo.
