# PLAN 0.6.0 — graph-scoped bulk loading

> Add `rdf_load_ntriples_to_graph(body TEXT, graph_iri TEXT) → INTEGER`
> (and a symmetric `rdf_load_turtle_to_graph` / `rdf_load_rdfxml_to_graph`)
> so consumers can bulk-load an N-Triples / Turtle / RDF/XML body into a
> named graph in a single FFI call. Today `rdf_load_*` hard-forces every
> parsed quad into the default graph (see `src/functions/rdf_triple.rs`
> `load_rdf` → `GraphName::DefaultGraph`), which is the last remaining
> named-graph gap on the engine's SQL surface.

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item **#1
Named graph support — INSERT path**. RS's `Semantica::Sparql.execute`
routes `INSERT DATA { GRAPH <iri> { … } }` through `rdf_load_ntriples`
today; with the default-graph forcing in place, the `GRAPH` wrapper is
silently discarded and the payload lands in the default graph. RS ships
`:engine_unsupported` refusal envelopes for graph-tagged INSERT paths
until this lands.

MM's `CONSUMER_REQUIREMENT_MM.md` has no outstanding ask against the
engine — named graphs (0.3.0), batched insert (0.4.0), and SPARQL UPDATE
(0.5.0) are all live. This release is RS-driven; MM benefits transitively
once RS un-stubs its graph-tagged write paths.

Depends on 0.2.0 (shared store), 0.3.0 (named-graph plumbing in
`store.rs`: `insert_triple_in_graph`, `build_quad`, blank-node-graph
rejection — all reused here).

---

## Goal

`cargo test` passes a new round-trip test that loads N-Triples into
`<urn:g:bhphoto>`, asserts isolation from the default graph, and reads
the rows back via a `GRAPH { … }` SPARQL pattern. RS can then drop the
`:engine_unsupported` stub on graph-tagged `INSERT DATA` and graduate
items #1–#4 of `CONSUMER_REQUIREMENT_RS.md` from "Requested" to
"SQL surfaces RS consumes".

---

## Why a separate scalar, not "teach `rdf_load_ntriples` to honour an
enclosing `GRAPH { … }` wrapper"

RS's CONSUMER doc offers both shapes. The separate-scalar route wins:

- N-Triples grammar (RDF 1.1 § 4) has no graph syntax. A body that
  contains `GRAPH <iri> { … }` is not N-Triples — it is TriG. Conflating
  the two under the `rdf_load_ntriples` name would mean either silently
  switching parsers or accepting a non-grammar superset; both are worse
  than naming the operation honestly.
- The single-row 4-arg surface (`rdf_insert(s, p, o, graph)` from 0.3.0)
  already established the convention that "graph-scoped variant" is a
  separate arity, not a re-interpretation of the payload. A separate
  scalar keeps that convention.
- The parser pick stays orthogonal to the graph routing. The three
  `_to_graph` variants reuse the existing `RdfFormat::{NTriples, Turtle,
  RdfXml}` parsers — no new format selection logic.

---

## Phase A — extend the internal `load_rdf` helper

`src/functions/rdf_triple.rs` currently houses:

```rust
fn load_rdf(text: &str, format: RdfFormat) -> crate::error::Result<usize> {
    with_store(|store| {
        let mut count = 0usize;
        let parser = RdfParser::from_format(format);
        for quad_result in parser.for_reader(text.as_bytes()) {
            let quad = quad_result
                .map_err(|e| SparqlError::RdfParseError(e.to_string()))?;
            // Force all triples into the default graph
            let dg_quad = oxigraph::model::Quad::new(
                quad.subject,
                quad.predicate,
                quad.object,
                GraphName::DefaultGraph,
            );
            store
                .insert(&dg_quad)
                .map_err(|e| SparqlError::StoreError(e.to_string()))?;
            count += 1;
        }
        Ok(count)
    })
}
```

Change the signature to take an optional graph IRI and route quads
accordingly:

```rust
fn load_rdf(
    text: &str,
    format: RdfFormat,
    graph: Option<&str>,
) -> crate::error::Result<usize> { … }
```

Behaviour:

- `graph = None` → preserve today's semantics exactly. Every parsed quad
  lands in `GraphName::DefaultGraph`, regardless of any graph slot the
  parser produced. This is what the existing single-arg loaders need so
  the 0.5.0 contract does not move.
- `graph = Some(iri)` → build a `NamedNode` from `iri` once
  (reusing `crate::store::build_named_node_or_err` if the helper is in
  scope, or inlining the same `NamedNode::new(iri).map_err(…)` pattern
  the 4-arg `rdf_insert` already uses). Wrap every parsed quad into
  `Quad::new(subject, predicate, object, GraphName::NamedNode(node))`.
  Blank-node graph IRIs are rejected with the same error message
  `store::build_quad` raises for the 4-arg insert path
  (`SparqlError::InvalidIri("blank-node graphs are not supported")` —
  match the exact wording the 0.3.0 path uses so RS's prefix-matching
  refusal envelope keeps working unchanged).

The 1-arg call sites (`rdf_load_turtle_fn`, `rdf_load_ntriples_fn`,
`rdf_load_rdfxml_fn`) update to pass `None`. No behaviour change at the
1-arg surface — pinned by the existing tests.

### Exit criteria for Phase A

```
cargo build              # 0 errors, 0 warnings
cargo test               # all green; no test changes yet
```

Existing tests `test_rdf_load_ntriples` and `test_rdf_load_turtle` (line
139ff in `tests/integration_test.rs`) keep passing — they exercise the
1-arg path which still routes to `None`.

---

## Phase B — the three new scalars

Add three 2-arg scalars beside their 1-arg siblings in
`src/functions/rdf_triple.rs`:

```rust
pub fn rdf_load_ntriples_to_graph_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let nt = api::value_text(values.get(0).expect("N-Triples text"))?;
    let g  = graph_arg(values.get(1).expect("graph"))?;
    let count = load_rdf(nt, RdfFormat::NTriples, g)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}
```

`rdf_load_turtle_to_graph_fn` and `rdf_load_rdfxml_to_graph_fn` follow
the same shape against `RdfFormat::Turtle` / `RdfFormat::RdfXml`.

Register all three in `register()`:

```rust
define_scalar_function(db, "rdf_load_ntriples_to_graph", 2,
    rdf_load_ntriples_to_graph_fn, FunctionFlags::UTF8)?;
define_scalar_function(db, "rdf_load_turtle_to_graph", 2,
    rdf_load_turtle_to_graph_fn,   FunctionFlags::UTF8)?;
define_scalar_function(db, "rdf_load_rdfxml_to_graph", 2,
    rdf_load_rdfxml_to_graph_fn,   FunctionFlags::UTF8)?;
```

`graph = NULL` continues to mean "default graph" (matches the 4-arg
`rdf_insert` / `rdf_delete` convention from 0.3.0). RS uses non-NULL
graph IRIs exclusively, but the NULL passthrough is the cheapest way to
let callers parameterise the graph without branching at the SQL layer.

### Note on `rdf_load_turtle_to_graph` and `rdf_load_rdfxml_to_graph`

Neither RS nor MM asks for these today. Shipping them anyway because:

- The internal `load_rdf` helper already takes `format` — adding the
  graph parameter to one format and not the others would create an
  asymmetric surface that future readers (and future consumers) would
  have to explain.
- Each one is ~10 lines: a wrapper + a `define_scalar_function` call.
  The cost of including them is below the cost of explaining their
  absence.

If review pushes back on the YAGNI grounds, dropping the Turtle and
RDF/XML variants is a trivial subtractive change; the N-Triples variant
is the only one with a named consumer.

### Exit criteria for Phase B

`cargo build` clean. `sqlite3 :memory:` after `.load …` exposes the
three new functions via `SELECT rdf_load_ntriples_to_graph(…)`.

---

## Phase C — integration tests

Add to `tests/integration_test.rs`, beside the existing
`test_rdf_delete_4arg_named_graph`:

### `test_rdf_load_ntriples_to_graph_roundtrip`

1. `rdf_clear()`.
2. `SELECT rdf_load_ntriples_to_graph(?, 'urn:g:bhphoto')` with a 3-line
   N-Triples body:
   ```
   <http://e/a> <http://e/p> "x" .
   <http://e/b> <http://e/p> "y" .
   <http://e/c> <http://e/p> "z" .
   ```
   Expect return value `3`.
3. Assert `rdf_count() = 0` (default graph empty).
4. Assert `rdf_count('urn:g:bhphoto') = 3`.
5. Assert `rdf_count_all() = 3`.
6. Issue `SELECT sparql_query('SELECT ?s WHERE { GRAPH <urn:g:bhphoto> { ?s ?p ?o } }')`
   and assert all three subjects come back.
7. Issue `SELECT sparql_query('SELECT ?s WHERE { ?s ?p ?o }')` (default
   graph) and assert empty `[]`.

### `test_rdf_load_ntriples_to_graph_null_is_default`

1. `rdf_clear()`.
2. `SELECT rdf_load_ntriples_to_graph(?, NULL)` with one triple.
3. Assert `rdf_count() = 1` and `rdf_count_all() = 1`.

Pins that `graph = NULL` is behaviourally identical to the 1-arg loader.

### `test_rdf_load_ntriples_to_graph_rejects_blank_node_graph`

1. `rdf_clear()`.
2. `SELECT rdf_load_ntriples_to_graph(?, '_:bgraph')` — expect a SQLite
   error whose message matches the same prefix the 4-arg `rdf_insert`
   raises for blank-node graphs. The exact string is fixed by whatever
   the 0.3.0 path emits today; this test pins parity.

### `test_rdf_load_ntriples_to_graph_parser_parity`

Same N-Triples body loaded via `rdf_load_ntriples` (1-arg, default
graph) and via `rdf_load_ntriples_to_graph(…, NULL)` produces
byte-identical `rdf_dump_ntriples()` output. Pins that the new path
does not accidentally drift the parser behaviour.

(Mirrors `test_insert_many_parser_parity_with_single` from 0.4.0.)

### Exit criteria for Phase C

```
cargo test               # all green
cargo test --release     # same
```

Test count climbs by 4.

---

## Phase D — docs

- `README.md` — extend the "SQL Function Reference" / "Triple
  Management" subsection to list the three new scalars under the
  existing 1-arg loaders, with one-line semantics each.
- `CLAUDE.md` — the "SQL Function Reference" table near the bottom
  gains three rows for the new functions.
- `CHANGELOG.md` — add a 0.6.0 entry. Lead with "named-graph bulk
  loading — `rdf_load_*_to_graph(body, graph)` lands the last piece of
  RS's named-graph surface ask". Cross-reference
  `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item #1.
- `src/functions/rdf_triple.rs` doc comment at the top — extend the
  table to list the three new functions.

### `CONSUMER_REQUIREMENT_RS.md` graduations

This file is the most out-of-date relative to current code. With 0.6.0
landing, fold its "Requested extensions" section the way
`CONSUMER_REQUIREMENT_MM.md` was folded by 0.3.0 / 0.4.0:

- **#1 Named graph INSERT path** — graduate to a new row in the "Triple
  management" table:
  `rdf_load_ntriples_to_graph(body TEXT, graph_iri TEXT) → INTEGER`
  (plus the Turtle / RDF/XML siblings). Note the `graph = NULL`
  shorthand for "default graph".
- **#2 Named graph DELETE path** — graduate. Already live as the 4-arg
  `rdf_delete(s, p, o, graph)` since 0.3.0; the RS doc just never
  caught up. Add the row to the "Triple management" table; note that
  subject + predicate remain bare IRIs per the existing asymmetry
  contract.
- **#3 Named graph SPARQL query path** — graduate by reference. No
  engine change needed (Oxigraph 0.4 already honours `GRAPH { … }`).
  Point the doc at the existing
  `test_rdf_delete_4arg_named_graph` / `test_named_graph_query_isolation`
  spec (line ~516 in `tests/integration_test.rs`) as the confirming
  fixture.
- **#4 `rdf_insert_many`** — graduate. Live as of 0.4.0; RS doc just
  needs the row in "Triple management".
- **#5 `sparql_update`** — already marked LANDED, leave alone.

After graduation, the "Requested extensions" section is empty; replace
it with a "Previously requested extensions — now landed" subsection in
the same style as `CONSUMER_REQUIREMENT_MM.md`, so the paper trail
survives.

### `CONSUMER_REQUIREMENT_MM.md` touchup

MM has no outstanding asks, but the "Available upstream but not
exercised by MM" section should grow three rows for the new
`rdf_load_*_to_graph` scalars (mirrors how `sparql_update` is listed
there today). MM doesn't bulk-load, so they sit in the
"available-but-unused" bucket from day one.

### Exit criteria for Phase D

Reading `CONSUMER_REQUIREMENT_RS.md` top-to-bottom no longer mentions
"Requested" items #1–#4. Reading `CHANGELOG.md` shows a 0.6.0 entry
naming the three new scalars.

---

## Phase E — tag 0.6.0

- Bump `Cargo.toml` and `VERSION` to `0.6.0`.
- `cargo test` green at the bumped version.
- `git tag v0.6.0` and push.
- Ping RS to open its PLAN that adopts the new surface (un-stub the
  `:engine_unsupported` refusal envelopes on graph-tagged INSERT
  paths; route `INSERT DATA { GRAPH <iri> { … } }` through
  `rdf_load_ntriples_to_graph`).

---

## Risks

- **Oxigraph parser produces non-default graph slots.** A TriG body
  fed to `rdf_load_ntriples_to_graph` could in principle carry its own
  graph annotation on each quad. The current `RdfFormat::NTriples`
  parser cannot — N-Triples has no graph syntax — but the
  `RdfFormat::Turtle` and `RdfFormat::RdfXml` parsers also produce
  triples without a graph slot, so the question is moot for the
  formats this release ships. If a future release adds a
  `_load_trig_to_graph` variant, decide then whether the explicit
  `graph` arg overrides the in-body graph or errors on conflict. Out
  of scope here.
- **The 4-arg `rdf_insert` IRI-validation path vs the parser's IRI
  validation.** The 4-arg insert builds the graph node via
  `NamedNode::new(iri)`; the parser builds subject / predicate / object
  nodes through its own grammar. Both reject malformed IRIs, but the
  error strings differ. The test
  `test_rdf_load_ntriples_to_graph_rejects_blank_node_graph` pins the
  message for the graph slot; subject / predicate / object error
  shapes stay whatever the parser already raises (pinned by the
  existing 0.5.0 round-trip tests).
- **Performance vs `rdf_insert_many`.** The new scalar inserts one
  quad at a time inside `load_rdf`'s loop, same as the 1-arg loaders
  do today. Oxigraph 0.4's `Store::insert(&quad)` is the cost floor
  here; if a future release exposes a `Store::bulk_load_quads` or
  similar API, the loop body can collapse without changing the SQL
  surface. Not a release blocker.

---

## Out of scope for 0.6.0

- **Persistent RocksDB backend.** PLAN_0.2.0's roadmap penciled this
  in as 0.6.0. Consumer pressure (RS item #1) is real and in front of
  us; persistence is not. Slip RocksDB to 0.7.0 and renumber the
  trailing milestones in `PLAN_0.2.0.md`'s roadmap table accordingly
  (gem wrapper → 0.8.0, HTTP endpoint → 0.9.0).
- **TriG loader.** No consumer asks for it; the conflation hazard
  outlined above is the reason to defer until someone does.
- **`rdf_dump_ntriples_from_graph(graph TEXT)`.** Symmetric dumper for
  one graph. Neither RS nor MM exercises bulk dumping (per their
  CONSUMER docs — `rdf_dump_ntriples` is listed under "Available
  upstream but not exercised"). Skip until asked.
- **A 5-arg `rdf_insert_many(json, graph)` overload.** The existing
  `rdf_insert_many` already accepts `[s, p, o, graph]` rows, so the
  per-row graph IRI is reachable today without a new function. Not a
  gap.

---

## Re-numbering downstream milestones

`PLAN_0.2.0.md`'s roadmap table currently lists:

| Version | Topic |
|---|---|
| 0.5.0 | SPARQL UPDATE |
| 0.6.0 | Persistent RocksDB backend |
| 0.7.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.8.0 | SPARQL HTTP endpoint |

After this plan, the trailing rows shift:

| Version | Topic |
|---|---|
| 0.6.0 | Graph-scoped bulk loading (this file) |
| 0.7.0 | Persistent RocksDB backend |
| 0.8.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.9.0 | SPARQL HTTP endpoint |

Update the table in `PLAN_0.2.0.md` as part of Phase D's doc pass. Do
**not** edit `PLAN_0.2.0.md`'s prose — the renumbering is the only
change there.
