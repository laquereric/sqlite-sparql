# PLAN 0.10.0 — Full OWL 2 RL coverage

> Expand the OWL 2 RL rule library from the 15-rule subset shipped
> in 0.9.0 to the full W3C OWL 2 RL/RDF rule table. Mechanical
> transcription — the engine surface, fixpoint plumbing, provenance
> emission, and dispatch are all in place from 0.9.0. Each new rule
> is a pure-Rust `Store::quads_for_pattern` query plus an entry in
> the static `RULES` array.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions"
item #6 (graduated as 15-rule subset in 0.9.0). The CR explicitly
calls out that 0.9.0's coverage matches `vv-graph`'s
`Vv::Graph::Reasoner::Rules::OwlRl` and that the remaining ~55
rules land here. VG's `Vv::Graph::Reasoner::Rules::PHASE_B_PENDING`
list is the authoritative scope ledger; if a rule leaves
`PHASE_B_PENDING` on the gem side, it joins this plan's scope on
the engine side. Lockstep.

Posture: **gated on `vv-graph` first.** Until VG's `OwlRl` rule
set expands beyond the 15 rules from 0.9.0, the engine's parity
contract (the `test_rdf_owl_rl_materialise_equivalence_with_vg`
test) holds trivially and shipping 0.10.0 buys nothing observable.
This plan is a **prepared plan** — it lays out the design so the
engine work can ship within hours of VG's expansion, not a
schedule-now plan.

Depends on 0.9.0 (the fixpoint loop, `RULES` dispatch, helper
library, provenance machinery — all reused unchanged).

---

## Goal

`cargo test` passes the `test_rdf_owl_rl_materialise_equivalence_with_vg`
test against an expanded fixture that exercises every rule
currently in VG's `OwlRl::RULES` (not just the 15 from 0.9.0).
When VG's rule set grows, the test fixture grows with it, and the
engine's `RULES` array grows lockstep — pinned by an explicit
"this engine release matches `vv-graph` revision X" assertion in
the CHANGELOG.

---

## What's in / out of scope per cycle

The full W3C OWL 2 RL/RDF rule table has ~70 rules. The 15
shipped in 0.9.0 are the "transcribed in VG's `Rules::OwlRl`
module" subset. The remaining bucket breaks down roughly as:

| Bucket | Example rules | Engine cost |
|---|---|---|
| Property characteristics (extended) | `prp-irp`, `prp-asyp`, `prp-pdw`, `prp-adp`, `prp-npa1`, `prp-npa2` | Mechanical |
| Equivalent / disjoint class axioms | `cax-eqc1`, `cax-eqc2`, `cax-dw` | Mechanical |
| Class expressions (intersection / union) | `cls-int1`, `cls-int2`, `cls-uni`, `cls-com`, `cls-svf1`, `cls-svf2`, `cls-avf`, `cls-maxc1`, `cls-maxc2`, `cls-maxqc1`–`cls-maxqc4` | Non-trivial — `owl:intersectionOf` / `owl:unionOf` use RDF list traversal (`rdf:first` / `rdf:rest`), which needs a small list-walker helper |
| Datatype reasoning | `dt-type1`, `dt-type2`, `dt-not-type`, `dt-eq`, `dt-diff`, `dt-diff2` | Requires Oxigraph's datatype comparison helpers; XSD facet handling |
| Equality reasoning (extended) | `eq-rep-s`, `eq-rep-p`, `eq-rep-o`, `eq-diff1` | Replacement rules — substitute one term for another across all positions. Heavy on the inferred graph |
| OWL property keys | `prp-key` | Multi-property functional-dependency check |
| Some entailment-table closures | `scm-cls`, `scm-int`, `scm-uni`, `scm-svf*`, `scm-avf*`, `scm-dom1`, `scm-dom2`, `scm-rng1`, `scm-rng2`, `scm-hv` | Mechanical once the class-expression helpers exist |

Recommendation: **don't try to ship all ~55 in one release**.
Split into two or three sub-releases as VG's `PHASE_B_PENDING`
shrinks. A defensible decomposition:

- **0.10.0 — "property characteristics + equivalent / disjoint
  classes"** (~12 rules). All mechanical, no new helpers. Ships
  parity with whatever VG ships first.
- **0.11.0 — "class expressions + scm-cls / scm-int / scm-uni"**
  (~15 rules). Needs the RDF-list-walker helper. Ships when VG
  ships `cls-int1` / `cls-uni`.
- **0.12.0 — "datatype reasoning + equality replacement + keys"**
  (remainder). Heavier — datatype comparison and term-replacement
  are non-trivial.

If the user prefers a single 0.10.0 with all ~55 rules, the
phases below collapse into one big rule-transcription pass; the
list-walker and datatype helpers land in Phase B regardless.

The roadmap (PLAN_0.2.0.md) currently allocates **one 0.10.0
slot** for "full OWL 2 RL coverage." Whether that becomes one big
release or three smaller ones is a call to make at Phase A time,
informed by how much VG ships at once.

---

## Phase A — confirm the rule subset

Re-read VG's `lib/vv/graph/reasoner/rules/owl_rl.rb` (and the
`PHASE_B_PENDING` list at the bottom of that module). Pick the
rules that have *graduated* from `PHASE_B_PENDING` since 0.9.0
shipped. This list is the scope.

If the list is empty (VG hasn't expanded), close the plan as
"deferred — no consumer-side movement" and skip the release.
The plan stays on the roadmap; revive on next VG bump.

### Exit criteria for Phase A

A short list (in this file's commit message at ship time) of which
rules are in 0.10.0 scope, plus the VG revision the parity claim
pins against.

---

## Phase B — transcribe the rules

For each rule:

1. Add `fn apply_<rule_iri>(store, asserted, inferred) -> Result<Vec<Triple>>`
   to `src/functions/rdf_owl_rl/rules.rs`. Match the existing
   style — pure-Rust pattern queries via `Store::quads_for_pattern`,
   no SPARQL parser hits, hash-map joins for transitive rules,
   `subj_to_term` / `term_to_subj` for the polymorphic positions.
2. Add a `Rule { iri: "<rule_iri>", apply: apply_<rule_iri> }`
   entry to `RULES`.
3. Add a small `#[cfg(test)] mod tests` smoke unit test in
   `rules.rs` exercising the rule against a 2–4-quad fixture.

The 0.9.0 helpers (`pairs_for_predicate`, `instances_of`,
`all_quads`, `transitive_closure`, `equivalent_to_subsumption`,
`domain_or_range`, `inverse_of`) cover the structural patterns
already in scope. New helpers needed for the expanded set:

- **`rdf_list_members(store, list_head, asserted, inferred) → Vec<Term>`**
  — walks an RDF list (`rdf:first` / `rdf:rest`) starting from
  `list_head`, returning the member terms in order. Used by
  `cls-int*` / `cls-uni` / `prp-key`. Tested independently —
  malformed lists (cycle, missing `rdf:rest`, non-`rdf:nil`
  termination) error cleanly.
- **`literals_with_datatype(store, datatype, asserted, inferred) → Vec<(Subject, Literal)>`**
  — used by `dt-type1` / `dt-not-type`. Wraps the per-literal
  datatype check.

### Exit criteria for Phase B

`cargo build` clean. Unit-test smokes per added rule pass. The
existing 0.9.0 tests pass unchanged.

---

## Phase C — extend the equivalence-with-vg fixture

`test_rdf_owl_rl_materialise_equivalence_with_vg` in
`tests/integration_test.rs` hand-writes the expected closure for
the 0.9.0 fixture. For 0.10.0, the fixture needs to:

1. Add input triples that exercise each new rule.
2. Add ASK queries (or a counted SELECT) for every new derived
   triple the expanded rule set should produce.
3. Pin the VG revision the expected closure was hand-computed
   against, in a fixture-header comment.

If the fixture grows past ~30 ASKs, move it from inline to
`tests/fixtures/owl_rl_expected.nt` and load it via
`rdf_load_ntriples` for comparison via SPARQL `MINUS`. The PLAN_0.9.0
decision was "inline ASKs over a fixture file"; revisit at Phase C
time if the inline form gets unwieldy.

### Exit criteria for Phase C

```
cargo test                # all green, including the expanded equivalence test
cargo build --release && cargo test --release    # see CLAUDE.md footgun
```

---

## Phase D — docs

- **`README.md`** — extend the "OWL 2 RL native reasoning" section's
  rule-coverage paragraph to name the newly-covered buckets.
  Roadmap checkbox `[x] Full OWL 2 RL coverage (remaining ~55 rules)
  — landed in 0.10.0` (or split across subsequent releases per
  the decomposition above).
- **`CLAUDE.md`** — "SQL Function Reference" → "Reasoning" subsection
  needs no surface change (the function signature is unchanged from
  0.9.0). Just bump the "15 rules → full / ~70 rules / chosen
  subset" prose.
- **`CHANGELOG.md`** — 0.10.0 entry leads with the rule-count delta
  and the specific buckets covered. Pin the VG revision the
  equivalence claim is against ("matches `vv-graph` @ <SHA>").
- **`CONSUMER_REQUIREMENT_VvGraph.md`** — update item #6's LANDED
  note to reflect the expanded coverage. If 0.10.0 ships
  full-coverage, drop the "remaining ~55 rules" caveat entirely.
- **`PLAN_0.2.0.md`** — only renumber if the decomposition splits
  the work across multiple releases (see Phase A's open question).
  Otherwise leave the roadmap as-is.

### Exit criteria for Phase D

CHANGELOG names the rules added in this release. VG CR no longer
mentions a pending subset (or names a smaller one).

---

## Phase E — tag

- Bump `Cargo.toml` and `VERSION` (likely to `0.10.0`, or
  `0.10.0`/`0.11.0`/`0.12.0` per the decomposition).
- `cargo test` + `cargo test --release` green at the bumped
  version.
- **Chain `git add … && git commit …` in a single bash invocation**
  to prevent the index-reset incident that hit v0.9.0. Re-verify
  `git status --short` immediately before the commit step. See
  `PLAN_0.9.1.md` for the operational background.
- `git tag v0.10.0` and `git push origin v0.10.0`.

---

## Risks

- **Rule complexity beyond the 0.9.0 pattern.** Some rules
  (`cls-maxqc*`, `prp-key`) need multi-way joins more elaborate
  than the existing helpers. Profile + factor as helpers grow.
  Premature abstraction risk on the helper layer — write the
  rules first, factor when three rules share shape.
- **Equivalence test fixture size.** Hand-writing the expected
  closure for 70 rules is tedious + error-prone. Phase C's
  fall-back-to-fixture-file is a release valve. The Hinnant-style
  reading-of-the-spec lookup table approach also works — encode
  each rule's expected output for a small canonical input and
  diff via SPARQL `MINUS`.
- **Datatype reasoning needs XSD facet handling.** `dt-type1` /
  `dt-not-type` / `dt-diff` depend on whether two literals are
  "the same value" under XSD semantics (e.g., `"1"^^xsd:integer`
  vs. `"01"^^xsd:integer`). Oxigraph's `Literal` exposes
  `value()` and `datatype()` but not a built-in `xsd_equals` —
  may need a small XSD-canonicalisation helper. Out-of-scope to
  build a full XSD value-space comparator; reuse Oxigraph's
  internal `xsd_lexical_to_value` if exposed, otherwise refuse
  with a clear error for the heavier datatype rules and ship
  the simpler subset.
- **Equality-replacement rules (`eq-rep-s` / `eq-rep-p` /
  `eq-rep-o`) explode the inferred graph.** For every `?x owl:sameAs ?y`,
  these rules emit a substituted copy of every triple
  mentioning `?x`. For dense sameAs cliques this is quadratic.
  Document the explosion; consider an opt-out via `options.rules`
  exclude list (deferred from 0.9.0 — revive if these rules ship).
- **VG drift.** If VG ships a rule with a non-W3C-spec interpretation
  (extension, bug, deliberate simplification), the engine's
  parity contract forces the engine to mirror VG, not the spec.
  Document this when it happens; the equivalence test is the
  authority.

---

## Out of scope

- **Configurable rule subset selection** (`options.rules:
  ["scm-sco", "cax-sco"]`) — deferred from 0.9.0; revive if a
  consumer asks. Useful for "warm fixpoint" workloads (re-running
  only the rules whose premises changed) and as the opt-out for
  the heavy equality-replacement rules above.
- **Streaming per-iteration callback** — also deferred from 0.9.0.
- **Native datatype value-space comparator** beyond what Oxigraph
  exposes — see datatype-reasoning risk above. If `dt-*` rules
  prove too dependent on this, scope-cut the datatype subset.
- **OWL 2 DL or OWL 2 EL reasoning** — out of scope; this plan
  is OWL 2 RL only. Operators needing the other profiles need a
  different engine entirely (e.g., a tableaux reasoner shelled
  out via UDF). Not on the roadmap; no consumer pull.

---

## Re-numbering downstream milestones

If 0.10.0 ships as a single full-coverage release, the roadmap
stays as `PLAN_0.2.0.md` already lists:

| Version | Topic |
|---|---|
| 0.10.0 | Full OWL 2 RL coverage (this file) |
| 0.11.0 | Native SHACL Core validator (VG CR #7) |
| 0.12.0 | Native DRed dependency index (VG CR #8) |

If 0.10.0 splits across multiple sub-releases (the decomposition
in "What's in / out of scope per cycle" above), the SHACL +
DRed plans shift right by however many slots the OWL 2 RL
expansion consumes. Update `PLAN_0.2.0.md` table at that point.
