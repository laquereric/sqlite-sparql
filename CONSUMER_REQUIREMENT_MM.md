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
- `rdf_delete(subject TEXT, predicate TEXT, object TEXT) → 1`.
- `rdf_count() → INTEGER` — used by MM's `bin/mm-smoke` semantica step.

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
  read + write. MM does **not** read this directly today, but
  `rails-semantica` may use it for bulk operations; if so, it's named in
  `rails-semantica`'s consumer requirement.

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

## Contact

For questions about MM's consumption pattern, see MM's
`docs/architecture/Semantica.md` or open an issue on the MM repo.
