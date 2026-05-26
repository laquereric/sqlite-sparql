# Changelog

## 0.10.0 — Full OWL 2 RL derivation coverage (~45 additional rules)

`rdf_owl_rl_materialise`'s rule library grows from 15 to 60 rules,
covering the W3C OWL 2 RL/RDF derivation rules across all five
tables (Scm, Cls, Cax, Prp, Eq, Dt). Function signature, return
shape, atomicity contract, and error envelopes from 0.9.0 are
unchanged — this release is purely additive at the rule level.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions"
item #6, second bullet ("The remaining ~55 rules … land in engine
0.10.0; Vv::Graph callers using ontologies that depend on
out-of-subset constructs stay on the per-rule `Sparql.execute`
path until then"). With this release, Vv::Graph's
`Vv::Graph::Reasoner::Rules::PHASE_B_PENDING` has a native engine
path; the gem can graduate its rule library on its own cadence.

New rules grouped by W3C table (45 total):

- **Scm — T-Box (16):** scm-cls, scm-op, scm-dp, scm-eqc2,
  scm-eqp2, scm-dom1, scm-dom2, scm-rng1, scm-rng2, scm-hv,
  scm-svf1, scm-svf2, scm-avf1, scm-avf2, scm-int, scm-uni.
- **Cls — class-expression A-Box (14):** cls-thing, cls-nothing1,
  cls-int1, cls-int2, cls-uni, cls-svf1, cls-svf2, cls-avf,
  cls-hv1, cls-hv2, cls-maxc2, cls-maxqc3, cls-maxqc4, cls-oo.
- **Cax — class-axiom A-Box (2):** cax-eqc1, cax-eqc2.
- **Prp — property reasoning (5):** prp-ifp, prp-spo2
  (property-chain composition via `owl:propertyChainAxiom`),
  prp-eqp1, prp-eqp2, prp-key (`owl:hasKey` → sameAs with
  cartesian-product key matching for multi-valued keys).
- **Eq — equality (4):** eq-ref, eq-rep-s, eq-rep-p, eq-rep-o.
- **Dt — datatype (4):** dt-type1 (closed 31-IRI W3C list of
  XSD + RDF datatypes get `rdf:type rdfs:Datatype`), dt-type2
  (same for consumer-defined datatypes that appear as literal
  datatypes in the store), dt-eq, dt-diff.

Two new options on `MaterialiseOptions`:

- `equality_saturation: bool` — gates `eq-rep-s` / `eq-rep-p` /
  `eq-rep-o`. **Default `true`** (W3C semantics). Set to `false`
  to short-circuit term-substitution when a graph with heavy
  `owl:sameAs` linkage would otherwise produce an O(N · K)
  closure. `eq-sym` and `eq-trans` continue to fire regardless.
- `eq_reflexive: bool` — gates `eq-ref`. **Default `false`** —
  this is a deviation from the original PLAN_0.10.0 design, which
  shipped `eq-ref` on by default. Phase D test runs showed
  `eq-ref` with `provenance: true` does not converge: every
  reflexive `?s owl:sameAs ?s` it derives gets two annotation
  triples whose subjects are quoted-triple terms new to the
  inferred graph, which `eq-ref` then derives reflexives for, and
  so on — the closure runs out the 50-iteration cap. The opt-in
  default keeps the engine bounded; consumers round-tripping
  against a W3C-strict reasoner that expects reflexive saturation
  enable it explicitly.

Known limitations:

- **`dt-eq` / `dt-diff` are no-ops in Oxigraph 0.4.** The W3C rule
  emits `?lit1 owl:sameAs ?lit2` / `?lit1 owl:differentFrom ?lit2`
  where both sides are literals. Oxigraph 0.4's `Subject` enum
  has no `Literal` variant, so a literal-subject triple is
  type-blocked at construction time. Both rule functions are
  wired into the dispatch but return empty derivations. Revive
  when Oxigraph upgrades the model (≥ 0.5).
- **Inconsistency rules deferred.** The ~15 W3C "false"-deriving
  rules (`prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1/2`,
  `cls-com`, `cls-nothing2`, `cls-maxc1`, `cls-maxqc1/2`,
  `cax-dw`, `cax-adc`, `eq-diff1/2/3`, `dt-not-type`) are not in
  this release. They detect contradictions rather than derive
  triples, which doesn't fit `rdf_owl_rl_materialise`'s monotonic
  fixpoint contract. A separate `rdf_owl_rl_consistent` surface
  is queued for a future release returning a JSON array of
  violation records.

Implementation notes:

- New helper module `src/functions/rdf_owl_rl/rdf_lists.rs` —
  walks `rdf:first` / `rdf:rest` chains for the list-using rules
  (cls-int1/2, cls-uni, cls-oo, scm-int, scm-uni, prp-spo2,
  prp-key). Cycle-safe; rejects ambiguous / malformed lists as
  `None` so rule bodies fail gracefully.
- The 0.9.0 fixpoint loop, provenance emission, options blob,
  return-shape, error envelopes, and dedup-against-inferred check
  are all reused unchanged. Only `rules.rs` grew (and the
  dispatch table within it).
- `Cargo.toml` and `VERSION` bump to `0.10.0`. No new
  external dependencies.

Tests: 30 → 61 lib + 70 → 77 integration + 1 ignored, both debug
and release. Per-rule lib smoke tests cover each derivation rule
in isolation; 5 full-stack integration tests
(`*_intersection_round_trip`, `*_property_chain_uncle`,
`*_has_key_resolves_duplicates`,
`*_inverse_functional_property_collapses`, `*_dt_type1_emits_xsd_axioms`)
exercise the rule families through the SQL surface;
`*_equality_saturation_disabled` and
`*_equality_saturation_default_substitutes` pin the new option's
contract.

See `docs/plans/PLAN_0.10.0.md` for the full design (scope split
rationale, equality-saturation discussion, the realised `eq-ref`
non-convergence under provenance, the deferred-inconsistency
follow-on plan).

## 0.9.0 — Native OWL 2 RL rule pass (15-rule subset)

`rdf_owl_rl_materialise(asserted_iri TEXT, inferred_iri TEXT,
options_json TEXT) → INTEGER` runs a native Rust fixpoint loop over
Oxigraph's store, applying 15 W3C OWL 2 RL/RDF rules in one FFI
crossing in place of `vv-graph`'s per-rule `Sparql.execute`
round-trip. Skips the SPARQL parser per rule; ships parity with
`vv-graph`'s `Vv::Graph::Reasoner::Rules::OwlRl` so the engine +
gem produce identical closures (pinned by
`test_rdf_owl_rl_materialise_equivalence_with_vg`).

Driver: `CONSUMER_REQUIREMENT_VG.md` § "Requested extensions"
item #6. VG's `Vv::Graph::Reasoner.materialise!` (gem-side Phase B
already shipped) issues one `sparql_update` per rule per fixpoint
iteration. The native pass collapses N rules × M iterations of
SQL parse + SPARQL parse + evaluate to a single FFI crossing while
preserving the gem's `:derivedBy <rule_iri> ; :derivedAt …`
RDF-star provenance shape.

Surface:

- `rdf_owl_rl_materialise(asserted_iri, inferred_iri, options_json) → INTEGER`
  - `asserted_iri = NULL` → default graph; otherwise a named graph.
  - `inferred_iri = NULL` is **rejected** — derived triples mixing
    into the default graph would erase the asserted-vs-derived
    distinction OWL reasoning depends on.
  - `options_json` JSON object; all fields optional. Defaults:
    `{"max_iterations": 50, "provenance": false,
     "derived_by_iri": "http://www.w3.org/ns/prov#wasDerivedFrom",
     "derived_at_iri": "http://www.w3.org/ns/prov#generatedAtTime",
     "rule_iri_prefix": "urn:semantica:rule:"}`.
  - Return: signed net delta in store size — matches
    `sparql_update`'s convention.

Rule coverage (the 15 rules — W3C names verbatim):

| Bucket | Rules |
|---|---|
| T-Box transitive closure | `scm-sco`, `scm-spo`, `scm-eqc1`, `scm-eqp1` |
| A-Box propagation        | `cax-sco`, `prp-spo1` |
| Domain / range           | `prp-dom`, `prp-rng` |
| Property characteristics | `prp-trp`, `prp-symp`, `prp-inv1`, `prp-inv2`, `prp-fp` |
| sameAs closure           | `eq-sym`, `eq-trans` |

The remaining ~55 W3C OWL 2 RL rules are deferred to 0.10.0.
Operators using ontologies that depend on out-of-subset constructs
(`owl:intersectionOf`, `owl:unionOf`, `owl:hasKey`, etc.) should
stay on the per-rule `sparql_update` path until 0.10.0 ships.

With `"provenance": true`, every derived triple is annotated with
two RDF-star quads in the inferred graph (since 0.7.0):

```
<< <s> <p> <o> >> prov:wasDerivedFrom <urn:semantica:rule:scm-sco> .
<< <s> <p> <o> >> prov:generatedAtTime "2026-05-25T20:02:43Z"^^xsd:dateTime .
```

The predicate IRIs and rule-IRI prefix are operator-overridable;
defaults match `vv-graph`'s `Vv::Graph::Reasoner` convention.

Decisions worth flagging for consumers:

- **Provenance shape commits to defaults that match VG.** Deviation
  from PLAN_0.7.0/0.8.0's "engine stays domain-agnostic" posture —
  materialisation has nowhere to put provenance except on the
  triple it just derived (no consumer round-trip the way
  `rdf_construct_many` has). The override mechanism softens the
  coupling for callers using a different provenance vocabulary.
  See `docs/plans/PLAN_0.9.0.md` for the rationale.
- **Atomicity is partial-on-iteration.** If the fixpoint isn't
  reached within `max_iterations`, the partial closure stays in
  the inferred graph rather than rolling back. Matches
  `sparql_update`'s partial-on-evaluation contract from 0.5.0.
- **No `chrono` dependency.** A hand-rolled RFC3339 formatter
  (Hinnant's civil-from-days algorithm, ~20 lines) handles the
  one timestamp call site. Avoids ~150 KB of dylib growth from
  a single-use dep.

Error envelopes (fixed prefix for consumer pattern-matching):

- `rdf_owl_rl_materialise: inferred_iri must be a named graph
  (NULL is not allowed for the inferred slot)`
- `rdf_owl_rl_materialise: fixpoint not reached after N iterations`
- `rdf_owl_rl_materialise: rule <id> error at iteration N: …`
- `rdf_owl_rl_materialise: options_json: …`

Tests: 62 → 70 + 1 ignored. 8 new under
`// ── 0.9.0 rdf_owl_rl_materialise ──` in
`tests/integration_test.rs`. The equivalence test pins the engine's
closure against a hand-written expected fixture (the closure VG
would produce for the same input).

## 0.8.0 — Batched CONSTRUCT

`rdf_construct_many(queries_json TEXT) → TEXT` evaluates N CONSTRUCT
queries in one FFI crossing. The return is a JSON array of N
N-Triples blobs — one per input query — preserving per-query
attribution so consumers can annotate per-rule downstream before
inserting. Matches the `_many` convention from 0.4.0 (`rdf_insert_many`)
and 0.6.0 (`rdf_load_*_to_graph`).

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item
#9 (Batched SHACL Rules execution), added in the post-v0.7.0 doc
update. RS's `Shacl::Rules.materialise!` issues one `sparql_update`
per rule per fixpoint iteration; ~50 rules per iteration paying the
SQL + FFI overhead 50× collapses to 1× with this scalar. (The
per-rule SPARQL parse cost still happens N× — Oxigraph parses each
query at evaluation time. The savings are on the SQL/FFI side, not
the SPARQL parser. A prepared-query model would be a separate, much
larger plan.)

Surface:

- `rdf_construct_many(queries_json TEXT) → TEXT` — `queries_json`
  is a JSON array of CONSTRUCT query strings. Returns a JSON array
  of the same length where the `i`-th element is the N-Triples
  output of the `i`-th query (an empty string when the query
  binds zero triples).

Decisions worth flagging for consumers:

- **JSON array of N-Triples blobs**, not a flat blob or an integer
  count. Flat would lose per-query attribution; integer would imply
  the engine inserts results (it doesn't — CONSTRUCT is read-only,
  and provenance shape is RS's call, not ours). See `docs/plans/PLAN_0.8.0.md`
  for the full rationale.
- **Provenance stays out of the engine.** Same posture as PLAN_0.7.0:
  the engine emits data, the consumer (RS) attaches `:derivedBy`
  / `:derivedAt` annotations downstream. The name
  `rdf_construct_many_with_provenance` is deliberately left
  unoccupied for a future engine-side annotation variant if RS asks.
- **All-or-nothing pre-flight parse.** Every query is parsed up
  front; if any fails the batch errors with the prefix
  `SPARQL parse error (query index N):` before any query evaluates.
  Matches `rdf_insert_many`'s atomicity contract.
- **Non-CONSTRUCT input is rejected** with the prefix
  `rdf_construct_many: query index N is not a CONSTRUCT`.
- **Non-array JSON input is rejected** with the prefix
  `rdf_construct_many: expected JSON array of query strings`.

No surface change to `sparql_construct` (1-arg). 1-element batches
are byte-identical to the 1-arg path — pinned by
`test_rdf_construct_many_parser_parity_with_single`.

RDF-star CONSTRUCT outputs (quoted-triple subjects from 0.7.0) flow
through unchanged — pinned by `test_rdf_construct_many_with_rdf_star`.

## 0.7.0 — RDF-star / SPARQL-star round-trip

Quoted-triple terms now survive the SQL boundary in both directions.
Before 0.7.0, the term serialiser in `src/functions/sparql_query.rs`
stubbed every `Term::Triple` / `Subject::Triple` to the literal string
`"<<rdf-star unsupported>>"`, and the term parser in `src/store.rs`
rejected any `<<…>>` input as a malformed IRI. The engine side
(Oxigraph 0.4) already parsed Turtle-star / N-Triples-star and
evaluated SPARQL-star — only the SQL boundary lost information.

Surface delta:

- **Write paths** — `rdf_insert(s, p, o[, g])`, `rdf_delete(s, p, o[, g])`,
  `rdf_insert_many`, `rdf_delete_many`, and the `rdf_triples` vtab all
  accept `<< <s> <p> <o> >>` in subject and object position. Predicate
  position stays IRI-only (RDF doesn't extend star to predicates).
- **Read paths** — `rdf_dump_ntriples`, `sparql_construct`, the JSON
  bindings from `sparql_query`, and `SELECT` over the `rdf_triples`
  vtab all emit `<< s p o >>` for quoted-triple terms. Nesting
  (`<< << s p o >> p o >>`) round-trips.
- **SPARQL-star** flows straight through to Oxigraph — annotation
  shorthand `{| |}`, explicit `<<>>` patterns, and the
  `TRIPLE` / `SUBJECT` / `PREDICATE` / `OBJECT` / `isTRIPLE` built-ins
  all work without SQL-side wrapping.
- **New scalars** (additive — every 0.6.x call works unchanged):
  - `rdf_triple_subject(term) → TEXT` — extract subject of a quoted triple.
  - `rdf_triple_predicate(term) → TEXT` — extract predicate.
  - `rdf_triple_object(term) → TEXT` — extract object.

Behaviour changes (call out for consumers):

- `rdf_term_type(term)` now returns `"triple"` for a `<<…>>` string
  (previously `"unknown"`).
- `rdf_term_value(term)` on a `<<…>>` string now raises a SQLite
  error with the fixed-prefix message
  `rdf_term_value: triple terms have no scalar value; use
  rdf_triple_subject / rdf_triple_predicate / rdf_triple_object: …`
  Previously raised `unrecognised term format: …`. Prefix-matching
  consumers (none known) must update.

Driver: the MM Conformer subagent in vv-memory's Silver tier — see
`docs/research/StarExts.md` §6. Neither `CONSUMER_REQUIREMENT_MM.md`
nor `CONSUMER_REQUIREMENT_RS.md` calls the new surface yet; both
list it in their "Available upstream but not exercised" sections so
the paper trail is in place when consumers adopt.

RocksDB persistence (penciled in for 0.7.0 by earlier roadmaps) is
deferred indefinitely — no consumer pressure. Revive on first ask.

## 0.6.0 — Graph-scoped bulk loading

Closes the last named-graph gap on the SQL surface. Until 0.6.0 the
three bulk loaders forced every parsed quad into the default graph,
which meant a consumer issuing `INSERT DATA { GRAPH <iri> { … } }`
through `rdf_load_ntriples` saw the `GRAPH` wrapper silently
discarded. Three new scalars route the parsed quads into a named graph
in one FFI call:

- `rdf_load_ntriples_to_graph(body TEXT, graph TEXT) → INTEGER`
- `rdf_load_turtle_to_graph(body TEXT, graph TEXT) → INTEGER`
- `rdf_load_rdfxml_to_graph(body TEXT, graph TEXT) → INTEGER`

`graph = NULL` means the default graph (identical to the 1-arg
loaders); `graph = '<iri>'`-style strings are rejected — pass the bare
IRI as the second argument, matching the 4-arg `rdf_insert(s, p, o,
graph)` convention from 0.3.0. Blank-node graph IRIs (`_:label`) are
rejected with the same `blank-node graphs are not supported` error
the 0.3.0 path raises, so consumer-side prefix-matching keeps working
unchanged.

The 1-arg loaders are byte-for-byte unchanged. The 2-arg form with
`NULL` produces the same store state as the 1-arg form — pinned by
`test_rdf_load_ntriples_to_graph_parser_parity`.

Driver: `CONSUMER_REQUIREMENT_RS.md` § "Requested extensions" item #1.
With this in place, items #1–#4 of that file graduate from "Requested"
to "SQL surfaces RS consumes."

## 0.5.0 — SPARQL UPDATE

Exposes Oxigraph's `Store::update` as a new scalar:

- `sparql_update(query) → INTEGER` — runs any SPARQL 1.1 UPDATE form
  (`INSERT DATA`, `DELETE DATA`, `INSERT { … } WHERE { … }`,
  `DELETE { … } WHERE { … }`, mixed modifies, `CLEAR`, `CREATE`,
  `DROP`, `LOAD`).

### Return value — important

Oxigraph 0.4's `Store::update` doesn't expose a first-class
affected-row count. `sparql_update` returns the **signed net change**
in store size, computed via `len()` before and after the call:

| UPDATE shape                            | Return value             |
|-----------------------------------------|--------------------------|
| `INSERT DATA { … }`                     | `+N` (newly inserted, post-dedup) |
| `DELETE DATA { … }`                     | `-N` (removed)           |
| `INSERT { … } WHERE { … }`              | `+N`                     |
| `DELETE { … } WHERE { … }`              | `-N`                     |
| mixed `DELETE/INSERT { … } WHERE { … }` | `inserts - deletes` (may be `0`) |
| `CLEAR DEFAULT` / `CLEAR ALL` / `CLEAR GRAPH <g>` | `-N`           |

A balanced mixed UPDATE returns `0` even though both halves ran.
When you need to assert *state*, use `rdf_count` / `sparql_ask` /
`sparql_query` instead of relying on the delta.

### Error classification

Errors are split into `ParseError` (Oxigraph's `EvaluationError::Parsing`
variant — bad SPARQL syntax) and `EvalError` (everything else —
graph-already-exists, unbound service, etc.). The resulting SQLite
error message is prefixed `SPARQL parse error: …` or
`SPARQL evaluation error: …` so downstream consumers can
pattern-match.

### Network safety

SPARQL 1.1 `LOAD <iri>` would make Oxigraph fetch the IRI over HTTP
from inside the database. The default Oxigraph build has no HTTP
support, so `LOAD` returns an evaluation error today. If you ever
build Oxigraph with HTTP enabled, sandbox the host process
accordingly — this is a deliberate non-mitigation in 0.5.0.

### Tests

Ten new integration tests (37 + 1 ignored, up from 27 + 1):
`test_sparql_update_insert_data`, `test_sparql_update_delete_data`,
`test_sparql_update_dedup_on_insert_data`,
`test_sparql_update_where_insert`,
`test_sparql_update_modify_mixed` (asserts store state, not delta,
for mixed ops), `test_sparql_update_named_graph`,
`test_sparql_update_clear_default`, `test_sparql_update_clear_all`,
`test_sparql_update_parse_error_surfaces`,
`test_sparql_update_evaluation_error_surfaces`.

## 0.4.0 — batched insert / delete

Adds `rdf_insert_many(json)` and `rdf_delete_many(json)` for writing
many triples in a single FFI crossing, collapsing the SQL-parse +
function-dispatch overhead of N separate `rdf_insert` calls down to
one.

### New SQL surface

- `rdf_insert_many(json) → INTEGER` — single JSON-array argument.
  Each row is `[s, p, o]` (default graph) or `[s, p, o, graph]`
  (named graph; `null` means default). Uses Oxigraph's `bulk_loader`
  internally. Returns the count of *newly* inserted quads; duplicates
  collapse under RDF set semantics and don't count.
- `rdf_delete_many(json) → INTEGER` — mirror. Per-row removal; no-ops
  (rows not present in the store) don't count toward the return value.

### Behaviour

- Empty array `'[]'` returns `0`, no error.
- Malformed input — non-array JSON, row of wrong arity, non-string
  element, invalid IRI, blank-node graph — aborts the *whole* batch
  before any write touches the store. Error messages include the
  failing row index (e.g. `row 7: subject: …`).
- Term encoding matches the single-row `rdf_insert(s, p, o)` parser
  exactly. Pinned by `test_insert_many_parser_parity_with_single`.

### Internal

- `store::{build_quad, parse_named_or_blank, parse_term,
  parse_graph_name}` are now `pub(crate)` so the bulk module reuses
  the single-row parser. This keeps the two write paths bit-identical
  in their handling of the term grammar (the risk the plan called
  out).

### Tests

Seven new tests (27 + 1 ignored perf-smoke, up from 20):
`test_insert_many_3_arg_rows`,
`test_insert_many_mixed_arities`,
`test_insert_many_dedup_return_value`,
`test_insert_many_malformed_aborts_batch`,
`test_insert_many_empty_array`,
`test_insert_many_parser_parity_with_single`,
`test_delete_many_partial`,
`test_insert_many_perf_smoke` (release-only, `#[ignore]` — run with
`cargo test --release -- --ignored insert_many_perf_smoke`; 1000-row
batch under 100 ms).

## 0.3.0 — named graphs

Adds named-graph support across the full SQL surface. All existing
zero- and three-argument signatures keep their 0.2.0 behaviour;
named-graph variants ride alongside.

### New SQL surface

- `rdf_insert(s, p, o, graph)` — 4-arg form routes into a named graph.
  `graph = NULL` is the default graph (same as the 3-arg form).
  Blank-node graphs (`_:…`) are rejected with a clear error.
- `rdf_delete(s, p, o, graph)` — mirror of insert.
- `rdf_count(graph)` — 1-arg form counts quads in a named graph;
  `NULL` is the default graph (same as `rdf_count()`).
- `rdf_count_all()` — counts across every graph, default included.
- `rdf_triples` virtual table now has a HIDDEN `graph` column:
  - `SELECT *` still returns three columns
  - `INSERT INTO triples VALUES (s, p, o)` still works (default graph)
  - `INSERT INTO triples(subject, predicate, object, graph) VALUES (…)`
    writes to a named graph
  - `WHERE graph = 'urn:g:…'` / `WHERE graph IS NULL` filter on graph

### SPARQL routing

SPARQL 1.1 `FROM <graph>`, `FROM NAMED <graph>`, and `GRAPH <graph> { … }`
clauses go straight through to Oxigraph — no extra plumbing needed.
The default dataset for an unqualified `?s ?p ?o` query remains the
default graph only; named-graph triples never leak in without an
explicit `FROM` or `GRAPH` clause (pinned by
`test_sparql_query_default_dataset_isolates`).

### Backward compatibility

Every 0.1.0 / 0.2.0 caller keeps working unchanged. The 3-arg forms,
zero-arg `rdf_count()`, and the 3-column `SELECT * FROM triples` /
`INSERT INTO triples VALUES (…)` shapes are unchanged in syntax and
semantics.

### Tests

Six new integration tests (20 total, up from 13):
`test_rdf_insert_4arg_named_graph`,
`test_rdf_delete_4arg_named_graph`,
`test_rdf_insert_4arg_rejects_blank_graph`,
`test_sparql_query_graph_clause`,
`test_sparql_query_default_dataset_isolates`,
`test_vtab_named_graph_round_trip`,
`test_vtab_default_graph_compat`.

## 0.2.0 — shared process-wide store

Replaces the per-thread Oxigraph store from 0.1.0 with a single
process-wide store wrapped in `OnceLock<Store>`. Every SQLite connection
on every thread now sees the same triple graph.

### Behaviour change

- A triple inserted on one SQLite connection is **visible** from every
  other connection in the same process, including connections on other
  threads. This is the headline fix for the "insert-on-thread-A-
  invisible-from-thread-B" footgun called out in
  `docs/reviews/REVIEW_0.1.0.md`.
- The SQL surface is unchanged. No function was renamed, added, or
  given a new signature in this release.
- `rdf_clear()` now empties the existing store in place (via
  `Store::clear`) rather than replacing it with a fresh instance. The
  observable behaviour is identical for callers (count → 0; subsequent
  inserts continue to work).
- Internal: `store::with_store_mut` was removed (it was always a
  misnomer — Oxigraph's `Store` mutates through `&self`). `with_store`
  takes its place at every call site. This is not a public API.

### Concurrency

Oxigraph 0.4's in-memory `Store` is internally concurrent — every
mutator takes `&self` and the storage layer uses `DashMap` plus
`RwLock` for synchronisation. The extension wraps the store in
`OnceLock` only for lazy initialisation; no additional `Mutex` or
`RwLock` is layered on top.

Downstream consumers like `rails-semantica` should be aware that
concurrent HTTP requests (Puma threads) can now interleave reads and
writes against the shared graph — which is the right correctness
story, but is a new concurrency surface compared to 0.1.0.

### Tests

- Dropped `test_thread_local_isolation` — it pinned the old, buggy
  invariant.
- Added `test_cross_thread_visibility` — proves the new invariant
  across threads.
- Added `test_shared_store_across_connections` — proves it across
  connections on the same thread.
- Added `serial_test` as a dev-dependency and marked every
  integration test `#[serial]`. The shared store means parallel tests
  would otherwise race; `cargo test` is now serialised at the
  integration-test layer only.

### Roadmap shift

PLAN_0.1.0 tentatively numbered "named graphs" as 0.2.0. That work
moves to **0.3.0** (`docs/plans/PLAN_0.3.0.md`); batched insert
(`rdf_insert_many`) was newly broken out as **0.4.0**
(`docs/plans/PLAN_0.4.0.md`). The MM consumer document
(`CONSUMER_REQUIREMENT_MM.md`) has been re-labelled to match.

## 0.1.0 — first green build

Initial release. SQLite loadable extension embedding the Oxigraph RDF/SPARQL
engine. The thread-local Oxigraph store is in-memory and resets when the
thread exits.

### SQL surface

- Scalar functions: `rdf_insert`, `rdf_delete`, `rdf_clear`, `rdf_count`,
  `rdf_load_turtle`, `rdf_load_ntriples`, `rdf_load_rdfxml`,
  `rdf_dump_ntriples`, `rdf_term_type`, `rdf_term_value`.
- SPARQL: `sparql_query` (SELECT → JSON), `sparql_ask` (ASK → 0/1),
  `sparql_construct` (CONSTRUCT → N-Triples).
- Virtual table: `rdf_triples` — read scans the default graph; INSERT
  writes through to the store. DELETE and UPDATE on the vtab are not
  supported in 0.1.x (use `rdf_delete(s,p,o)` or a SPARQL DELETE).

### Scope

- RDF 1.1 only — RDF-star quoted triples are rejected with a clear error.
- All triples live in the default graph; named graphs land in 0.2.0.
- In-memory store only; the persistent RocksDB backend lands in 0.4.0.

### Known limitations

- Thread-local store: Rails 8's SQLite connection pool reuses threads, so
  each pooled thread sees its own store. Acceptable for the in-memory
  build; revisit when the persistent backend lands.
