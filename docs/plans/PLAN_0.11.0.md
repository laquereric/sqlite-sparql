# PLAN 0.11.0 — Native SHACL Core validator pass

> Add `rdf_shacl_core_validate(data_iri TEXT, shapes_iri TEXT,
> report_iri TEXT, options_json TEXT) → INTEGER` — a native Rust
> SHACL Core validator that walks the data graph once per shape,
> evaluates the per-property constraints, and emits a W3C-conformant
> `sh:ValidationReport` graph. Replaces `vv-graph`'s per-constraint
> per-focus-node `sparql_query` round-trip with one FFI crossing.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions"
item **#7 — Native SHACL Core validator pass**. VG's
`Vv::Graph::Shacl.validate!` (PLAN_0.10.0 Phase B already shipped
on the gem side, commit `ed55ef4`) currently issues one
`sparql_ask` or `sparql_query` per constraint per focus-node —
O(focus_nodes × constraints × shapes). The native validator walks
the store once per shape and batches the constraint checks.

VG posture (same as PLAN_0.9.0): "revisit on first concrete
bottleneck signal, not schedule a release." Forward-leaning ship.

Depends on 0.3.0 (named-graph plumbing), 0.7.0 (RDF-star —
provenance annotations on report entries use
`<< _:v sh:resultMessage "…" >> :reportedBy :Shape_X`), 0.9.0
(the surface pattern of `*_materialise` / `*_validate` scalars
with options-JSON, signed-int return, fixed-prefix error envelopes
— this plan reuses the shape).

---

## Goal

`cargo test` passes a round-trip test that:

1. Loads a 5-triple SHACL shapes graph (one `sh:NodeShape` with
   `sh:targetClass` + 3 `sh:property` blocks covering
   `sh:minCount`, `sh:datatype`, `sh:pattern`).
2. Loads a 6-triple data graph (3 instances of the target class,
   one of which violates each constraint).
3. Calls `rdf_shacl_core_validate('urn:g:data', 'urn:g:shapes',
   'urn:g:report', '{}')`.
4. Asserts the return integer equals the violation count (3
   violations, one per constraint).
5. Asserts the report graph contains a W3C-conformant
   `sh:ValidationReport` with three `sh:ValidationResult` nodes,
   each carrying the expected `sh:focusNode`, `sh:resultPath`,
   `sh:sourceShape`, `sh:sourceConstraintComponent`,
   `sh:resultSeverity`, and `sh:resultMessage`.

Plus an **equivalence test** against VG's `Vv::Graph::Shacl.validate!`
output for the same input — `sameTerm`-equivalent report graphs
under permutation. The blank-node naming in the report is the
gotcha (VG and the engine will choose different `_:v123` IDs);
either normalise blank-node labels before comparison or compare
via SPARQL `MINUS` over the predicate-structured shape (every
`sh:focusNode` / `sh:resultPath` pair matches).

---

## What 0.11.0 covers vs. doesn't

VG's `Vv::Graph::Shacl::ConstraintLibrary` ships 12 constraint
components (per the CHANGELOG / `Constraints::PHASE_B_PENDING`
list). 0.11.0 ships parity:

| Status | Constraints |
|---|---|
| **0.11.0 in scope** | `sh:minCount`, `sh:maxCount`, `sh:datatype`, `sh:nodeKind`, `sh:class`, `sh:pattern`, `sh:minLength`, `sh:maxLength`, `sh:in`, `sh:hasValue`, `sh:minInclusive`, `sh:maxInclusive` |
| **0.12.0+ (out of scope)** | The remaining ~18 SHACL Core constraint components in VG's `PHASE_B_PENDING` |

Same posture as PLAN_0.10.0: engine coverage tracks VG coverage
lockstep. If VG expands first, the engine bumps next. If VG
shrinks (unlikely), the engine narrows next.

---

## Why a separate scalar, not `sparql_query` ergonomics

The CR ask is explicit:

> A native Rust pass that evaluates SHACL Core constraints
> against a data graph in place of the gem's per-constraint /
> per-focus-node `sparql_query` round-trip. The pass produces a
> W3C-conformant `sh:ValidationReport` graph as output.

Three reasons (same shape as PLAN_0.9.0's "why a separate scalar"):

- **Different cost profile.** The native validator amortises the
  per-shape store walk across N constraints, where `sparql_query`
  re-walks per constraint. Engine-side batching is the whole
  point.
- **Different return contract.** `rdf_shacl_core_validate` writes
  the report graph to a named graph and returns a violation
  count. `sparql_query` returns a JSON binding array. Different
  shapes; different names.
- **Different provenance posture.** The validator emits provenance
  on report entries (`:reportedBy <Shape>`, `:reportedAt …`) in
  the same shape as `rdf_owl_rl_materialise` (0.9.0). Defaults
  match VG.

---

## Phase A — surface scaffolding

New module `src/functions/rdf_shacl_core.rs` plus
`src/functions/rdf_shacl_core/` subdirectory for constraints +
path evaluator.

```rust
#[derive(Deserialize)]
pub(crate) struct ValidateOptions {
    #[serde(default = "default_max_violations")]
    pub max_violations: usize,            // safety guard
    #[serde(default)]
    pub provenance: bool,
    #[serde(default = "default_reported_by_iri")]
    pub reported_by_iri: String,
    #[serde(default = "default_reported_at_iri")]
    pub reported_at_iri: String,
    #[serde(default = "default_shape_iri_prefix")]
    pub shape_iri_prefix: String,
}

fn default_max_violations() -> usize { 10_000 }
// ... defaults match vv-graph's Shacl convention
```

`rdf_shacl_core_validate(data_iri, shapes_iri, report_iri,
options_json) → INTEGER`. `data_iri = NULL` → default graph
(consistent with `rdf_owl_rl_materialise`). `shapes_iri = NULL`
or `report_iri = NULL` is rejected with the same fixed-prefix
error shape as 0.9.0.

Register, return early with a stub error, ship Phase A.

### Exit criteria for Phase A

`cargo build` clean. Function visible in `.functions` introspection.

---

## Phase B — constraint library (12 components)

Add `src/functions/rdf_shacl_core/constraints.rs` with one
function per constraint component:

```rust
pub(crate) struct Constraint {
    pub iri: &'static str,
    pub evaluate: fn(&Store, &FocusContext, &PropertyShape) -> Result<Vec<Violation>>,
}

pub(crate) static CONSTRAINTS: &[Constraint] = &[
    Constraint { iri: "http://www.w3.org/ns/shacl#minCount",     evaluate: eval_min_count     },
    Constraint { iri: "http://www.w3.org/ns/shacl#maxCount",     evaluate: eval_max_count     },
    // ... 12 entries
];
```

Each `eval_*` function:

- Takes the focus node, the property path's resolved values, and
  the constraint parameter (e.g., the integer for `sh:minCount`).
- Returns a `Vec<Violation>` (zero for conforming, one per failing
  binding for cardinality constraints, one per non-conforming
  value for per-value constraints).

`Violation` is a struct carrying the W3C-spec fields
(`focus_node`, `result_path`, `value`, `source_constraint_component`,
`result_severity`, `result_message`).

### Exit criteria for Phase B

`cargo build` clean. Per-constraint unit tests in
`constraints.rs` exercise each component against a 2–3-quad
fixture.

---

## Phase C — path evaluator

SHACL property paths (`sh:path`) include:

- **Predicate paths** (the common case): `sh:path :p` → match
  `(focus, :p, ?value)` quads.
- **Inverse paths**: `sh:path [ sh:inversePath :p ]` → match
  `(?value, :p, focus)`.
- **Sequence paths**: `sh:path ( :p1 :p2 )` — RDF-list-of-paths,
  composed.
- **Alternative paths**: `sh:path [ sh:alternativePath ( :p1 :p2 ) ]`.
- **Zero-or-more / one-or-more / zero-or-one**: `sh:zeroOrMorePath`,
  `sh:oneOrMorePath`, `sh:zeroOrOnePath`.

`src/functions/rdf_shacl_core/path.rs`:

```rust
pub(crate) enum Path {
    Predicate(NamedNode),
    Inverse(Box<Path>),
    Sequence(Vec<Path>),
    Alternative(Vec<Path>),
    ZeroOrMore(Box<Path>),
    OneOrMore(Box<Path>),
    ZeroOrOne(Box<Path>),
}

impl Path {
    pub fn evaluate(&self, store: &Store, focus: &Subject, graph: &GraphName) -> Vec<Term> { ... }
    pub fn parse(store: &Store, path_node: &Term, graph: &GraphName) -> Result<Path> { ... }
}
```

The parse step reads the RDF representation of the path
(predicate IRI directly, or a blank-node-headed structure for
non-predicate paths) and builds the `Path` AST. The evaluate
step walks the store.

### Exit criteria for Phase C

Path evaluator handles all six path types listed above. Unit
tests for each path shape.

---

## Phase D — shape parser + validator driver

`src/functions/rdf_shacl_core/validator.rs`:

1. **Enumerate shapes**: query the shapes graph for every
   `sh:NodeShape` and `sh:PropertyShape`. Build a `Vec<Shape>`
   in memory.
2. **For each shape**:
   a. Resolve targets via `sh:targetClass` / `sh:targetNode` /
      `sh:targetSubjectsOf` / `sh:targetObjectsOf`. Yields a
      `HashSet<Subject>` of focus nodes.
   b. For each property shape (`sh:property` block), evaluate its
      `sh:path` against each focus node to get the value bindings.
   c. For each constraint in the property shape, call
      `eval_<constraint>(store, focus, &property_shape) → Vec<Violation>`.
3. **Collect violations** into a `Vec<Violation>`.
4. **Emit the report graph**: for each violation, generate a
   `_:v<n>` blank node with the W3C-spec predicates, write into
   `report_iri`. Plus a single `_:report sh:conforms false`
   header (or `true` when zero violations).
5. **Return** the violation count (signed-int convention from
   PLAN_0.9.0).

The report graph is cleared before writing (so re-validation
overwrites). Document this in the surface contract.

### Exit criteria for Phase D

End-to-end smoke test via the SQLite CLI: load shapes + data,
validate, dump the report graph, see W3C-conformant structure.

---

## Phase E — integration tests

Add to `tests/integration_test.rs` under a
`// ── 0.11.0 rdf_shacl_core_validate ──` banner. Pattern matches
the 0.9.0 banner.

### Tests

1. `test_rdf_shacl_core_validate_min_count_violation` — one shape
   with `sh:minCount 1`, focus node with zero values → exactly
   one violation.
2. `test_rdf_shacl_core_validate_datatype_violation` — focus node
   with a literal of wrong datatype.
3. `test_rdf_shacl_core_validate_full_shape_round_trip` — the
   §Goal fixture: 5-triple shapes graph + 6-triple data graph →
   3 violations + W3C-conformant report.
4. `test_rdf_shacl_core_validate_conforms_when_no_violations` —
   single conforming instance, return 0, report has
   `sh:conforms true`.
5. `test_rdf_shacl_core_validate_max_violations_guard` —
   `max_violations: 1` against multi-violation input → error
   with fixed-prefix.
6. `test_rdf_shacl_core_validate_data_iri_default_graph` —
   `data_iri = NULL` uses the default graph.
7. `test_rdf_shacl_core_validate_shapes_iri_required` — NULL
   shapes_iri rejected.
8. `test_rdf_shacl_core_validate_report_iri_required` — NULL
   report_iri rejected.
9. `test_rdf_shacl_core_validate_clears_report_on_rewrite` —
   second call into the same report_iri clears the prior report.
10. `test_rdf_shacl_core_validate_path_inverse` — exercises the
    `sh:inversePath` form.
11. `test_rdf_shacl_core_validate_path_sequence` — exercises a
    2-step sequence path.
12. `test_rdf_shacl_core_validate_equivalence_with_vg` — the big
    one. Hand-written expected report (or run VG separately and
    diff via SPARQL `MINUS` modulo blank-node labels).

### Exit criteria for Phase E

```
cargo test                # all green
cargo build --release && cargo test --release    # see CLAUDE.md footgun
```

Test count climbs by ~12.

---

## Phase F — docs + CR graduation

Same shape as PLAN_0.9.0 Phase F:

- **`README.md`** — new "SHACL Core validation (since 0.11.0)"
  section between the OWL 2 RL section and `Bulk Load (Turtle)`.
  Roadmap checkbox.
- **`CLAUDE.md`** — new subsection under "Reasoning" (rename to
  "Reasoning / validation"?) with the function signature,
  options, error envelopes.
- **`CHANGELOG.md`** — 0.11.0 entry leading with the FFI-collapse
  framing, the 12-constraint coverage, the path-evaluator
  capability, the W3C-conformant report contract.
- **`CONSUMER_REQUIREMENT_VvGraph.md`** — item #7 graduates from
  "Requested" to live "SPARQL querying" / "Validation" table
  row.
- **`CONSUMER_REQUIREMENT_MM.md`** — add line under "Available
  upstream but not exercised by MM".
- **`PLAN_0.2.0.md`** — roadmap renumber.

---

## Phase G — tag

Same as PLAN_0.9.0 / PLAN_0.9.1. **Chain `git add … && git
commit …` in a single bash invocation** to prevent the
publication-fix incident from recurring.

---

## Risks

- **Property paths are non-trivial.** The path evaluator is the
  single biggest code path in 0.11.0 — easy to ship a subset
  that handles predicate paths only and defer the recursive
  forms. If telemetry doesn't justify the full path grammar,
  scope-cut to predicate + inverse + sequence; defer ZeroOrMore
  / OneOrMore / ZeroOrOne / Alternative to 0.12.0 alongside the
  constraint expansion.
- **Blank-node naming in the report graph.** VG and the engine
  will produce different `_:vN` labels. The equivalence test
  needs blank-node-renaming awareness — either canonical
  normalisation (Hogan canon, more code) or shape-only
  comparison (SPARQL `MINUS` on the W3C-spec predicates,
  treating blank nodes as existentials). Pick the latter for
  0.11.0; consider a Hogan-canon helper if cross-store
  comparison becomes a separate need.
- **Report graph rewrite semantics.** `report_iri` is **cleared**
  before each validate call. This is a write side-effect that
  contradicts the "engine emits, consumer decides where it
  lands" posture from 0.8.0. Defensible because the report is
  the validate call's own output, not a derivation from
  consumer-managed state. Document loudly.
- **VG-side blank-node-vs-named-node choice for shape IRIs.**
  Shapes can be either. The shape-IRI-prefix option (for
  `:sourceShape` provenance) only applies when shapes are named
  nodes; blank-node shapes get a synthesised stable identifier
  (e.g., `_:bN` rewritten as `<urn:engine:shape:bN>` for
  provenance). Document or punt.

---

## Out of scope

- **SHACL-SPARQL constraints** (`sh:sparql`) — these are
  arbitrary embedded SPARQL queries that act as constraints.
  Out of scope for 0.11.0 (different evaluation model — falls
  back to the `sparql_query` round-trip anyway).
- **SHACL Rules** (`sh:rule`) — that's
  `Vv::Graph::Shacl::Rules.materialise!`, which routes through
  the 0.8.0 `rdf_construct_many` surface gem-side. No engine work
  needed.
- **The remaining ~18 SHACL Core constraints in VG's
  `PHASE_B_PENDING`** — same posture as PLAN_0.10.0. Expand
  lockstep with VG.
- **SHACL Advanced features** (`sh:function`, `sh:expression`,
  `sh:rule`-with-`sh:condition`) — not in the SHACL Core profile.

---

## Re-numbering downstream milestones

If 0.11.0 ships with the 12-constraint subset matching VG:

| Version | Topic |
|---|---|
| 0.11.0 | Native SHACL Core validator (12-constraint subset) — this file |
| 0.12.0 | Native DRed dependency index (VG CR #8) |
| 0.13.0 | Full SHACL Core coverage (remaining ~18 constraints) — if VG expands |
| 0.14.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.15.0 | SPARQL HTTP endpoint |

If 0.11.0 splits across sub-releases (predicate paths first,
recursive paths later), the DRed plan slides right by however
many slots that consumes. Update `PLAN_0.2.0.md` at ship time.
