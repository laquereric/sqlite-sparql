# PLAN 0.10.0 — Full OWL 2 RL coverage (remaining derivation rules)

> Extend `rdf_owl_rl_materialise` from the 0.9.0 15-rule subset to the
> full W3C OWL 2 RL/RDF **derivation-rule** table. Adds class-expression
> reasoning (`owl:intersectionOf`, `owl:unionOf`, `owl:oneOf`,
> `owl:someValuesFrom`, `owl:allValuesFrom`, `owl:hasValue`,
> `owl:maxCardinality`), inverse-functional + property-chain + hasKey
> property reasoning, equality replacement (`eq-rep-s/p/o`), and the
> derivation slice of the datatype rules.
>
> **Out of scope for 0.10.0:** the 15-odd *inconsistency* rules
> (`prp-irp`, `cax-dw`, `cls-com`, `eq-diff*`, `dt-not-type`, etc.).
> These detect contradictions rather than derive triples and ship as
> their own surface (`rdf_owl_rl_consistent`) in a later plan —
> rationale in §"Inconsistency rules — deferred to a separate surface"
> below.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions" item
**#6 — Native OWL 2 RL rule pass**, second bullet ("15-rule subset,
not the full ~70 W3C OWL 2 RL rule table … The remaining ~55 rules
… land in engine 0.10.0; Vv::Graph callers using ontologies that depend
on out-of-subset constructs stay on the per-rule `Sparql.execute` path
until then").

The 0.9.0 release ships the fixpoint loop, options blob, provenance
emission, dedup-against-inferred check, and the error envelopes. **All
of those are reused verbatim.** This plan only adds rule functions to
`src/functions/rdf_owl_rl/rules.rs`'s `RULES` table plus one new helper
module for RDF-list traversal. The fixpoint loop in
`src/functions/rdf_owl_rl.rs` doesn't change.

Forward-leaning ship posture, same as 0.7.0 / 0.8.0 / 0.9.0: substrate
ships ahead of telemetry. Vv::Graph's `Vv::Graph::Reasoner::Rules` only
exposes the 15-rule subset today (`Rules::PHASE_B_PENDING` lists the
remainder); this plan ships the engine half so a future VG PLAN can
graduate `PHASE_B_PENDING` lockstep without further engine work.

Depends on 0.9.0 (everything outside `rules.rs` is unchanged).

---

## Goal

`cargo test` passes per-rule round-trip tests for every derivation rule
added in this release, plus an **extended equivalence-with-VG fixture**
covering full closure (the 0.9.0 fixture's 15-rule slice grows to cover
the new ~40 rules). The function signature, options JSON shape, return
convention, error envelopes, and provenance annotation shape from 0.9.0
all stay byte-identical — this release is purely additive at the rule
level.

Concretely:

1. A pure-T-Box test loads an ontology using
   `owl:intersectionOf (:A :B)`, `owl:hasValue`, `owl:someValuesFrom`,
   `rdfs:subClassOf`, calls `rdf_owl_rl_materialise`, and asserts every
   expected derived `rdfs:subClassOf` triple lands.
2. A pure-A-Box test loads instance data + class expressions, calls
   materialise, and asserts every expected `rdf:type` propagation lands
   (e.g., `?x rdf:type :C` where `:C` is defined as
   `owl:intersectionOf (:A :B)` and `?x rdf:type :A`, `?x rdf:type :B`).
3. The equality-replacement test (`eq-rep-s/p/o`): given
   `:a owl:sameAs :b` + `:a :p :o`, derive `:b :p :o`. Pin the
   `equality_saturation: false` option as the escape hatch for callers
   who don't want this (perf reasons; see §"Equality replacement is
   opt-out, not opt-in" below).
4. The full-closure equivalence test (extending the 0.9.0 fixture)
   passes — engine output diffs against
   `tests/fixtures/owl_rl_expected.nt` and matches under permutation.

---

## What 0.10.0 covers vs. doesn't

The W3C OWL 2 RL/RDF rule table groups rules into Eq, Prp, Cls, Cax,
Scm, and Dt (Datatype). Cell colour key: **D** = derivation
(monotonic, derives new triples); **I** = inconsistency (concludes
"false", i.e. the input is inconsistent — no triple to insert).

| Table | Rule | 0.9.0 | 0.10.0 | Out (inconsistency) |
|---|---|---|---|---|
| Eq | eq-ref | | ✓ D | |
| Eq | eq-sym | ✓ | | |
| Eq | eq-trans | ✓ | | |
| Eq | eq-rep-s | | ✓ D | |
| Eq | eq-rep-p | | ✓ D | |
| Eq | eq-rep-o | | ✓ D | |
| Eq | eq-diff1 / eq-diff2 / eq-diff3 | | | I |
| Prp | prp-ap | | | trivial — skipped (no semantics) |
| Prp | prp-dom | ✓ | | |
| Prp | prp-rng | ✓ | | |
| Prp | prp-fp | ✓ | | |
| Prp | prp-ifp | | ✓ D | |
| Prp | prp-irp | | | I |
| Prp | prp-symp | ✓ | | |
| Prp | prp-asyp | | | I |
| Prp | prp-trp | ✓ | | |
| Prp | prp-spo1 | ✓ | | |
| Prp | prp-spo2 (property chain) | | ✓ D | |
| Prp | prp-eqp1 / prp-eqp2 | | ✓ D | |
| Prp | prp-pdw / prp-adp | | | I |
| Prp | prp-inv1 / prp-inv2 | ✓ | | |
| Prp | prp-key | | ✓ D | |
| Prp | prp-npa1 / prp-npa2 | | | I |
| Cls | cls-thing | | ✓ D | (axiomatic) |
| Cls | cls-nothing1 | | ✓ D | (axiomatic) |
| Cls | cls-nothing2 | | | I |
| Cls | cls-int1 / cls-int2 | | ✓ D | |
| Cls | cls-uni | | ✓ D | |
| Cls | cls-com | | | I |
| Cls | cls-svf1 | | ✓ D | |
| Cls | cls-svf2 | | ✓ D | (someValuesFrom owl:Thing case) |
| Cls | cls-avf | | ✓ D | |
| Cls | cls-hv1 / cls-hv2 | | ✓ D | |
| Cls | cls-maxc1 / cls-maxqc1 / cls-maxqc2 | | | I |
| Cls | cls-maxc2 | | ✓ D | (maxCardinality 1 → sameAs) |
| Cls | cls-maxqc3 / cls-maxqc4 | | ✓ D | (maxQualifiedCardinality 1 → sameAs) |
| Cls | cls-oo | | ✓ D | |
| Cax | cax-sco | ✓ | | |
| Cax | cax-eqc1 / cax-eqc2 | | ✓ D | |
| Cax | cax-dw / cax-adc | | | I |
| Scm | scm-cls | | ✓ D | |
| Scm | scm-sco | ✓ | | |
| Scm | scm-eqc1 | ✓ | | |
| Scm | scm-eqc2 | | ✓ D | |
| Scm | scm-op / scm-dp | | ✓ D | |
| Scm | scm-spo | ✓ | | |
| Scm | scm-eqp1 | ✓ | | |
| Scm | scm-eqp2 | | ✓ D | |
| Scm | scm-dom1 / scm-dom2 | | ✓ D | |
| Scm | scm-rng1 / scm-rng2 | | ✓ D | |
| Scm | scm-hv | | ✓ D | |
| Scm | scm-svf1 / scm-svf2 | | ✓ D | |
| Scm | scm-avf1 / scm-avf2 | | ✓ D | |
| Scm | scm-int / scm-uni | | ✓ D | |
| Dt | dt-type1 | | ✓ D | (axiomatic — every xsd datatype is rdfs:Datatype) |
| Dt | dt-type2 | | ✓ D | (a literal's datatype → datatype rdf:type rdfs:Datatype) |
| Dt | dt-eq / dt-diff | | ✓ D | |
| Dt | dt-not-type | | | I |

Totals: **15 derivation rules in 0.9.0**, **~40 derivation rules added
in 0.10.0**, **~15 inconsistency rules deferred** to a separate plan
(see §"Inconsistency rules — deferred to a separate surface" below).
`prp-ap` is excluded across the table — annotation-property rules have
no entailment consequences in OWL 2 RL/RDF.

---

## Inconsistency rules — deferred to a separate surface

The W3C rule table mixes two semantically different things:

1. **Derivation rules.** Given premises P, derive consequence C.
   Monotonic: each application strictly grows the inferred graph.
2. **Inconsistency rules.** Given premises P, conclude `false` —
   the input is inconsistent. There is no triple to derive; the
   consequence is a metadata flag on the *whole* graph pair.

`rdf_owl_rl_materialise` was designed in 0.9.0 as a monotonic
fixpoint-over-derivations and returns *signed net delta in store
size*. Inconsistency rules don't fit that contract — they don't
produce quads, and "the input is inconsistent" is not a counted
result.

Three options considered:

1. **Emit a marker triple per inconsistency rule fire** (e.g.,
   `<inferred-graph> :inconsistent "true"^^xsd:boolean ;
   :inconsistencyReason <urn:semantica:rule:cls-com> , … .`).
   Rejected: it conflates "I derived 5 triples" with "I detected
   3 inconsistencies, each of which made the entire ontology
   semantically vacuous." The return-delta number stops meaning
   what callers expect.
2. **Surface inconsistency as a SQLite error** (fixed-prefix
   `rdf_owl_rl_materialise: inconsistency detected: rule <id> at
   <s p o>`). Rejected: errors abort the call; consumers can't see
   the full picture of "what's the *complete* set of inconsistencies
   in this ontology?" with one round-trip. They'd have to fix one,
   re-run, fix the next.
3. **A separate function `rdf_owl_rl_consistent(asserted_iri,
   inferred_iri, options_json) → TEXT`** returning a JSON array of
   `{rule, s, p, o}` violation records (or `[]` for consistent).
   **Picked.** Honest contract split: materialise is for derivations,
   consistent is for consistency. A future engine release ships this
   alongside `rdf_shacl_core_validate` (PLAN_0.11.0) — both produce
   violation reports, both stay separate from monotonic
   derivation.

This release explicitly leaves inconsistency-rule coverage to a
future plan (working name: `PLAN_0.13.0` after SHACL Core lands
and DRed lands, since neither needs OWL inconsistency detection
on the critical path). Vv::Graph's `Vv::Graph::Reasoner.consistent?`
(once it ships) can route through `rdf_owl_rl_consistent` then.

If a consumer surfaces a strong signal for inconsistency detection
before 0.13.0 — Vv::Graph's consistency check becoming a routine
production-path call, say — pull the work forward. No signal today.

---

## Equality replacement is opt-out, not opt-in

`eq-rep-s`, `eq-rep-p`, `eq-rep-o` substitute terms across the entire
graph wherever `owl:sameAs` holds:

```
:a owl:sameAs :b .
:a :p :o .

⇒ derives :b :p :o .   (eq-rep-s)
```

This is correctness-required for OWL 2 RL semantics, but on a graph
with N triples and K `sameAs` pairs the closure can grow by O(N · K)
in the worst case. A consumer running materialise on a graph with
heavy entity-resolution-driven `sameAs` linkage gets a multi-hour
closure where the 0.9.0 subset finished in seconds.

**The default stays "ship the W3C semantics":**
`options.equality_saturation` defaults to `true`. Callers who know
their graph blows up disable with `{"equality_saturation": false}`,
which short-circuits `eq-rep-s/p/o` (but keeps `eq-sym`, `eq-trans`,
so `sameAs` is still a proper equivalence relation; callers just
don't get term-substitution).

**Phase D update — `eq-ref` is opt-in, not opt-out.** When this
plan was first written, `eq-ref` was assumed safe to ship on by
default. Phase D test runs proved otherwise: `eq-ref` with
`provenance: true` does **not** converge. Every reflexive
`?s owl:sameAs ?s` it derives gets two annotation quads under
provenance, whose subjects are quoted-triple terms not present
in the input graph; `eq-ref` then derives reflexive sameAs for
*those* quoted triples on the next iteration, which themselves
land annotated, and so on — the closure runs out the
50-iteration cap or hangs. Solution: a separate
`options.eq_reflexive` flag (default **`false`**) gates the rule.
Callers round-tripping against a W3C-strict reasoner that expects
the reflexive saturation can opt in; the engine default stays
bounded.

Documenting this in the `MaterialiseOptions` struct docstring + in
the CHANGELOG. The equivalence-with-VG fixture uses
`equality_saturation: true` (default) and `eq_reflexive: false`
(default); Vv::Graph's `Reasoner` ships its own opt-out mechanism
(Vv::Graph PLAN_0.10.0 territory) — engine just exposes the levers.

---

## Phase A — RDF list traversal helper

A large fraction of the new rules reference RDF lists:
`(c1 c2 c3) = [rdf:first c1; rdf:rest [rdf:first c2; rdf:rest
[rdf:first c3; rdf:rest rdf:nil]]]`. Used by `cls-int1/2`, `cls-uni`,
`cls-oo`, `cls-svf*`, `cls-avf`, `cls-hv*`, `cls-maxqc*`, `prp-spo2`
(property chain), `prp-key`, `scm-int`, `scm-uni`.

New file: `src/functions/rdf_owl_rl/rdf_lists.rs`.

```rust
use oxigraph::model::{GraphName, NamedNodeRef, Subject, Term, TermRef};
use oxigraph::store::Store;
use crate::error::{Result, SparqlError};
use std::collections::HashSet;

pub(crate) const RDF_FIRST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#first");
pub(crate) const RDF_REST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#rest");
pub(crate) const RDF_NIL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#nil");

/// Walk an RDF list starting at `head`, returning the sequence of
/// `rdf:first` values in list order. Returns `None` (not an empty
/// `Vec`) when `head` is not a well-formed list — caller can
/// distinguish "empty list (= rdf:nil)" from "malformed."
///
/// Termination guarantee: a `seen` set rejects cycles. A list of
/// length N takes N store lookups.
pub(crate) fn walk_list(
    store: &Store,
    head: &Subject,
    graphs: &[&GraphName],
) -> Result<Option<Vec<Term>>> { /* … */ }
```

The helper queries the asserted+inferred graphs (same convention as
`pairs_for_predicate` / `instances_of` in `rules.rs`). A list head of
`rdf:nil` returns `Some(vec![])`. A cycle or missing `rdf:first`/`rdf:rest`
returns `None`.

### Exit criteria for Phase A

`cargo build` clean. Unit tests in `rdf_lists.rs` cover: empty list
(`rdf:nil`), single-element, three-element, cyclic, missing-first,
missing-rest. Helper is used by Phase B/C rules.

---

## Phase B — scm-* T-Box rules

Add to `rules.rs`. All operate over the union of asserted+inferred,
all monotonic, all small adjacency-table joins (no list traversal
needed except for `scm-int` / `scm-uni`).

| Rule | Premise | Derived |
|---|---|---|
| `scm-cls` | `?c rdf:type owl:Class` | `?c rdfs:subClassOf ?c` AND `?c owl:equivalentClass ?c` AND `?c rdfs:subClassOf owl:Thing` AND `owl:Nothing rdfs:subClassOf ?c` |
| `scm-eqc2` | `?c1 rdfs:subClassOf ?c2 . ?c2 rdfs:subClassOf ?c1` | `?c1 owl:equivalentClass ?c2` |
| `scm-op` | `?p rdf:type owl:ObjectProperty` | `?p rdfs:subPropertyOf ?p` AND `?p owl:equivalentProperty ?p` |
| `scm-dp` | `?p rdf:type owl:DatatypeProperty` | `?p rdfs:subPropertyOf ?p` AND `?p owl:equivalentProperty ?p` |
| `scm-eqp2` | `?p1 rdfs:subPropertyOf ?p2 . ?p2 rdfs:subPropertyOf ?p1` | `?p1 owl:equivalentProperty ?p2` |
| `scm-dom1` | `?p rdfs:domain ?c1 . ?c1 rdfs:subClassOf ?c2` | `?p rdfs:domain ?c2` |
| `scm-dom2` | `?p2 rdfs:domain ?c . ?p1 rdfs:subPropertyOf ?p2` | `?p1 rdfs:domain ?c` |
| `scm-rng1` | `?p rdfs:range ?c1 . ?c1 rdfs:subClassOf ?c2` | `?p rdfs:range ?c2` |
| `scm-rng2` | `?p2 rdfs:range ?c . ?p1 rdfs:subPropertyOf ?p2` | `?p1 rdfs:range ?c` |
| `scm-hv` | `?c1 owl:hasValue ?v ; owl:onProperty ?p1 . ?c2 owl:hasValue ?v ; owl:onProperty ?p2 . ?p1 rdfs:subPropertyOf ?p2` | `?c1 rdfs:subClassOf ?c2` |
| `scm-svf1` | `?c1 owl:someValuesFrom ?y1 ; owl:onProperty ?p . ?c2 owl:someValuesFrom ?y2 ; owl:onProperty ?p . ?y1 rdfs:subClassOf ?y2` | `?c1 rdfs:subClassOf ?c2` |
| `scm-svf2` | `?c1 owl:someValuesFrom ?y ; owl:onProperty ?p1 . ?c2 owl:someValuesFrom ?y ; owl:onProperty ?p2 . ?p1 rdfs:subPropertyOf ?p2` | `?c1 rdfs:subClassOf ?c2` |
| `scm-avf1` | `?c1 owl:allValuesFrom ?y1 ; owl:onProperty ?p . ?c2 owl:allValuesFrom ?y2 ; owl:onProperty ?p . ?y1 rdfs:subClassOf ?y2` | `?c1 rdfs:subClassOf ?c2` |
| `scm-avf2` | `?c1 owl:allValuesFrom ?y ; owl:onProperty ?p1 . ?c2 owl:allValuesFrom ?y ; owl:onProperty ?p2 . ?p1 rdfs:subPropertyOf ?p2` | `?c2 rdfs:subClassOf ?c1` (note: direction flips for avf2) |
| `scm-int` | `?c owl:intersectionOf (?c1 … ?cn)` | `?c rdfs:subClassOf ?c1 … ?c rdfs:subClassOf ?cn` |
| `scm-uni` | `?c owl:unionOf (?c1 … ?cn)` | `?c1 rdfs:subClassOf ?c … ?cn rdfs:subClassOf ?c` |

`scm-cls`, `scm-op`, `scm-dp` are *axiomatic shells* — they emit
reflexive triples plus the `owl:Thing` / `owl:Nothing` bracketing.
They fire once per class/property in the graph (no fixpoint
expansion); the rule predicate matches an existing instance, the
derived triple is well-defined.

### Exit criteria for Phase B

`cargo build` clean. Each scm-* rule has a `#[cfg(test)]` smoke
test in `rules.rs`.

---

## Phase C — class-expression A-Box rules

The big one — class expressions (`owl:intersectionOf`, `owl:unionOf`,
`owl:someValuesFrom`, `owl:allValuesFrom`, `owl:hasValue`,
`owl:oneOf`, `owl:maxCardinality 1`) propagate `rdf:type` on
instances.

| Rule | Premise | Derived |
|---|---|---|
| `cls-thing` | (axiom) | `owl:Thing rdf:type owl:Class` |
| `cls-nothing1` | (axiom) | `owl:Nothing rdf:type owl:Class` |
| `cls-int1` | `?c owl:intersectionOf (?c1 … ?cn) . ?x rdf:type ?c1 . … . ?x rdf:type ?cn` | `?x rdf:type ?c` |
| `cls-int2` | `?c owl:intersectionOf (?c1 … ?cn) . ?x rdf:type ?c` | `?x rdf:type ?c1 . … . ?x rdf:type ?cn` |
| `cls-uni` | `?c owl:unionOf (?c1 … ?cn) . ?x rdf:type ?ci  (for some i)` | `?x rdf:type ?c` |
| `cls-svf1` | `?c owl:someValuesFrom ?y ; owl:onProperty ?p . ?u ?p ?v . ?v rdf:type ?y` | `?u rdf:type ?c` |
| `cls-svf2` | `?c owl:someValuesFrom owl:Thing ; owl:onProperty ?p . ?u ?p ?v` | `?u rdf:type ?c` |
| `cls-avf` | `?c owl:allValuesFrom ?y ; owl:onProperty ?p . ?x rdf:type ?c . ?x ?p ?u` | `?u rdf:type ?y` |
| `cls-hv1` | `?c owl:hasValue ?v ; owl:onProperty ?p . ?x rdf:type ?c` | `?x ?p ?v` |
| `cls-hv2` | `?c owl:hasValue ?v ; owl:onProperty ?p . ?x ?p ?v` | `?x rdf:type ?c` |
| `cls-maxc2` | `?c owl:maxCardinality 1 ; owl:onProperty ?p . ?x rdf:type ?c . ?x ?p ?y1 . ?x ?p ?y2` | `?y1 owl:sameAs ?y2` |
| `cls-maxqc3` | `?c owl:maxQualifiedCardinality 1 ; owl:onProperty ?p ; owl:onClass ?cls . ?x rdf:type ?c . ?x ?p ?y1 . ?y1 rdf:type ?cls . ?x ?p ?y2 . ?y2 rdf:type ?cls` | `?y1 owl:sameAs ?y2` |
| `cls-maxqc4` | `?c owl:maxQualifiedCardinality 1 ; owl:onProperty ?p ; owl:onClass owl:Thing . ?x rdf:type ?c . ?x ?p ?y1 . ?x ?p ?y2` | `?y1 owl:sameAs ?y2` |
| `cls-oo` | `?c owl:oneOf (?x1 … ?xn)` | `?x1 rdf:type ?c . … . ?xn rdf:type ?c` |
| `cax-eqc1` | `?c1 owl:equivalentClass ?c2 . ?x rdf:type ?c1` | `?x rdf:type ?c2` |
| `cax-eqc2` | `?c1 owl:equivalentClass ?c2 . ?x rdf:type ?c2` | `?x rdf:type ?c1` |

Implementation note: for the `owl:onProperty`-restriction rules
(`cls-svf*`, `cls-avf`, `cls-hv*`, `cls-maxc2`, `cls-maxqc*`),
pre-build an index `restriction_class → (onProperty, onValue/onClass)`
once per iteration. Reuse the index across the rules that share the
shape. Keeps the inner loops linear in the data graph rather than
quadratic.

### Exit criteria for Phase C

`cargo build` clean. Per-rule smoke tests. Adds the bulk of the new
test count.

---

## Phase D — remaining property + equality rules

| Rule | Premise | Derived |
|---|---|---|
| `prp-ifp` | `?p rdf:type owl:InverseFunctionalProperty . ?x1 ?p ?y . ?x2 ?p ?y` | `?x1 owl:sameAs ?x2` |
| `prp-spo2` | `?p owl:propertyChainAxiom (?p1 … ?pn) . ?u1 ?p1 ?u2 . … . ?un ?pn ?u(n+1)` | `?u1 ?p ?u(n+1)` |
| `prp-eqp1` | `?p1 owl:equivalentProperty ?p2 . ?x ?p1 ?y` | `?x ?p2 ?y` |
| `prp-eqp2` | `?p1 owl:equivalentProperty ?p2 . ?x ?p2 ?y` | `?x ?p1 ?y` |
| `prp-key` | `?c owl:hasKey (?p1 … ?pn) . ?x rdf:type ?c . ?x ?p1 ?z1 . … . ?x ?pn ?zn . ?y rdf:type ?c . ?y ?p1 ?z1 . … . ?y ?pn ?zn` | `?x owl:sameAs ?y` |
| `eq-ref` | `?s ?p ?o` | `?s owl:sameAs ?s . ?p owl:sameAs ?p . ?o owl:sameAs ?o` |
| `eq-rep-s` | `?s owl:sameAs ?s2 . ?s ?p ?o` | `?s2 ?p ?o` |
| `eq-rep-p` | `?p owl:sameAs ?p2 . ?s ?p ?o` (where `?p2` is a NamedNode) | `?s ?p2 ?o` |
| `eq-rep-o` | `?o owl:sameAs ?o2 . ?s ?p ?o` | `?s ?p ?o2` |

Notes:

- `prp-spo2` requires the RDF-list helper (chain of properties).
  The premise pattern is a recursive join — implement as a sequence
  walk: hold a `Vec<NamedNode>` chain `[p1, …, pn]`, then collect
  the set of endpoint pairs `(u1, u(n+1))` by joining left-to-right.
- `prp-key` requires the RDF-list helper. For a key `(p1, …, pn)`
  with K class instances, build a `Vec<(value_vector, instance)>`
  index, then sort + groupby. Don't naive double-loop — O(K²) on a
  10K-class instance hurts.
- `eq-ref` fires *one reflexive triple per term used in any
  position* in any quad. Generates a *lot* of triples on a
  10K-triple graph (every term gets one). The dedup check in the
  fixpoint loop keeps the work bounded after the first iteration;
  the second iteration emits nothing. Still — consider whether
  this is *worth* emitting; in practice consumers want the
  equivalence-relation closure (eq-sym, eq-trans) but not the
  reflexive-on-every-term flood. **Keep eq-ref in scope** — it's
  W3C semantics. Document the firehose.
- `eq-rep-p` only emits when both `?p` and `?p2` are
  NamedNodes (predicates must be IRIs). Skip blank-node sameAs
  pairs in predicate position.
- `eq-rep-s/p/o` are gated by `options.equality_saturation` (see
  §"Equality replacement is opt-out, not opt-in" above). When the
  option is `false`, the three functions return empty `Vec`s.

### Exit criteria for Phase D

`cargo build` clean. Per-rule smoke tests including a `prp-key`
test exercising a two-property key (`schema:givenName`,
`schema:familyName`) collapsing duplicate identities. A
`prp-spo2` test exercising a length-3 chain.

---

## Phase E — datatype rules

| Rule | Premise | Derived |
|---|---|---|
| `dt-type1` | (axiom) | every well-known XSD datatype IRI gets `rdf:type rdfs:Datatype`. The W3C list: `xsd:decimal, xsd:integer, xsd:nonNegativeInteger, xsd:positiveInteger, xsd:long, xsd:int, xsd:short, xsd:byte, xsd:nonPositiveInteger, xsd:negativeInteger, xsd:unsignedLong, xsd:unsignedInt, xsd:unsignedShort, xsd:unsignedByte, xsd:double, xsd:float, xsd:string, xsd:normalizedString, xsd:token, xsd:language, xsd:Name, xsd:NCName, xsd:NMTOKEN, xsd:boolean, xsd:hexBinary, xsd:base64Binary, xsd:anyURI, xsd:dateTime, xsd:dateTimeStamp, rdf:XMLLiteral, rdf:PlainLiteral` |
| `dt-type2` | `?lt ∈ literals` | `datatype(?lt) rdf:type rdfs:Datatype` |
| `dt-eq` | `?lt1 ∈ literals . ?lt2 ∈ literals` (with `value-equal(?lt1, ?lt2)`) | `?lt1 owl:sameAs ?lt2` |
| `dt-diff` | `?lt1 ∈ literals . ?lt2 ∈ literals` (with `value-not-equal(?lt1, ?lt2)`) | `?lt1 owl:differentFrom ?lt2` |

Notes:

- `dt-type1` is constant — emit one fixed list at startup. Use
  `[const NAMED_NODES]` to avoid runtime allocation.
- `dt-type2` runs once per iteration, looking up the set of
  datatypes-currently-used-in-some-literal. Reuses the `all_quads`
  walk; collect unique datatype IRIs from `Term::Literal` objects.
- `dt-eq` and `dt-diff` rely on Oxigraph's literal-comparison
  (`Literal::value_eq`). Oxigraph 0.4 ships value-equality for
  standard XSD numeric / boolean / string types; this rule is
  effectively "Oxigraph thinks these two literals are
  value-equal, derive owl:sameAs". *Subjects of owl:sameAs must be
  IRIs or blank nodes in standard RDF; OWL 2 RL allows literals
  as sameAs subjects only inside the rule's derivation context
  (the spec explicitly extends this).* Emit using
  `Subject::Literal(…)` if Oxigraph 0.4's model supports it; if
  not, skip the literal-in-subject-position emission and document
  the limitation (defer fix until Oxigraph 0.5 or when a consumer
  asks).

### Exit criteria for Phase E

`cargo build` clean. Smoke tests: dt-eq fires on
`"1"^^xsd:integer` ≡ `"1"^^xsd:int`; dt-diff fires on
`"1"^^xsd:integer` ≠ `"2"^^xsd:integer`; dt-type2 lists exactly
the literal datatypes present in the graph.

If Oxigraph 0.4 doesn't support literals-in-subject-position for
sameAs derivations, `dt-eq` / `dt-diff` emit *nothing* (the
premise pattern still evaluates, but no quad is insertable).
Document this in the CHANGELOG as a known limitation pending
Oxigraph 0.5; a per-call counter in the `WARNINGS` channel would
be ideal but isn't worth the surface cost today.

---

## Phase F — integration tests + VG-equivalence fixture

Add to `tests/integration_test.rs` under a
`// ── 0.10.0 rdf_owl_rl_materialise (full coverage) ──` banner.

### Per-rule smoke tests

One per derivation rule added in this release (~40 tests). Each
loads a 2–5-quad minimum ontology + instance set, calls materialise
into a named graph, asserts the expected single derived triple
lands. Name them `test_rdf_owl_rl_<rule_id>` (e.g.,
`test_rdf_owl_rl_cls_int1`).

### Composition tests

A handful of multi-rule scenarios exercising rule interaction:

- `test_rdf_owl_rl_int_avf_chain` — `:Vegetarian rdf:type owl:Class
  ; owl:intersectionOf (:Person :OnlyEatsPlants)`,
  `:OnlyEatsPlants owl:onProperty :eats ; owl:allValuesFrom
  :Plant`, `:alice rdf:type :Vegetarian ; :eats :spinach`. Expect
  `:spinach rdf:type :Plant` (cls-int2 + cls-avf composition).
- `test_rdf_owl_rl_key_resolves_duplicates` — `:Person owl:hasKey
  (:givenName :familyName)`, two instances with the same name
  pair. Expect `:p1 owl:sameAs :p2`, then with
  `equality_saturation: true` the union of `:p1`'s and `:p2`'s
  predicates lands on both.

### Equivalence-with-VG fixture extension

The 0.9.0 fixture lives at `tests/fixtures/owl_rl_expected.nt` and
covers the 15-rule subset. Extend with cases exercising each new
rule:

- Add ontology fragments + expected derivations to
  `tests/fixtures/owl_rl_input.nt` (or `.ttl` — pick the one the
  0.9.0 fixture uses; mirror format).
- Add expected derived triples to `owl_rl_expected.nt`.

The test loader reads input, calls materialise, dumps the inferred
graph, diffs against expected under permutation. Same shape as
0.9.0's `test_rdf_owl_rl_materialise_equivalence_with_vg`.

### Idempotence test extension

The 0.9.0 idempotence test (second call returns 0) should still
pass on the expanded ruleset. No new test; just verify it still
goes green with the extended fixture.

### Equality-saturation opt-out test

`test_rdf_owl_rl_materialise_equality_saturation_disabled` —
load `:a owl:sameAs :b . :a :p :o`, call with
`{"equality_saturation": false}`. Assert `:b :p :o` is NOT in
the inferred graph (but `:b owl:sameAs :a` IS there, via
`eq-sym`).

### Exit criteria for Phase F

```
cargo test
cargo build --release && cargo test --release
```

Both green. Test count rises by ~45 (40 per-rule + 5 composition/
fixture/opt-out). CLAUDE.md release-mode footgun applies — rebuild
release first.

---

## Phase G — docs, CHANGELOG, tag 0.10.0

- **`CHANGELOG.md`** — 0.10.0 entry. Lead with "Full OWL 2 RL
  coverage (~40 additional derivation rules)". List the new rule
  IDs grouped by W3C table (Eq, Prp, Cls, Cax, Scm, Dt). Call out
  the equality-saturation opt-out flag with its rationale. Call
  out the inconsistency-rules deferral with a forward pointer to
  the future `rdf_owl_rl_consistent` plan.

- **`README.md`** — extend the "OWL 2 RL native reasoning (since
  0.9.0)" section: rule-coverage badge bumps from 15 → ~55. Add
  the equality-saturation option to the example call. Roadmap
  checkbox: `[x] Full OWL 2 RL derivation coverage — landed in
  0.10.0`. Open a new line: `[ ] OWL 2 RL inconsistency
  detection (rdf_owl_rl_consistent) — planned for a future
  release`.

- **`CLAUDE.md`** — "SQL Function Reference" → "Reasoning"
  subsection: add the `equality_saturation` option to the
  documented options blob. Mention the inconsistency-rules
  deferral. "Completing the implementation" item 7 (currently
  PLAN_0.10.0) graduates to LANDED; renumber the remaining items
  down; insert a new "Native OWL 2 RL inconsistency detection
  (`rdf_owl_rl_consistent`) — DEFERRED" entry where the old item
  7 lived.

- **`CONSUMER_REQUIREMENT_VvGraph.md`** — item #6 "Native OWL 2 RL
  rule pass" section: change the LANDED note from "(15-rule subset)
  in 0.9.0" to "(15-rule subset in 0.9.0; full derivation coverage
  in 0.10.0)". Update the inline coverage paragraph: replace "The
  remaining ~55 rules … land in engine 0.10.0" with "The remaining
  inconsistency rules (`prp-irp`, `cax-dw`, `cls-com`, `eq-diff*`,
  `dt-not-type`, etc.) ship as a separate `rdf_owl_rl_consistent`
  surface in a future engine release." Update the live "Reasoning"
  table row to reflect the new coverage + option.

- **`CONSUMER_REQUIREMENT_MM.md`** — the existing line under
  "Available upstream but not exercised by MM" updates from
  15-rule to full-derivation coverage, same shape.

- **`PLAN_0.2.0.md`** roadmap table: row 0.10.0 "Full OWL 2 RL
  coverage" graduates with an "✓ landed" marker; insert
  PLAN_0.13.0 (or whatever number aligns with SHACL/DRed
  ordering) for the inconsistency-detection surface.

- **`Cargo.toml`** and **`VERSION`** bump to `0.10.0`.

- **`src/functions/rdf_owl_rl/rules.rs`** doc comment — note the
  full coverage and link to PLAN_0.10.0.

- **`src/functions/rdf_owl_rl.rs`** doc comment — bump the rule
  count in the prose; mention the `equality_saturation` option.

### Exit criteria for Phase G

Reading `CHANGELOG.md` shows a 0.10.0 entry naming the rule groups
and the deferred inconsistency follow-on. Reading
`CONSUMER_REQUIREMENT_VvGraph.md` shows item #6 fully landed for
derivations (no remaining "0.10.0 will ship" promise).

### Tag

- `cargo test` and `cargo test --release` both green at the
  bumped version.
- `git tag v0.10.0` and push.
- Ping Vv::Graph: their `PHASE_B_PENDING` rules now have an engine
  path. They can graduate `Reasoner::Rules::OwlRl` to full
  coverage on their own cadence (no engine-floor bump required —
  the new rules are additive at the `rdf_owl_rl_materialise`
  level; Vv::Graph callers using the 0.9.0 subset still see the
  same closure).

---

## Risks

- **Equality saturation blow-up.** Covered above. The opt-out
  flag is the mitigation. If a Vv::Graph caller hits a real
  perf wall and the opt-out isn't reachable through the
  Reasoner API, that's a Vv::Graph plan, not an engine plan —
  the engine ships the lever.

- **`prp-spo2` property-chain inefficiency.** A length-N chain
  with K matching subjects per step is O(K^N) in the naïve join
  shape. For N ≤ 3 (the practical limit; longer chains rarely
  appear in published ontologies) this is fine. For pathological
  N ≥ 5, plan a worst-case cap (e.g., refuse to evaluate chains
  > 6 properties with a fixed-prefix error). Defer until a
  fixture exposes the cost.

- **`prp-key` index memory.** With K class instances and
  N key properties, the index is O(K · N) terms. For
  K = 1M, N = 3 (a common entity-resolution key shape), that's
  3M term clones in memory per iteration. Profile; consider
  streaming the groupby if needed. Defer until Phase F surfaces
  a real cost.

- **`dt-eq` literal-subject limitation.** If Oxigraph 0.4 doesn't
  accept `Subject::Literal(…)`, dt-eq / dt-diff emit nothing.
  This is silent. Cover with a Phase E smoke test that asserts
  *some* derivation; if the smoke test fails on Oxigraph 0.4,
  reduce dt-eq / dt-diff to a no-op + document; revive when
  Oxigraph upgrades.

- **Test fixture maintenance cost.** The
  `owl_rl_expected.nt` fixture is hand-written. With ~40 new
  rules, the fixture grows from ~45 expected triples to
  ~150. Refactor the fixture into per-rule subfiles
  (`tests/fixtures/owl_rl/scm_dom1.nt`, etc.) if it gets
  unwieldy. Decision in Phase F when the size becomes
  uncomfortable.

- **`eq-ref` firehose — realised in Phase D.** The original plan
  shipped `eq-ref` on unconditionally to match W3C semantics. In
  practice, with `provenance: true`, the closure does not converge
  within the 50-iteration cap: each reflexive sameAs `eq-ref`
  derives gets two annotation triples whose subjects are
  quoted-triple terms new to the inferred graph; `eq-ref` then
  derives reflexives for *those* quoted triples on the next
  iteration, and so on. Phase D added `options.eq_reflexive`
  (default **`false`**) gating the rule. Consumers wanting
  W3C-strict round-trip enable explicitly; the engine default
  stays bounded. See §"Equality replacement is opt-out, not
  opt-in" for the full discussion.

- **Inconsistency-rule deferral surprises a consumer.** A
  consumer who reads "0.10.0 = full OWL 2 RL" and expects
  consistency checking will be surprised. The CHANGELOG, README,
  and CONSUMER_REQUIREMENT_VvGraph all call out the split
  prominently; the consistent-vs-materialise contract split is
  the simplest defensible answer. If Vv::Graph's
  `Reasoner.consistent?` plan lands before the engine ships
  `rdf_owl_rl_consistent`, the engine plan pulls forward;
  forward-leaning posture applies symmetrically.

---

## Out of scope for 0.10.0

- **OWL 2 RL inconsistency rules.** See §"Inconsistency rules
  — deferred to a separate surface." Future plan.

- **Annotation property rule (`prp-ap`).** No entailment
  consequences in OWL 2 RL/RDF. Permanently skipped.

- **Native SHACL Core validator.** Vv::Graph CR item #7;
  PLAN_0.11.0.

- **Native dependency index for DRed.** Vv::Graph CR item #8;
  PLAN_0.12.0. The dep-index gets a write-through hook in
  `rdf_owl_rl_materialise` at that point — this plan doesn't
  touch the fixpoint loop, so the hook lands cleanly later.

- **Differential dataflow.** Vv::Graph CR item #10. Stays
  deferred.

- **Persistent RocksDB backend.** Stays deferred.

- **OWL 2 EL / OWL 2 QL profile reasoning.** Vv::Graph doesn't
  ask. The OWL 2 RL/RDF rule set is the conformant fragment for
  rule-based reasoning over RDF; EL and QL require different
  algorithms (tableau / saturation). No signal.

- **Per-rule disable flags** (other than `equality_saturation`).
  Operators picking which subset to apply could be useful for
  warm-fixpoint workloads. Defer until a consumer asks.

- **Performance benchmarks.** The performance claim in the 0.9.0
  CR ("order of magnitude faster") is inherited and still
  unproven. A Phase H benchmark could quantify the engine pass
  vs. the per-rule `sparql_update` shape, but no consumer has
  surfaced telemetry showing a regression. Add only on first
  signal.

---

## Why not split into 0.10.0 / 0.11.0 / 0.12.0

A reasonable alternative would split the ~40 rules across three
releases (scm-* first, cls-* second, eq-rep + prp-key + dt-* third).
Rejected:

- **No consumer is mid-flight.** Vv::Graph stays on the per-rule
  `sparql_update` path for out-of-subset constructs until *all*
  the new rules land. Shipping cls-int1 without cls-int2 means
  Vv::Graph still can't graduate, so the partial release has
  no consumer value.
- **The rules share helpers.** RDF-list walking is used across
  scm-int, cls-int, cls-uni, cls-svf, cls-avf, prp-spo2,
  prp-key. Building it once and reusing is cheaper than
  shipping in pieces.
- **The risk profile is uniform.** Every rule is a small,
  read-the-W3C-spec, write-the-pattern, smoke-test mechanical
  task. The plan stays linear because the work stays
  predictable.

If profile-driven evidence later shows that the cls-* class-
expression rules need substantially different infrastructure
(e.g., a TBox normalisation pass), pull them into PLAN_0.11.0
and ship the remainder in 0.10.0. No evidence of that today.

---

## Re-numbering downstream milestones

`PLAN_0.2.0.md`'s roadmap table currently lists (after
PLAN_0.9.0's renumber):

| Version | Topic |
|---|---|
| 0.9.0 | Native OWL 2 RL pass (15-rule subset) |
| 0.10.0 | Full OWL 2 RL coverage (remaining ~55 rules) |
| 0.11.0 | Native SHACL Core validator (Vv::Graph CR #7) |
| 0.12.0 | Native DRed dependency index (Vv::Graph CR #8) |
| 0.13.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.14.0 | SPARQL HTTP endpoint |

After this plan:

| Version | Topic |
|---|---|
| 0.9.0 | Native OWL 2 RL pass (15-rule subset) |
| 0.10.0 | Full OWL 2 RL derivation coverage — this file |
| 0.11.0 | Native SHACL Core validator (Vv::Graph CR #7) |
| 0.12.0 | Native DRed dependency index (Vv::Graph CR #8) |
| 0.13.0 | Native OWL 2 RL inconsistency detection (`rdf_owl_rl_consistent`) — deferred from 0.10.0 |
| 0.14.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.15.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend; differential dataflow (Vv::Graph CR #10) |

The new row for inconsistency detection inherits the same
"forward-leaning, no consumer required" posture as items 0.9.0
and 0.10.0 themselves. Slot reflects logical adjacency (it's
OWL-RL-shaped work) rather than priority — pull forward if a
consumer signal arrives sooner.

If a future consumer surfaces a strong signal for the packaging-
shape items before 0.13.0 / 0.14.0 / 0.15.0 land, shuffle.
Roadmap stays consumer-driven.
