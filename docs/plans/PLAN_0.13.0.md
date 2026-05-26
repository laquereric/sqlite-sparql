# PLAN 0.13.0 — OWL 2 RL inconsistency detection

> Add `rdf_owl_rl_consistent(asserted_iri TEXT, inferred_iri TEXT,
> options_json TEXT) → TEXT` — a native Rust pass evaluating the
> 17 W3C OWL 2 RL/RDF *inconsistency* rules that PLAN_0.10.0
> deliberately deferred. Returns a JSON array of
> `{rule, s, p, o}` violation records (or `[]` for consistent),
> paralleling SHACL's `sh:ValidationReport` shape. Stays out of
> `rdf_owl_rl_materialise`'s monotonic-derivation contract.

Forward-leaning ship per the project's pattern. PLAN_0.10.0
explicitly nominated 0.13.0 as the inconsistency-detection slot
("revisit on first concrete bottleneck signal, or simply when
the SHACL Core + DRed work in front of it has landed" —
both shipped in 0.11.0 / 0.12.0). No consumer signal from
Vv::Graph today (`Vv::Graph::Reasoner.consistent?` does not yet
exist), but the surface gap is the next obvious substrate move:
the OWL 2 RL semantics is incomplete without inconsistency, and
shipping it now means VG can route through whenever it grows
a consistency check.

Depends on 0.3.0 (named-graph plumbing), 0.7.0 (RDF-star — used
only for round-tripping any annotations the consumer left on
premise quads), and 0.10.0 (the rule-library shape and helper
inventory — every inconsistency rule reuses one of `pairs_for_
predicate`, `all_quads`, `instances_of`, or the small subclass-
walk helper).

---

## Goal

`cargo test` passes a round-trip test that:

1. Loads a 6-triple T-Box+A-Box that triggers exactly one
   `cax-dw` violation (`:alice rdf:type :Animal, :Vegetable`
   where `:Animal owl:disjointWith :Vegetable`).
2. Calls `rdf_owl_rl_consistent(NULL, 'urn:g:inferred', '{}')`.
3. Parses the JSON return as `Vec<ViolationRecord>` and asserts
   exactly one record with `rule = "cax-dw"`, `s = "<urn:alice>"`,
   `p = "<rdf:type>"`, `o = "<urn:Animal>"` (or `:Vegetable` —
   `cax-dw` fires symmetrically; either witness is acceptable
   and the engine commits to "first match wins" deterministically).
4. Asserts the store is unchanged (`rdf_count_all()` before ==
   after — inconsistency detection is read-only).

Plus per-rule smoke tests for every one of the 17 rules,
patterned after `tests/integration_test.rs`'s OWL 2 RL Phase E
banner (one minimal positive case per rule).

---

## What 0.13.0 covers vs. doesn't

The 17 W3C OWL 2 RL inconsistency rules, grouped as PLAN_0.10.0's
Table 9 lays them out:

| Group | Rules | Notes |
|---|---|---|
| **Prp** (6) | `prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1`, `prp-npa2` | Negative property assertions + irreflexive / asymmetric / disjoint predicates |
| **Cls** (5) | `cls-nothing2`, `cls-com`, `cls-maxc1`, `cls-maxqc1`, `cls-maxqc2` | `rdf:type owl:Nothing` + class complement / max-cardinality-zero contradictions |
| **Cax** (2) | `cax-dw`, `cax-adc` | Class disjointness (pair + n-ary) |
| **Eq** (3) | `eq-diff1`, `eq-diff2`, `eq-diff3` | `owl:differentFrom` + `owl:AllDifferent` |
| **Dt** (1) | `dt-not-type` | Literal value does not match its declared datatype |

| Status | Item |
|---|---|
| **0.13.0 in scope** | All 17 rules above; the dispatch loop; the JSON return shape; `max_violations` safety guard (matches 0.11.0); first-witness-wins per-rule determinism |
| **0.13.0 out of scope** | Repair-suggestion records (W3C SHACL `sh:resultMessage` analogue); a SHACL-style `report_iri TEXT` write-into-graph mode (a future option flag could add it — see "Risks"); `rdf:PlainLiteral` / `rdf:langString` lexical-form validation (treated as the same constant set as `dt-type1`); cross-rule deduplication when two rules report the same violation triple (each rule emits independently) |

`prp-ap` (annotation properties) stays excluded across the rule
library — same posture as 0.10.0.

`dt-not-type` is the dual concern of `dt-eq` / `dt-diff` in
0.10.0: Oxigraph 0.4's `Subject` enum can't carry `Literal`,
but `dt-not-type` doesn't need a literal subject — it
reports a violation when a literal's lexical form fails to
parse against its datatype IRI. Implementable in 0.13.0;
re-use `oxigraph::model::Literal::value()` + a small
type-dispatched parse table. Documented separately from the
0.10.0 limitation in the function docstring.

---

## Why a separate scalar, not a `materialise` option

Three options revisited from PLAN_0.10.0 §"Inconsistency rules —
deferred":

1. **Marker triple per inconsistency** — rejected for the same
   reason as in 0.10.0: it conflates monotonic derivation
   delta with a non-monotonic semantic verdict.
2. **SQLite error on first inconsistency** — rejected: aborts
   the call; consumer can't see the full set in one round-trip.
3. **Separate scalar returning JSON** — picked. Matches
   `rdf_construct_many`'s shape (JSON-array return for a
   bounded set of records). Honest contract split.

`rdf_owl_rl_consistent` parallels `rdf_shacl_core_validate`
shape-wise: both are read-only passes that report violations.
They diverge on output (JSON return vs report-graph write):
SHACL ships a standardised report-graph format that consumers
expect to query; OWL 2 RL inconsistency has no equivalent W3C
canonical report graph, so the JSON envelope is the cheaper
move. A future option flag (`{"report_iri": "urn:g:report"}`)
could add a graph-write mode if a consumer asks for SPARQL
queryability over the violations. Not in 0.13.0.

---

## Return-shape contract

JSON array of objects, one per violation:

```json
[
  {
    "rule": "cax-dw",
    "s": "<http://example.org/alice>",
    "p": "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>",
    "o": "<http://example.org/Animal>"
  },
  {
    "rule": "prp-npa1",
    "s": "<http://example.org/alice>",
    "p": "<http://example.org/married>",
    "o": "<http://example.org/bob>"
  }
]
```

`s` / `p` / `o` use the same N-Triples-style serialisation as
`rdf_term_value`'s inverse (the existing `term_to_nt_string`
helper in `rdf_owl_rl/rules.rs`'s tracked variants). Blank
nodes round-trip as `_:b123`; literals as `"text"^^<dt>`;
quoted-triple terms as `<< s p o >>` (RDF-star
since 0.7.0 — `prp-npa*` can take a triple-term object).

`[]` (an empty array) is returned when the store is consistent.
This is the consumer's "no problems found" signal — distinct
from any error.

**Per-rule witness convention.** Each rule emits one record per
violation witness it finds. `cax-dw` with `:alice` typed as both
`:Animal` and `:Vegetable` fires once (the witness is the
asymmetric pair; the engine commits to the lexicographically
smaller object IRI for determinism). `prp-asyp` with a symmetric
pair `(a, p, b) + (b, p, a)` and `p` declared asymmetric emits
**one** record with `s/p/o = (a, p, b)` — the same lexicographic
rule. Documented in each rule's docstring.

---

## Phase A — per-rule violation detection

`src/functions/rdf_owl_rl/inconsistency.rs` (new module):

```rust
pub(crate) struct InconsistencyRule {
    pub iri: &'static str,
    pub detect: fn(&Store, &GraphName, &GraphName)
        -> Result<Vec<ViolationRecord>>,
}

pub(crate) struct ViolationRecord {
    pub rule: &'static str,
    pub s: String,
    pub p: String,
    pub o: String,
}

pub(crate) static INCONSISTENCY_RULES: &[InconsistencyRule] = &[
    InconsistencyRule { iri: "prp-irp",      detect: detect_prp_irp },
    InconsistencyRule { iri: "prp-asyp",     detect: detect_prp_asyp },
    InconsistencyRule { iri: "prp-pdw",      detect: detect_prp_pdw },
    InconsistencyRule { iri: "prp-adp",      detect: detect_prp_adp },
    InconsistencyRule { iri: "prp-npa1",     detect: detect_prp_npa1 },
    InconsistencyRule { iri: "prp-npa2",     detect: detect_prp_npa2 },
    InconsistencyRule { iri: "cls-nothing2", detect: detect_cls_nothing2 },
    InconsistencyRule { iri: "cls-com",      detect: detect_cls_com },
    InconsistencyRule { iri: "cls-maxc1",    detect: detect_cls_maxc1 },
    InconsistencyRule { iri: "cls-maxqc1",   detect: detect_cls_maxqc1 },
    InconsistencyRule { iri: "cls-maxqc2",   detect: detect_cls_maxqc2 },
    InconsistencyRule { iri: "cax-dw",       detect: detect_cax_dw },
    InconsistencyRule { iri: "cax-adc",      detect: detect_cax_adc },
    InconsistencyRule { iri: "eq-diff1",     detect: detect_eq_diff1 },
    InconsistencyRule { iri: "eq-diff2",     detect: detect_eq_diff2 },
    InconsistencyRule { iri: "eq-diff3",     detect: detect_eq_diff3 },
    InconsistencyRule { iri: "dt-not-type",  detect: detect_dt_not_type },
];
```

Each `detect_*` mirrors the corresponding derivation rule's
helper shape (`pairs_for_predicate`, `all_quads`, `instances_of`)
but emits `ViolationRecord` instead of `Triple`. The 17
functions average ~30 lines each (a handful of joins + a witness
selection); the module weighs in around 500–600 lines of Rust.

**Helper reuse.** The new module imports from
`rdf_owl_rl/rules.rs` the helpers that already exist
(`pairs_for_predicate`, `all_quads`, `instances_of`,
`subclass_closure_walk` — used by `cls-nothing2` which needs to
chase `rdf:type` through `rdfs:subClassOf*`). Add a tiny
`term_to_nt_string` formatter shared between this module and the
materialise's provenance encoder; today both inline a slightly
different formatter — consolidate as part of this work.

### Exit criteria for Phase A

`cargo build` clean. Module compiles, dispatch table well-typed.
Per-rule unit tests in the module (cheap fresh-store probes —
mirrors `rules.rs`'s existing `mod tests`).

---

## Phase B — dispatch loop + SQL scalar

`src/functions/rdf_owl_rl_consistent.rs` (sibling to
`rdf_owl_rl.rs`):

```rust
pub fn rdf_owl_rl_consistent_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let asserted = arg_text_or_null(values.get(0).expect("asserted_iri"))?;
    let inferred = arg_text_or_null(values.get(1).expect("inferred_iri"))?;
    let options_json = arg_text_or_null(values.get(2).expect("options_json"))?
        .unwrap_or("{}");

    let inferred = inferred.ok_or_else(|| {
        Error::new_message(
            "rdf_owl_rl_consistent: inferred_iri must be a named graph \
             (NULL is not allowed for the inferred slot)",
        )
    })?;

    let opts: ConsistentOptions = parse_options(options_json)?;
    let asserted_g = parse_graph_name(asserted)?;
    let inferred_g = parse_graph_name(Some(inferred))?;

    let json = with_store(|store| -> Result<String> {
        let mut all: Vec<ViolationRecord> = Vec::new();
        for rule in inconsistency::INCONSISTENCY_RULES {
            if all.len() >= opts.max_violations {
                return Err(SparqlError::EvalError(format!(
                    "rdf_owl_rl_consistent: violation count exceeded \
                     max_violations ({})",
                    opts.max_violations
                )));
            }
            let mut found = (rule.detect)(store, &asserted_g, &inferred_g)?;
            // Truncate per-rule output so we don't blow past the cap mid-rule.
            let remaining = opts.max_violations.saturating_sub(all.len());
            found.truncate(remaining);
            all.extend(found);
        }
        Ok(serde_json::to_string(&all).expect("violation vec → JSON"))
    })
    .map_err(sqlite_loadable::Error::from)?;

    api::result_text(context, &json)?;
    Ok(())
}
```

### `ConsistentOptions`

```rust
#[derive(Deserialize, Debug)]
struct ConsistentOptions {
    #[serde(default = "default_max_violations")]
    max_violations: usize,
}
fn default_max_violations() -> usize { 10_000 }
```

Same default and same fixed-prefix error envelope as
`rdf_shacl_core_validate`'s 0.11.0 surface — pinned for consumer
pattern-matching.

### Error envelopes (fixed-prefix)

- `rdf_owl_rl_consistent: inferred_iri must be a named graph …`
- `rdf_owl_rl_consistent: options_json: <serde error>`
- `rdf_owl_rl_consistent: violation count exceeded max_violations (N)`
- `rdf_owl_rl_consistent: rule <id> error: <message>`

### Exit criteria for Phase B

`cargo build` clean. Function registered in `lib.rs`. Smoke
test through the SQLite CLI returns `[]` for an empty store and
a well-shaped JSON array for a contrived inconsistency.

---

## Phase C — integration tests

Add to `tests/integration_test.rs` under a new
`// ── 0.13.0 rdf_owl_rl_consistent ──` banner:

1. `test_rdf_owl_rl_consistent_empty_store_returns_array` —
   sanity: returns `"[]"` on an empty store.
2. `test_rdf_owl_rl_consistent_no_violations` — load a few
   coherent triples → `"[]"`.
3. `test_rdf_owl_rl_consistent_cax_dw_single_violation` — the
   plan's headline example (`:alice` typed as two disjoint
   classes).
4. `test_rdf_owl_rl_consistent_eq_diff1_violation` — two terms
   declared `owl:differentFrom` but later inferred `owl:sameAs`.
5. `test_rdf_owl_rl_consistent_prp_npa1_violation` — explicit
   negative property assertion that's nonetheless asserted in
   the graph.
6. `test_rdf_owl_rl_consistent_prp_irp_violation` — irreflexive
   property used reflexively.
7. `test_rdf_owl_rl_consistent_prp_asyp_violation` — asymmetric
   property used symmetrically.
8. `test_rdf_owl_rl_consistent_cls_nothing2_violation` —
   `:x rdf:type owl:Nothing`.
9. `test_rdf_owl_rl_consistent_cls_com_violation` — `:x` typed
   as both a class and its complement.
10. `test_rdf_owl_rl_consistent_cls_maxc1_violation` — a property
    with `owl:maxCardinality 0` that nonetheless has an instance.
11. `test_rdf_owl_rl_consistent_dt_not_type_violation` — literal
    `"notanint"^^xsd:integer`.
12. `test_rdf_owl_rl_consistent_multiple_violations_distinct_rules`
    — one graph that triggers `cax-dw` *and* `prp-irp`; assert
    the JSON array length is 2 and the `rule` values are the two
    expected.
13. `test_rdf_owl_rl_consistent_max_violations_guard` — a graph
    with 5 violations, called with `max_violations: 2` → fixed-
    prefix error.
14. `test_rdf_owl_rl_consistent_read_only` — assert
    `rdf_count_all()` before == after the consistent call (no
    triple emitted).
15. `test_rdf_owl_rl_consistent_inferred_iri_required` — NULL
    inferred_iri → fixed-prefix error.

Additional per-rule smoke tests for the remaining rules
(`prp-pdw`, `prp-adp`, `prp-npa2`, `cls-maxqc1`, `cls-maxqc2`,
`cax-adc`, `eq-diff2`, `eq-diff3`) — keep them brief; one
positive case each. Final test count ~22–23 new entries.

### Exit criteria for Phase C

```
cargo test                                    # all green
cargo build --release && cargo test --release # see CLAUDE.md footgun
```

---

## Phase D — docs

- **`README.md`** — new "OWL 2 RL inconsistency detection (since
  0.13.0)" subsection alongside the SHACL Core section.
- **`CLAUDE.md`** — replace the "DEFERRED" status on the
  `rdf_owl_rl_consistent` entry (currently item #7 in the
  numbered "Completing the Implementation" list) with the LANDED
  shape; add a SQL function entry under "Reasoning / validation."
- **`CHANGELOG.md`** — 0.13.0 entry leading with the
  "JSON-violation-records" framing, the per-rule witness
  convention, the `max_violations` guard, and the deliberate
  contract split from `rdf_owl_rl_materialise`.
- **`CONSUMER_REQUIREMENT_VvGraph.md`** — currently no item;
  add one as "LANDED in 0.13.0 (no `Reasoner.consistent?`
  caller yet)" so the consumer-side gem can flip on whenever
  it ships.
- **`CONSUMER_REQUIREMENT_MM.md`** — add line under "available
  upstream but not exercised by MM."
- **`PLAN_0.2.0.md`** — strike the "deferred from 0.10.0"
  annotation on the 0.13.0 row; the row stays.

---

## Phase E — tag

Same as PLAN_0.11.0 / PLAN_0.12.0. Single chained
`git add … && git commit …` invocation, push to origin
when authorised.

---

## Risks

- **Symmetric-rule double-reporting.** Many inconsistency rules
  fire on a witness pair `(a, b)`. The naive implementation
  would emit two records — `(a, p, b)` and `(b, p, a)` — for a
  single semantic violation. Mitigation: lexicographic-order
  witness selection (smaller IRI/blank-node first). Pinned in
  each rule's docstring + dedicated `test_*_no_double_reporting`
  test for `cax-dw`, `prp-asyp`, `eq-diff1`.
- **`prp-npa1` / `prp-npa2` shape.** Negative property
  assertions live in the data graph as
  `_:b owl:sourceIndividual :a ; owl:assertionProperty :p ;
  owl:targetIndividual :b` (or `owl:targetValue "literal"`).
  Parsing this requires walking the `owl:NegativePropertyAssertion`
  structure — three joined patterns per assertion. The detection
  function will be the longest single rule (~60 lines). Mitigation:
  follows the same pattern as `cls-int1` / `cls-uni` in
  `rules.rs`, which already walk multi-triple OWL structures.
- **`cls-maxqc1` / `cls-maxqc2`.** Qualified-cardinality
  inconsistency requires evaluating `owl:onClass` membership for
  the witness instances — i.e. a subclass-closure walk inside the
  rule. Mitigation: extract the existing closure helper from
  `cls-maxqc3` / `cls-maxqc4` in 0.10.0 into a shared utility.
- **`eq-diff2` / `eq-diff3`.** `owl:AllDifferent` is an n-ary
  predicate carried via `rdf:List` on `owl:distinctMembers` (or
  the deprecated `owl:members`). The rule must walk both list
  encodings, then assert pairwise distinctness — and pairwise
  `owl:sameAs` membership negates that. The list walker exists
  (`rdf_owl_rl/rdf_lists.rs` from 0.10.0); reuse it.
- **`dt-not-type` and Oxigraph's literal API.** Oxigraph 0.4
  exposes `Literal::value()` and `Literal::datatype()`. The
  parse table needs to cover the same XSD set as `dt-type1`
  (~25 datatypes). For non-XSD datatypes, no validation — the
  consumer datatype is opaque to OWL 2 RL.
- **Witness ordering across runs.** `HashMap` iteration order
  is non-deterministic. Per-rule output must be sorted (by
  rule iri, then by `(s, p, o)` lexicographic) before being
  appended to the result vector — otherwise consumers see
  shuffled JSON between runs. Pin this in a
  `test_rdf_owl_rl_consistent_ordering_stable` test.
- **`max_violations` truncation semantics.** If `cax-dw` alone
  blows past the cap, do we error or truncate at the cap and
  succeed? PLAN_0.11.0's SHACL surface errors. Match that —
  consistency is one of those things you want to know is
  complete or know it's not.
- **No SPARQL access to the violations.** Returning JSON means
  consumers must `JSON_EACH` to query the violations from SQL.
  A future `report_iri` option could write a graph instead.
  Defer until a consumer asks.

---

## Out of scope

- **Repair-suggestion messages.** SHACL's `sh:resultMessage`
  analogue. Future option flag could add per-rule
  `:resultMessage "human-readable summary"` literals.
- **Cross-rule deduplication.** Two rules can witness the same
  underlying contradiction (e.g., `cls-com` and `cax-dw` fire
  together when complement-of is also disjoint-with). Each rule
  emits independently. Documented; consumers dedupe gem-side if
  they care.
- **Datatype lexical-form validation beyond XSD.** Plain
  literals (`xsd:string` default) and language-tagged literals
  skip validation — no rule applies. Custom datatypes (consumer
  IRIs) skip too.
- **`rdf:PlainLiteral` semantics.** Treated as `xsd:string`.
- **Integration with `rdf_dred_overdelete`.** Inconsistency
  detection is purely read-only; it doesn't touch the
  dependency index. If a consumer wants "DRed, then check
  consistency" they call them in sequence.

---

## Re-numbering downstream milestones

| Version | Topic |
|---|---|
| 0.13.0 | OWL 2 RL inconsistency detection (this file) |
| 0.14.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.15.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB; differential dataflow |

Unchanged from PLAN_0.12.0's renumber.
