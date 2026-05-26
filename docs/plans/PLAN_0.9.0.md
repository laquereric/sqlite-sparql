# PLAN 0.9.0 — Native OWL 2 RL rule pass (15-rule subset)

> Add `rdf_owl_rl_materialise(asserted_iri TEXT, inferred_iri TEXT,
> options_json TEXT) → INTEGER` — a native Rust fixpoint loop that
> applies the W3C OWL 2 RL/RDF rule set in one FFI crossing, in
> place of `vv-graph`'s per-rule `Sparql.execute` round-trip.
> Order-of-magnitude faster on large closures; skips the SPARQL
> parser per rule.
>
> **0.9.0 scope: the same 15-rule subset `vv-graph`'s Phase B
> already ships, plus the fixpoint plumbing + provenance
> emission.** Expanding to the full ~70 W3C OWL 2 RL rules is
> PLAN_0.10.0.

Driver: `CONSUMER_REQUIREMENT_VG.md` § "Requested extensions" item
**#6 — Native OWL 2 RL rule pass**. VG's
`Vv::Graph::Reasoner.materialise!` (PLAN_0.9.0 Phase B, commit
`e3dc6bc`) issues one `sparql_update` per rule per fixpoint
iteration — N rules × M iterations of SQL + SPARQL parse +
evaluation. The native pass collapses that to one FFI crossing
while preserving the `:derivedBy <rule_iri> ; :derivedAt …`
RDF-star provenance shape VG attaches gem-side.

The 0.1.0 review (`docs/reviews/REVIEW_0.1.0.md`) has no live
bearing on this scope. Engine v0.7.0's RDF-star round-trip + the
existing `sparql_update` evaluator are the substrate; this plan
adds a new code path that reuses both.

VG posture (verbatim from `CONSUMER_REQUIREMENT_VG.md` §
"Requested extensions" preamble):

> None of the items below are blockers — VG PLANs all ship against
> the engine's existing 0.7.0 surfaces … Priority is "revisit on
> first concrete bottleneck signal," not "schedule a release."

This release stays **forward-leaning** in the same shape as
PLAN_0.7.0 and PLAN_0.8.0: substrate ships ahead of telemetry.
The capability lands; gem-side adoption stays a separate
decision in VG's roadmap and waits for a real signal.

Depends on 0.2.0 (shared store), 0.3.0 (named-graph plumbing for
`asserted_iri` / `inferred_iri`), 0.5.0 (`sparql_update`'s
evaluator path, reused as a fallback for non-supported rules
during the transition), 0.7.0 (RDF-star — provenance annotations
serialise via `term_to_ntriples` on `Term::Triple`), and 0.8.0
(`rdf_construct_many` — not called directly but the same JSON
input convention is reused for `options_json`).

---

## Goal

`cargo test` passes a round-trip test that:

1. Loads a 6-triple OWL T-Box (3 `rdfs:subClassOf`, 2
   `rdfs:subPropertyOf`, 1 `rdfs:domain`) plus a 4-triple A-Box
   into the default graph (via `rdf_load_turtle`).
2. Calls `rdf_owl_rl_materialise('default-graph-sentinel',
   'urn:g:inferred', json('{"max_iterations": 50, "provenance":
   true}'))`.
3. Asserts the return integer matches the expected derived count.
4. Asserts the inferred graph contains every derived triple at
   `sameTerm`-equivalence with the expected closure.
5. Asserts each derived triple carries the expected RDF-star
   annotations: `<< s p o >> :derivedBy <urn:semantica:rule:<id>>`
   and `<< s p o >> :derivedAt <some xsd:dateTime>`.

Plus an **equivalence test** (the big one): run VG's
`Vv::Graph::Reasoner.materialise!` AND `rdf_owl_rl_materialise`
on byte-identical inputs; assert the resulting inferred graphs
are `sameTerm`-equivalent under permutation. This is the
contract: consumers must see the same inferred-graph contents
whichever path they take.

---

## What 0.9.0 covers vs. doesn't

VG's `Vv::Graph::Reasoner::Rules::OwlRl` ships 15 rules covering
T-Box transitive closures, A-Box propagation, domain/range,
property characteristics, and partial sameAs closure. The
remaining ~55 W3C OWL 2 RL/RDF rules sit in
`Rules::PHASE_B_PENDING`. **This plan's 0.9.0 scope matches
VG's coverage exactly**, so the equivalence test pins
parity, not extrapolation.

| Status | Rules | Notes |
|---|---|---|
| **0.9.0 in scope** | `scm-sco`, `scm-spo`, `scm-eqc1`, `scm-eqp1`, `cax-sco`, `prp-spo1`, `prp-dom`, `prp-rng`, `prp-trp`, `prp-symp`, `prp-inv1`, `prp-inv2`, `prp-fp`, `eq-sym`, `eq-trans` | 15 rules — matches VG `Rules::OwlRl` exactly |
| **0.10.0 (out of scope here)** | The remaining ~55 W3C OWL 2 RL rules | Mechanical transcription, deferred until VG's `PHASE_B_PENDING` is being worked |

If VG's rule set expands before 0.10.0 ships, the gap re-opens
and the equivalence test starts failing — bump the engine
floor + ship 0.10.0 lockstep, same as the 0.7.0 → 0.8.0
graduation pattern.

---

## Why a native pass, not "teach `sparql_update` to recognise OWL"

The CR ask is explicit (verbatim):

> A native Rust pass that walks the Oxigraph store directly to
> apply the OWL 2 RL rule set in place of the gem's per-rule
> `sparql_update` round-trip. Order-of-magnitude faster on large
> closures; skips the SPARQL parser per rule.

Three reasons to keep the native pass as a separate scalar
rather than overloading `sparql_update`:

- **Different shape, different name.** `sparql_update(query)`
  takes a SPARQL UPDATE string; `rdf_owl_rl_materialise(asserted,
  inferred, options)` takes graph IRIs plus a JSON options blob.
  Conflating the two under one name means callers sniff. Name it
  honestly.
- **Different cost profile.** `sparql_update` parses + plans +
  evaluates per call. The native pass amortises the parse / plan
  cost across N rules × M iterations by hand-rolling the rule
  application loop in Rust. A user issuing a
  `sparql_update('INSERT … { … OWL rule body … }')` should NOT
  silently get the native pass; that's a surprising routing
  decision. Separate scalar = explicit opt-in.
- **Different contract for provenance.** The native pass emits
  RDF-star annotations on every derived triple. `sparql_update`
  doesn't. If they shared a name, the "does this emit
  provenance?" question becomes shape-dependent. Separate
  scalar keeps the contract clean: the native pass emits
  provenance iff `options.provenance == true`; `sparql_update`
  never does (consumers attach annotations themselves, per the
  0.8.0 `rdf_construct_many` posture).

---

## Why match VG's `:derivedBy <rule_iri>` shape, not invent our own

VG's `Vv::Graph::Reasoner` already commits to:

```
<< :s :p :o >> :derivedBy <urn:semantica:rule:scm-sco> .
<< :s :p :o >> :derivedAt "2026-05-25T10:30:00Z"^^xsd:dateTime .
```

Predicate IRIs (`:derivedBy`, `:derivedAt`) are
`http://www.w3.org/ns/prov#wasDerivedFrom` and
`http://www.w3.org/ns/prov#generatedAtTime` mapped through VG's
namespace alias — VG documents the exact IRIs in
`lib/vv/graph/reasoner/rule_set.rb`.

The engine pass adopts the same predicate IRIs verbatim, exposed
via the options JSON:

```json
{
  "max_iterations": 50,
  "provenance": true,
  "derived_by_iri": "http://www.w3.org/ns/prov#wasDerivedFrom",
  "derived_at_iri": "http://www.w3.org/ns/prov#generatedAtTime",
  "rule_iri_prefix": "urn:semantica:rule:"
}
```

Defaults match VG's current shape; operators can override if
they're emitting into a different provenance vocabulary. The
defaults pin the equivalence-test contract.

This is a deviation from PLAN_0.7.0 and PLAN_0.8.0's "engine
stays domain-agnostic" posture. The reason: OWL 2 RL
materialisation has nowhere to *put* provenance except on the
triple it just derived — there's no consumer round-trip the way
`rdf_construct_many` has (where RS hands back N-Triples blobs
and the consumer annotates). The materialisation is engine-side
end-to-end; provenance has to be engine-side too. Pick VG's
shape as the default + let it be overridden.

---

## Return value — net derived count, signed

Same convention as `sparql_update` (PLAN_0.5.0): the integer
return is the **signed net delta in the inferred graph's size**
across the call. Always positive for materialisation (the rule
set is monotonic — fixpoint adds, never removes), but signing it
matches the established surface convention. A return of `0`
means the asserted graph was already at fixpoint.

Provenance-annotation triples count toward the return: each
derived asserted triple emits 1 (asserted) + 2 (annotations)
quads when `provenance: true`, so a closure of 7 derived asserted
triples returns 21 with provenance and 7 without.

---

## Atomicity — fixpoint-or-rollback, plus max-iterations guard

Two failure modes:

1. **Iteration cap reached without fixpoint.** Return a SQLite
   error with the prefix
   `rdf_owl_rl_materialise: fixpoint not reached after N iterations`.
   The inferred graph at this point holds a *partial* closure —
   not rolled back. Rationale: rolling back a 1-million-triple
   closure on iteration N+1 is expensive; the operator can
   inspect, decide, and re-run with a larger `max_iterations`
   or `rdf_clear` the inferred graph and retry. Matches
   `sparql_update`'s partial-on-evaluation posture.
2. **Rule-application error.** If any rule's pattern evaluation
   raises (e.g., a malformed IRI in the asserted graph), abort
   the iteration and surface
   `rdf_owl_rl_materialise: rule <id> error at iteration N: …`.
   Same partial-state semantics as #1.

The non-error case (fixpoint reached) returns cleanly. The
fixpoint contract: a second call on the same input returns `0`.

---

## Phase A — surface scaffolding + register

New module `src/functions/rdf_owl_rl.rs`. Skeleton:

```rust
use oxigraph::model::{NamedNode, Quad, Subject, Term};
use serde::{Deserialize, Serialize};
use sqlite_loadable::{api, define_scalar_function, prelude::*, FunctionFlags};

use crate::error::SparqlError;
use crate::store::with_store;

#[derive(Deserialize)]
struct MaterialiseOptions {
    #[serde(default = "default_max_iterations")]
    max_iterations: usize,
    #[serde(default)]
    provenance: bool,
    #[serde(default = "default_derived_by")]
    derived_by_iri: String,
    #[serde(default = "default_derived_at")]
    derived_at_iri: String,
    #[serde(default = "default_rule_prefix")]
    rule_iri_prefix: String,
}

fn default_max_iterations() -> usize { 50 }
fn default_derived_by() -> String { "http://www.w3.org/ns/prov#wasDerivedFrom".into() }
fn default_derived_at() -> String { "http://www.w3.org/ns/prov#generatedAtTime".into() }
fn default_rule_prefix() -> String { "urn:semantica:rule:".into() }

pub fn rdf_owl_rl_materialise_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    // asserted_iri: TEXT or NULL (NULL = default graph)
    // inferred_iri: TEXT (named graph)
    // options_json: TEXT (JSON object, see MaterialiseOptions)
    // returns: INTEGER signed delta
    todo!("Phase B")
}

pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(db, "rdf_owl_rl_materialise", 3,
        rdf_owl_rl_materialise_fn, FunctionFlags::UTF8)?;
    Ok(())
}
```

Wire `register(db)` into `src/lib.rs`'s entrypoint alongside the
existing function-module registrations.

### Exit criteria for Phase A

`cargo build` clean (the `todo!` is fine in dev — Phase B
removes it). The function shows up in
`sqlite_master`-equivalent introspection: `.functions` in the
SQLite CLI lists `rdf_owl_rl_materialise`.

---

## Phase B — rule library (the 15-rule subset)

Add `src/functions/rdf_owl_rl/rules.rs` with one function per
rule. Each rule:

- Takes `&Store`, `asserted_graph: &GraphName`, `inferred_graph:
  &GraphName`.
- Queries the store via `Store::quads_for_pattern` (NOT through
  SPARQL — that's the cost we're avoiding). The query shape is
  fixed per rule.
- Returns `Vec<Triple>` of newly-derivable asserted triples (not
  yet in inferred_graph).

Rule-by-rule pattern:

| Rule | Premise pattern | Derived triple |
|---|---|---|
| `scm-sco` | `?c1 rdfs:subClassOf ?c2 . ?c2 rdfs:subClassOf ?c3` | `?c1 rdfs:subClassOf ?c3` |
| `scm-spo` | `?p1 rdfs:subPropertyOf ?p2 . ?p2 rdfs:subPropertyOf ?p3` | `?p1 rdfs:subPropertyOf ?p3` |
| `scm-eqc1` | `?c1 owl:equivalentClass ?c2` | `?c1 rdfs:subClassOf ?c2` AND `?c2 rdfs:subClassOf ?c1` |
| `scm-eqp1` | `?p1 owl:equivalentProperty ?p2` | `?p1 rdfs:subPropertyOf ?p2` AND `?p2 rdfs:subPropertyOf ?p1` |
| `cax-sco` | `?s rdf:type ?c1 . ?c1 rdfs:subClassOf ?c2` | `?s rdf:type ?c2` |
| `prp-spo1` | `?s ?p1 ?o . ?p1 rdfs:subPropertyOf ?p2` | `?s ?p2 ?o` |
| `prp-dom` | `?s ?p ?o . ?p rdfs:domain ?c` | `?s rdf:type ?c` |
| `prp-rng` | `?s ?p ?o . ?p rdfs:range ?c` | `?o rdf:type ?c` |
| `prp-trp` | `?p rdf:type owl:TransitiveProperty . ?x ?p ?y . ?y ?p ?z` | `?x ?p ?z` |
| `prp-symp` | `?p rdf:type owl:SymmetricProperty . ?x ?p ?y` | `?y ?p ?x` |
| `prp-inv1` | `?p1 owl:inverseOf ?p2 . ?x ?p1 ?y` | `?y ?p2 ?x` |
| `prp-inv2` | `?p1 owl:inverseOf ?p2 . ?x ?p2 ?y` | `?y ?p1 ?x` |
| `prp-fp` | `?p rdf:type owl:FunctionalProperty . ?x ?p ?y1 . ?x ?p ?y2` | `?y1 owl:sameAs ?y2` |
| `eq-sym` | `?x owl:sameAs ?y` | `?y owl:sameAs ?x` |
| `eq-trans` | `?x owl:sameAs ?y . ?y owl:sameAs ?z` | `?x owl:sameAs ?z` |

Each rule lives as a `fn apply_scm_sco(store, asserted, inferred)
-> Vec<Triple>` (etc.) in `src/functions/rdf_owl_rl/rules.rs`.
The dispatch is a static array:

```rust
pub static RULES: &[Rule] = &[
    Rule { iri: "scm-sco",   apply: apply_scm_sco },
    Rule { iri: "scm-spo",   apply: apply_scm_spo },
    // ...15 total
];

pub struct Rule {
    pub iri: &'static str,
    pub apply: fn(&Store, &GraphName, &GraphName) -> crate::error::Result<Vec<Triple>>,
}
```

### Exit criteria for Phase B

`cargo build` clean. Unit tests (in `rules.rs` itself, `#[cfg(test)]`)
exercise each rule against a hand-built 3-quad store and assert
the derived triples set.

---

## Phase C — fixpoint loop + provenance emission

`src/functions/rdf_owl_rl.rs`'s body replaces the `todo!`:

```rust
fn execute_materialise(
    asserted: Option<&str>,
    inferred: &str,
    options_json: &str,
) -> crate::error::Result<i64> {
    let opts: MaterialiseOptions = serde_json::from_str(options_json)
        .map_err(|e| SparqlError::InvalidArgument(
            format!("rdf_owl_rl_materialise: options_json: {e}")))?;

    let asserted_g = parse_graph_name(asserted)?;
    let inferred_g = parse_graph_name(Some(inferred))?;

    with_store(|store| {
        let before = store.len().unwrap_or(0) as i64;
        let mut iteration = 0;

        loop {
            iteration += 1;
            if iteration > opts.max_iterations {
                return Err(SparqlError::EvalError(format!(
                    "rdf_owl_rl_materialise: fixpoint not reached after {} iterations",
                    opts.max_iterations
                )));
            }

            let mut new_quads: Vec<Quad> = Vec::new();
            for rule in rules::RULES {
                let derived = (rule.apply)(store, &asserted_g, &inferred_g)
                    .map_err(|e| SparqlError::EvalError(format!(
                        "rdf_owl_rl_materialise: rule {} error at iteration {iteration}: {e}",
                        rule.iri
                    )))?;
                for t in derived {
                    let asserted_quad = Quad::new(
                        t.subject.clone(), t.predicate.clone(),
                        t.object.clone(), inferred_g.clone(),
                    );
                    if store.contains(&asserted_quad).unwrap_or(false) {
                        continue; // dedup: already in inferred graph
                    }
                    new_quads.push(asserted_quad);
                    if opts.provenance {
                        new_quads.extend(provenance_annotations(
                            &t, rule.iri, &opts, &inferred_g
                        ));
                    }
                }
            }

            if new_quads.is_empty() {
                break; // fixpoint
            }
            for q in &new_quads {
                store.insert(q).map_err(|e| SparqlError::StoreError(e.to_string()))?;
            }
        }

        let after = store.len().unwrap_or(0) as i64;
        Ok(after - before)
    })
}
```

`provenance_annotations` emits the two RDF-star annotation
quads per derived triple:

```rust
fn provenance_annotations(
    derived: &Triple,
    rule_iri: &str,
    opts: &MaterialiseOptions,
    inferred_g: &GraphName,
) -> Vec<Quad> {
    let quoted = Subject::Triple(Box::new(derived.clone()));
    let derived_by = NamedNode::new(&opts.derived_by_iri).unwrap();
    let derived_at = NamedNode::new(&opts.derived_at_iri).unwrap();
    let rule_node = NamedNode::new(format!("{}{}", opts.rule_iri_prefix, rule_iri)).unwrap();
    let now_lit = Literal::new_typed_literal(
        chrono::Utc::now().to_rfc3339(),
        NamedNode::new("http://www.w3.org/2001/XMLSchema#dateTime").unwrap(),
    );
    vec![
        Quad::new(quoted.clone(), derived_by, Term::NamedNode(rule_node),
                  inferred_g.clone()),
        Quad::new(quoted,         derived_at, Term::Literal(now_lit),
                  inferred_g.clone()),
    ]
}
```

Adds `chrono` to `Cargo.toml` `[dependencies]` for the timestamp
(`chrono = { version = "0.4", default-features = false, features =
["clock"] }` — minimal feature set, no time-zone db). If `chrono`
proves too heavy, fall back to `std::time::SystemTime` + a
hand-rolled RFC3339 formatter.

### Exit criteria for Phase C

`cargo build` clean. A manual smoke test via the SQLite CLI:
load a 6-triple ontology, call `rdf_owl_rl_materialise`, dump
the inferred graph, see the expected closure + annotations.

---

## Phase D — graph-routing edge cases

Three edge cases the loop above doesn't yet handle:

1. **`asserted_iri` = `inferred_iri`.** Materialising into the
   same graph as the asserts means rules fire on derived triples
   too — a *recursive closure* over the asserted+derived union.
   This is semantically correct for OWL 2 RL (the rule set is
   monotonic) but doubles the iteration count. Document but
   allow.
2. **`asserted_iri` = NULL (default graph).** Rules query both
   the default graph (premises) and the inferred named graph
   (existing derived triples, for dedup). The `quads_for_pattern`
   call needs to handle both. Test pin.
3. **`inferred_iri` = NULL** (would mean "into the default
   graph"). **Reject** with a clear error:
   `rdf_owl_rl_materialise: inferred_iri must be a named graph
   (NULL is not allowed for the inferred slot)`. Mixing
   derived triples into the default graph erases the
   asserted-vs-derived distinction OWL reasoning depends on; if
   an operator really wants this, they can pass a named graph
   and copy from it after.

### Exit criteria for Phase D

Three new unit tests covering each edge case. Build clean.

---

## Phase E — integration tests

Add to `tests/integration_test.rs` under a
`// ── 0.9.0 rdf_owl_rl_materialise ──` banner.

### `test_rdf_owl_rl_materialise_scm_sco`

Single-rule round-trip. Load `:A rdfs:subClassOf :B ; :B
rdfs:subClassOf :C` into the default graph. Call
`rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{"provenance":
false}')`. Expect return `1` (one derived triple, no
annotations). Assert `:A rdfs:subClassOf :C` exists in
`urn:g:inferred`.

### `test_rdf_owl_rl_materialise_full_closure`

The 6-triple T-Box + 4-triple A-Box from §Goal. Run with
`provenance: true`. Assert return matches expected closure +
annotation count. Inspect inferred graph, count
`<<…>> :wasDerivedFrom …` quads = derived asserted count.

### `test_rdf_owl_rl_materialise_fixpoint_idempotent`

Call materialise twice. Second call returns `0` (fixpoint
already reached, nothing new).

### `test_rdf_owl_rl_materialise_max_iterations_guard`

Construct an input that would loop indefinitely if the guard
weren't there (a `prp-trp` rule on a pre-saturated transitive
graph won't loop; need to construct one that genuinely
expands at each iteration). If no natural-looking input
triggers this, fall back to `max_iterations: 1` against a
multi-iteration input and assert the error.

### `test_rdf_owl_rl_materialise_inferred_must_be_named`

`SELECT rdf_owl_rl_materialise(NULL, NULL, '{}')` errors with
the fixed-prefix message.

### `test_rdf_owl_rl_materialise_options_default`

`SELECT rdf_owl_rl_materialise(NULL, 'urn:g:inferred', '{}')`
runs with defaults — `max_iterations: 50`, `provenance: false`,
default predicate IRIs. Pins the defaults' contract.

### `test_rdf_owl_rl_materialise_provenance_predicate_override`

Pass `{"provenance": true, "derived_by_iri":
"http://example.org/byRule"}`. Assert the emitted annotations
use the overridden predicate.

### `test_rdf_owl_rl_materialise_equivalence_with_vg`

**The big one.** Compares engine output against VG-shaped
expected output. Two options here:

(a) **Hand-write the expected closure** for a small fixture and
    assert the engine matches it. Doesn't require running VG.
(b) **Spawn a Ruby subprocess** that calls
    `Vv::Graph::Reasoner.materialise!` on the same fixture, dumps
    the resulting inferred graph, diff against the engine's
    output. Cross-language cross-process — heavy.

Pick (a). The fixture is small enough (15 rules × ~3 examples
each ≈ 45 expected derived triples) that the test data can be
checked into `tests/fixtures/owl_rl_expected.nt`. If VG's
output ever drifts from this fixture, the spec on VG's side
fails first; if the engine's output drifts, this test fails.
Either failure is a real drift signal.

### Exit criteria for Phase E

```
cargo test               # all green
cargo build --release && cargo test --release    # see CLAUDE.md footgun note
```

Test count climbs by 8 (62 → 70 + 1 ignored).

---

## Phase F — docs

- **`README.md`** — new "OWL 2 RL native reasoning (since 0.9.0)"
  section with the example call shape, the 15-rule coverage
  note, and a pointer to PLAN_0.10.0 for the remaining 55
  rules. Roadmap checkbox: `[x] rdf_owl_rl_materialise (15-rule
  subset) — landed in 0.9.0`.

- **`CLAUDE.md`** — "SQL Function Reference" gains a new
  subsection "Reasoning" with `rdf_owl_rl_materialise`. New
  module in repo-layout diagram (`src/functions/rdf_owl_rl/`).
  Item 6 in "Completing the implementation" shifts down (Native
  OWL 2 RL is now item 6; gem wrapper shifts to 7; HTTP
  endpoint to 8).

- **`CHANGELOG.md`** — 0.9.0 entry. Lead with "Native OWL 2 RL
  rule pass — 15-rule subset". Cross-reference
  `CONSUMER_REQUIREMENT_VG.md` § "Requested extensions" item
  #6. Spell out the 0.9.0 scope (subset only, ~55 rules
  remaining) and the equivalence-with-VG-gem contract. Call
  out the `chrono` dep addition.

- **`Cargo.toml`** — add `chrono = { version = "0.4",
  default-features = false, features = ["clock"] }` to
  `[dependencies]`. Mention in CHANGELOG.

- **`src/functions/rdf_owl_rl.rs`** doc comment — table form,
  same style as `sparql_query.rs`'s table.

### `CONSUMER_REQUIREMENT_VG.md` graduation

Item #6 graduates from "Requested" to a new row in the "SPARQL
querying" or "Reasoning" table:

`rdf_owl_rl_materialise(asserted TEXT, inferred TEXT, options
TEXT) → INTEGER` — call site `Vv::Graph::Reasoner.materialise!`
(once VG bumps engine floor to ≥ 0.9.0 and routes through it).
Pin the 15-rule coverage, the JSON options shape, the
provenance predicates, the fixpoint contract, the max-iterations
error envelope.

In the "Requested extensions" section, replace item #6's full
block with a LANDED note pointing at the live row, same style
as item #9's 0.8.0 graduation.

### `CONSUMER_REQUIREMENT_MM.md` touchup

MM doesn't reason today. Add one line under "Available upstream
but not exercised by MM" pointing at the new function. Mirrors
how `rdf_construct_many` was added in 0.8.0.

### Exit criteria for Phase F

Reading `CONSUMER_REQUIREMENT_VG.md` top-to-bottom no longer
mentions item #6 in the "Requested" section. Reading
`CHANGELOG.md` shows a 0.9.0 entry naming the function and
the 15-rule scope.

---

## Phase G — tag 0.9.0

- Bump `Cargo.toml` and `VERSION` to `0.9.0`.
- `cargo test` and `cargo test --release` both green at the
  bumped version.
- `git tag v0.9.0` and push.
- Ping VG to open a follow-up plan that bumps its engine floor
  to ≥ 0.9.0 and routes `Vv::Graph::Reasoner.materialise!`
  through `rdf_owl_rl_materialise` when telemetry signals the
  per-rule path is the bottleneck.

---

## Risks

- **15-rule subset is partial OWL 2 RL.** Operators relying on
  full coverage (`cls-int1` / `cls-uni` for `owl:intersectionOf`
  / `owl:unionOf`, `prp-key` for `owl:hasKey`, the remaining
  ~52 rules) get a 0-derived result for those constructs. The
  CHANGELOG must call this out prominently — "0.9.0 ships
  parity with VG's current Phase B; if your ontology uses
  constructs outside that subset, the missing closures don't
  appear and you should stay on the per-rule
  `sparql_update` path until PLAN_0.10.0".

- **Provenance shape coupling to VG.** This release commits the
  engine to VG's `:derivedBy <urn:semantica:rule:<id>>` shape
  as the default. If VG changes its convention, the engine
  default has to change lockstep — the override mechanism
  (options JSON) softens this but only for callers who know to
  set it. Mitigation: the equivalence test fixture pins the
  full shape; drift fails fast.

- **`chrono` dependency addition.** Adds ~150 KB to the
  compiled `.dylib`. Acceptable for a Rust binary that already
  pulls in Oxigraph (~8 MB), but call it out in the CHANGELOG
  so packaging-conscious users (the gem-wrapper folks at
  0.10.0+) know. If the size hit is genuinely unacceptable, the
  hand-rolled RFC3339 formatter is a 30-line fallback.

- **Equivalence test fixture drift.** When VG ships new rules
  (PLAN_0.10.0 territory), the fixture file
  `tests/fixtures/owl_rl_expected.nt` must update lockstep with
  the engine + VG. Document the fixture-update procedure in
  `CLAUDE.md` under "Testing".

- **Performance claim ("order of magnitude faster") is unproven
  in 0.9.0.** The CR ask makes the claim; this plan inherits
  it. No benchmark is in Phase E. If performance is worse than
  VG's per-rule path (e.g., because the native pass over-iterates
  due to `quads_for_pattern` being slower than the SPARQL
  evaluator's plan caching), the release is technically a
  correctness win but a perf loss. Add a deferred Phase H
  "benchmark and document" if telemetry surfaces a regression
  signal.

- **`Store::contains` on every derived quad.** The dedup check
  inside the inner loop (`if store.contains(&asserted_quad)`) is
  O(log N) per check; for a million-triple closure that's
  millions of point lookups. Profile and consider a
  per-iteration HashSet of newly-derived quads for amortised O(1)
  dedup. Out of scope for Phase B/C; revisit if Phase E perf is
  a problem.

---

## Out of scope for 0.9.0

- **The remaining ~55 OWL 2 RL rules.** PLAN_0.10.0.

- **Native SHACL Core validator.** VG CR item #7. Substantial
  parallel effort; PLAN_0.11.0.

- **Native dependency index for DRed.** VG CR item #8. Gated on
  this plan (and #6 native pass) — the index is a write-through
  during `rdf_owl_rl_materialise`. PLAN_0.12.0.

- **Differential dataflow.** VG CR item #10. Explicitly marked
  "genuinely out-of-reach for incremental engine work" in the
  VG CR. Stays deferred.

- **Persistent RocksDB backend.** Stays deferred per PLAN_0.7.0.

- **Configurable rule subset selection.** `options.rules:
  ["scm-sco", "cax-sco"]` — operators picking which subset to
  apply. Could be useful for "warm fixpoint" workloads
  (re-running only the rules whose premises changed). Defer
  until a consumer asks; no signal today.

- **Streaming / per-iteration callback.** A version that reports
  progress per iteration via a SQLite callback. Operators with
  multi-million-triple closures might want this; no signal
  today.

---

## Re-numbering downstream milestones

`PLAN_0.2.0.md`'s roadmap table currently lists (after
PLAN_0.8.0's renumber):

| Version | Topic |
|---|---|
| 0.8.0 | Batched CONSTRUCT (`rdf_construct_many`) |
| 0.9.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.10.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend |

VG CR items #6, #7, #8 take priority over the packaging-shape
items (gem wrapper, HTTP endpoint). After this plan:

| Version | Topic |
|---|---|
| 0.8.0 | Batched CONSTRUCT (`rdf_construct_many`) |
| 0.9.0 | Native OWL 2 RL pass (15-rule subset) — this file |
| 0.10.0 | Full OWL 2 RL coverage (remaining ~55 rules) |
| 0.11.0 | Native SHACL Core validator (VG CR #7) |
| 0.12.0 | Native DRed dependency index (VG CR #8) |
| 0.13.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.14.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB backend; differential dataflow (VG CR #10) |

Update the table in `PLAN_0.2.0.md` as part of Phase F's doc
pass. Do **not** edit `PLAN_0.2.0.md`'s prose — the renumbering
+ four-row insertion is the only change there.

If a future consumer surfaces a strong signal for the
packaging-shape items before 0.10.0 / 0.11.0 / 0.12.0 land,
shuffle. The roadmap stays consumer-driven.
