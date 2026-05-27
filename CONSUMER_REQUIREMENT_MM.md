# Consumer requirements — MagenticMarket substrate

This file records the surface [MagenticMarket](https://github.com/laquereric/magentic-market-ai)
(the substrate; "MM" hereafter) consumes from `sqlite-sparql`. It exists so
upstream changes can be checked against a written consumer expectation —
**drift** between this file and the extension's actual behaviour signals
work that needs to land in both repos lockstep.

MM consumes `sqlite-sparql` **indirectly**, through the
[`vv-graph`](https://github.com/laquereric/vv-graph) gem (renamed from
`rails-semantica` to `vv-graph` at v0.15.0 per `PLAN_0_82_0`; the
Ruby namespace also moved from `Semantica::*` to `Vv::Graph::*`). So in practice, drift on
this extension's surface tends to surface as failing specs in `vv-graph`
first, and only after that as failing specs in MM. Both layers care
about this file.

- MM repo: <https://github.com/laquereric/magentic-market-ai>
- MM plan that introduced the dependency: `docs/plans/PLAN_0_29_1.md`
- MM plan that landed the rename sweep: `docs/plans/PLAN_0_82_0.md`
- MM plan that authored this CR refresh: `docs/plans/PLAN_0_91_0.md`
- Intermediate consumer: `vv-graph` (its
  `CONSUMER_REQUIREMENT_VvGraph.md` — sibling of this file — covers
  the gem-level surface)

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
is checked in CI against the rev `vv-graph`'s `Gemfile.lock` was
tested against, so a single bump moves both layers lockstep.

## SQL surfaces MM (indirectly) consumes

MM exercises these via `Vv::Graph::Sparql` and `Vv::Graph::Storable`. If
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
- `rdf_insert_many(json TEXT) → INTEGER` (from 0.4.0) — JSON array of
  `[s,p,o]` or `[s,p,o,graph]` rows; one FFI crossing, one bulk-load,
  one return value (newly-inserted count, dedup-aware).
- `rdf_delete_many(json TEXT) → INTEGER` (from 0.4.0) — symmetric;
  rows not present in the store are silent no-ops and don't count.

### SPARQL querying

- `sparql_query(query TEXT) → TEXT` — JSON array of binding objects.
  `Vv::Graph::Sparql.select` parses the JSON; MM observes the parsed
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
  MM does **not** read this directly today, but `vv-graph` may
  use it for bulk operations; if so, it's named in `vv-graph`'s
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
  MM must see no behavioural change at the `Vv::Graph::Sparql` envelope.
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

- SPARQL UPDATE (intentionally not exposed via `Vv::Graph::Sparql` — MM
  mutates via `Storable` lifecycle hooks).
- Named graph support (MM uses the default graph only).
- RDF/XML loading (MM loads via Storable, not bulk loaders).

## Available upstream but not exercised by MM

These ship in the engine but MM does not call them today. They are
listed for completeness — if MM ever needs them, no upstream work is
required:

- `sparql_update(query) → INTEGER` (from 0.5.0) — arbitrary SPARQL
  1.1 UPDATE. MM mutates via `Storable` lifecycle hooks (which call
  `rdf_insert`/`rdf_delete`), so UPDATE is unused. vv-graph exposes it
  through `Vv::Graph::Sparql.execute` for the gem-level facade.
- `rdf_load_ntriples_to_graph(body, graph) → INTEGER` (from 0.6.0) —
  bulk-load an N-Triples body into a named graph in one FFI call.
  MM tripler output goes through `Storable` lifecycle hooks
  (single-row `rdf_insert` or batched `rdf_insert_many`), so the
  loader path is unused by MM. vv-graph routes graph-tagged `INSERT DATA`
  through it.
- `rdf_load_turtle_to_graph(body, graph)` /
  `rdf_load_rdfxml_to_graph(body, graph)` (from 0.6.0) — siblings
  for the Turtle / RDF/XML parsers. Same graph routing convention;
  no MM consumer.
- `rdf_load_rdfxml(text)` — MM doesn't bulk-load RDF/XML.
- `rdf_dump_ntriples()` — MM doesn't dump.
- `rdf_term_type` / `rdf_term_value` — MM hands string-typed values
  to the gem, which doesn't need these helpers.
- **RDF-star / SPARQL-star round-trip (from 0.7.0)** — quoted-triple
  terms (`<< <s> <p> <o> >>`) survive every read/write path; SPARQL-star
  syntax (annotation shorthand `{| |}`, explicit `<<>>` patterns,
  `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE` built-ins)
  flows straight through to Oxigraph. The MM vv-memory Silver
  Conformer is the intended consumer — see
  `docs/research/StarExts.md` §6 — but no MM code calls it today.
  Will graduate to "SQL surfaces MM consumes" when the Conformer
  PLAN pins the gem-level facade.
- `rdf_triple_subject(t)` / `rdf_triple_predicate(t)` /
  `rdf_triple_object(t)` (from 0.7.0) — plain-SQL destructors for
  quoted-triple terms. Conformer-adjacent; same status.
- `rdf_construct_many(queries_json) → TEXT (JSON array)` (from
  0.8.0) — runs N CONSTRUCTs in one FFI crossing, returns
  per-query N-Triples blobs. vv-graph driver (`Shacl::Rules.materialise!`
  fixpoint loop); no MM call site today.
- `rdf_owl_rl_materialise(asserted, inferred, options_json) → INTEGER`
  (from 0.9.0; full derivation coverage from 0.10.0) — native Rust
  fixpoint loop applying the full W3C OWL 2 RL/RDF derivation rule
  set (60 rules across Scm / Cls / Cax / Prp / Eq / Dt as of 0.10.0;
  inconsistency rules defer to a future `rdf_owl_rl_consistent`
  surface). Emits RDF-star `prov:wasDerivedFrom` annotations on
  every derived triple when `provenance: true`. vv-graph driver
  (`Vv::Graph::Reasoner.materialise!` would route through this once
  the gem floor bumps to engine ≥ 0.10.0); no MM call site today.
- `rdf_shacl_core_validate(data, shapes, report, options_json) →
  INTEGER` (from 0.11.0) — native Rust SHACL Core validator. Walks
  the data graph once per shape and emits a W3C-conformant
  `sh:ValidationReport` into a named report graph. Ships the
  12-constraint subset matching `vv-graph`'s
  `Vv::Graph::Shacl::ConstraintLibrary` plus a 7-form path
  evaluator. vv-graph driver (`Vv::Graph::Shacl.validate!` would
  route through this once the gem floor bumps to engine ≥ 0.11.0);
  no MM call site today.
- `rdf_dred_overdelete(inferred_iri, retracted_premises_json) →
  INTEGER` (from 0.12.0) — paired with a new `track_dependencies`
  option on `rdf_owl_rl_materialise`. Powers DRed-style
  delete-and-rederive incremental reasoning via a native side-table
  mapping inferred quads to per-derivation premise sets. 0.12.0
  tracks five W3C "core derivation" rules (`scm-sco`, `scm-spo`,
  `eq-trans`, `cax-sco`, `prp-spo1`); other rules still fire but
  skip the index write-through. vv-graph driver
  (`Vv::Graph::Reasoner.dred!` once that ships PLAN_0.11.0 Phase A);
  no MM call site today.
- `rdf_owl_rl_consistent(asserted, inferred, options_json) →
  TEXT` (from 0.13.0) — read-only pass over the 17 W3C OWL 2 RL
  inconsistency rules (`prp-irp/asyp/pdw/adp/npa1/npa2`,
  `cls-nothing2/com/maxc1/maxqc1/maxqc2`, `cax-dw/adc`,
  `eq-diff1/2/3`, `dt-not-type`). Returns a JSON array of
  `{rule, s, p, o}` records (or `"[]"` when consistent). Never
  inserts into the store. vv-graph driver
  (`Vv::Graph::Reasoner.consistent?` once that lands gem-side);
  no MM call site today.
- `sqlite-sparql` Ruby gem (in-tree `ruby/` subdirectory, since
  0.14.0). Loader (`SqliteSparql.load(db)`), ergonomic `Store`
  wrapper, and `HasRdfTriples` AR concern. MM continues to load
  the cdylib via Rails 8's `extensions:` config key (through
  `vv-graph`); the new gem is available as an alternative for
  non-Rails Ruby consumers but doesn't change MM's wiring path.
  Not yet on RubyGems (waits on cross-platform binary
  distribution); no MM call site today.

## Behaviours MM does NOT depend on

So upstream is free to change these without notifying MM:

- The exact Cargo dependency versions, as long as the SQL surface above
  stays stable.
- Internal module layout (`src/functions/`, `src/vtab/`, etc.).
- The error enum's variant names (`SparqlError`) — MM never sees these
  directly; the gem-level Loader maps to `Vv::Graph::ExtensionMissing`.
- The `examples/demo.sql` script — MM doesn't run it.

## Drift signals

A drift between this file and the extension's behaviour is detectable in
these places:

- `vv-graph`'s own spec suite (gem-level Loader / Sparql specs)
  fails first. That's the primary tripwire.
- MM's `server/spec/integration/semantica_roundtrip_spec.rb` —
  `Product.create!` → `Sparql.select` round-trip goes red.
- MM's `bin/mm-smoke` — semantica step goes red.

When drift is detected, the fix path is:

1. Open an upstream PR on `sqlite-sparql` with the corrected behaviour +
   integration test coverage.
2. Land it; bump `vv-graph`'s pin (if the gem also changes); record
   the new SHA in MM's submodule + `Gemfile.lock`.
3. Update this file if the consumer expectation changed.

Never fix drift by patching the extension from within MM, and never by
patching the gem from within MM. Both boundaries stay bright.

## Previously requested extensions — now landed

> **History.** This section previously listed two requested
> extensions as "toward 0.2.0". The 0.2.0 slot was retasked for the
> shared process-wide store correctness fix surfaced in
> `docs/reviews/REVIEW_0.1.0.md` (see `docs/plans/PLAN_0.2.0.md`),
> so the consumer-requested features shifted: named graphs shipped
> as **0.3.0** (`docs/plans/PLAN_0.3.0.md`) and batched insert as
> **0.4.0** (`docs/plans/PLAN_0.4.0.md`). Both are live and listed
> in the "SQL surfaces MM consumes" section above. The historical
> contracts and MM-side acceptance signals are kept below as the
> paper trail for the milestone-spanning work.

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
   matching `vv-graph` SHA (which surfaces the graph parameter
   through its Storable DSL — see
   [`vv-graph/CONSUMER_REQUIREMENT_MM.md`](https://github.com/laquereric/vv-graph/blob/main/CONSUMER_REQUIREMENT_MM.md#5-named-graph-parameter)).
2. Migrate the data: re-emit `ProductTripler`'s existing output into
   the `"bhphoto"` graph; delete the legacy `triples` AR table.
3. The "Triple management" + "Virtual table" sections above already
   list the named-graph surface as live.

### Array-argument batched insert — LANDED in 0.4.0

The upstream side is complete as of `v0.4.0`. The contract MM
requested is satisfied:

- `rdf_insert_many(json) → INTEGER` — accepts a JSON array of rows;
  each row is `[s, p, o]` or `[s, p, o, graph]`. Loops in Rust via
  Oxigraph's bulk loader; returns the post-dedup count of newly
  inserted quads. ✓
- Symmetric `rdf_delete_many(json)`. ✓
- Same N-Triples term parser as the single-row `rdf_insert`, pinned
  by `test_insert_many_parser_parity_with_single`. ✓
- Additive: `rdf_insert(s, p, o)` keeps its current shape. ✓

```sql
SELECT rdf_insert_many(
  '[
    ["urn:mm:product:EPET2850", "schema:name", "\"Epson EcoTank\""],
    ["urn:mm:product:EPET2850", "schema:category", "\"printer\""],
    ["urn:mm:product:EPET2850", "schema:gtin", "\"01234567890123\""]
  ]'
);
-- → 3
```

Live surface is documented in the "Triple management" section above.

### Acceptance signal (batched insert) — UPSTREAM READY

MM-side work remaining:

1. Bump the `sqlite-sparql` submodule SHA in MM to `v0.4.0` + the
   matching `vv-graph` SHA (which exposes a
   `Vv::Graph::Sparql.bulk_insert` convenience over the batched
   function — see
   [`vv-graph/CONSUMER_REQUIREMENT_MM.md`](https://github.com/laquereric/vv-graph/blob/main/CONSUMER_REQUIREMENT_MM.md#6-batched-write-convenience-sparqlbulk_insert)).
2. Rewrite the PLAN_0_29_1 Phase B.1 copy migration to call
   `Vv::Graph::Sparql.bulk_insert` once per ~1000-triple batch
   (instead of N per-triple `INSERT DATA` calls).
3. `Vv::Graph::Storable`'s per-save lifecycle hook batches all
   declared predicates for a record into a single batched call.

## Contact

For questions about MM's consumption pattern, see MM's
`docs/architecture/Semantica.md` or open an issue on the MM repo.

## Last reviewed

2026-05-25 against MM substrate commit `e66aa9d` per `docs/plans/PLAN_0_91_0.md` (Phase A).
