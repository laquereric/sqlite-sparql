# PLAN 0.12.0 — Native dependency index for DRed

> Maintain a side-table inside the engine that maps each
> inferred-triple's quad ID to the set of premise-quad IDs that
> produced it during materialisation. Expose
> `rdf_dred_overdelete(inferred_iri, retracted_premises_json) →
> INTEGER` so DRed's over-deletion phase consults the index
> instead of pattern-matching against RDF-star `:derivedFrom`
> annotations on a dense provenance graph.

Driver: `CONSUMER_REQUIREMENT_VvGraph.md` § "Requested extensions"
item **#8 — Native dependency index for DRed**. VG's
`Vv::Graph::ChangeSet` / DRed loop (PLAN_0.11.0 Phase A landed on
the gem side) currently has no efficient over-deletion path —
the only way to find inferred triples whose support touches a
retracted premise is a SPARQL pattern-match against the
`:derivedFrom` annotation graph, which is O(retracted × inferred-
with-overlap). The native index makes the lookup O(log N) per
premise.

VG posture: same as PLAN_0.9.0 / PLAN_0.10.0 / PLAN_0.11.0 —
"revisit on first concrete bottleneck signal, not schedule a
release." Forward-leaning ship.

Depends on:
- 0.9.0 (the OWL 2 RL materialise — the index is populated as a
  write-through during `rdf_owl_rl_materialise`)
- 0.11.0 (the SHACL Core validator — if SHACL Rules ever
  emit-and-track via a materialise call, that's the second
  write-through site)

---

## Goal

`cargo test` passes a round-trip test that:

1. Loads a 5-triple T-Box + A-Box that triggers `scm-sco` and
   `cax-sco` (so the inferred graph has triples derived from
   specific premise triples).
2. Calls `rdf_owl_rl_materialise(...)` with the new
   `{"track_dependencies": true}` option.
3. Calls `rdf_dred_overdelete('urn:g:inferred',
   json('[["urn:s", "urn:p", "urn:o"]]'))` with a known premise
   triple.
4. Asserts the return integer equals the number of inferred
   triples whose derivation chain touched that premise.
5. Asserts those inferred triples are no longer in the inferred
   graph after the call.
6. Asserts inferred triples whose derivation did **not** touch
   the retracted premise are still present.

Plus a re-materialise check: after the over-delete, call
`rdf_owl_rl_materialise` again — any inferred triples that
remain derivable from the *other* premises should reappear
(the DRed re-derivation pass). The combination of over-delete
+ re-materialise is the full DRed cycle.

---

## What 0.12.0 covers vs. doesn't

| Status | Item |
|---|---|
| **0.12.0 in scope** | The dependency-index side-table; write-through hook on `rdf_owl_rl_materialise`; `rdf_dred_overdelete` read scalar; index persistence across `rdf_owl_rl_materialise` calls (in-process; the store is in-memory) |
| **0.12.0 out of scope** | Persistence across process restarts (the in-memory store doesn't survive a restart anyway — PLAN_0.7.0 deferred RocksDB indefinitely); SHACL Rules write-through hook (if `rdf_construct_many` ever feeds materialise it'll join the write-through sites; defer until VG asks) |

---

## Why a side-table, not RDF-star annotations

VG's existing path uses RDF-star annotations
(`<< s p o >> :derivedFrom << s' p' o' >>`) to track provenance.
Two reasons that's the wrong shape for DRed at scale:

- **Query cost.** Finding "every inferred triple whose support
  touches retracted premise `<s, p, o>`" is a SPARQL pattern
  match across the annotation graph — O(inferred × premises-per-
  inferred). For a dense provenance graph (high fan-in) this is
  the DRed bottleneck.
- **Storage cost.** Each derived triple generates 2 annotation
  quads in 0.9.0 (`:derivedBy`, `:derivedAt`). For DRed, we'd
  add 1+ more per *premise* (one `:derivedFrom` per premise quad
  used). For a rule with 3-premise body, that's 5 annotation
  quads per derived triple. The annotation graph balloons faster
  than the asserted graph.

The side-table is `HashMap<QuadId, Vec<QuadId>>` (or similar) —
O(N) total storage for N inferred quads, O(log N) lookup per
premise via reverse index. Domain-side concern; doesn't belong
in the data graph.

---

## Why expose as a separate scalar, not bake into `rdf_owl_rl_materialise`

`rdf_owl_rl_materialise`'s contract from 0.9.0 is "produce the
closure; emit RDF-star provenance optionally." DRed is a
different operation — given a set of retracted premises, return
the inferred triples that depended on them and remove them. The
two share the underlying dependency graph but not the user-facing
shape.

`rdf_dred_overdelete(inferred_iri, retracted_premises_json) →
INTEGER`:

- `retracted_premises_json`: JSON array of `[s, p, o]` or
  `[s, p, o, graph]` rows — same shape as `rdf_insert_many`'s
  argument (0.4.0 surface).
- Returns the count of over-deleted inferred triples.
- Reads the dependency index, computes the transitive closure of
  "inferred triples whose support touched any retracted premise,"
  removes them from `inferred_iri`.
- Updates the index accordingly (removes the over-deleted entries).
- Does NOT re-materialise. That's a separate
  `rdf_owl_rl_materialise` call the consumer makes after.

The over-delete + re-materialise sequence is DRed's
"delete-and-rederive" loop. Engine ships the primitives; consumer
sequences them.

---

## Phase A — dependency-index data structure

`src/store.rs` gets a sibling side-state:

```rust
/// Per-process dependency index. Populated as a write-through
/// during rdf_owl_rl_materialise; consumed by rdf_dred_overdelete.
///
/// Maps inferred quad-IDs to the premise quad-IDs that produced
/// them. A given inferred quad can have multiple derivations
/// (e.g., scm-sco might find :A ⊑ :C via :A ⊑ :B ⊑ :C and via
/// :A ⊑ :B' ⊑ :C from a separate intermediary); we store the
/// union of all premise sets.
static DEPENDENCY_INDEX: OnceLock<Mutex<DependencyIndex>> = OnceLock::new();

struct DependencyIndex {
    // inferred-quad-key → set-of-premise-quad-keys
    forward: HashMap<QuadKey, HashSet<QuadKey>>,
    // premise-quad-key → set-of-inferred-quad-keys (reverse index)
    reverse: HashMap<QuadKey, HashSet<QuadKey>>,
}

type QuadKey = (Subject, NamedNode, Term, GraphName);
```

The reverse index is what makes over-delete fast — given a
retracted premise, look up the inferred quads in O(1).

**Why a separate OnceLock + Mutex** rather than embedding in the
Oxigraph store: the store is `Send + Sync` and internally
concurrent, but its indexes don't include derivation
relationships (Oxigraph is a triple store, not a Datalog engine).
The dependency index is a domain-specific overlay; coupling it
to the store would be a layering inversion.

### Exit criteria for Phase A

`cargo build` clean. Index struct + lazy init + helpers
(`insert`, `lookup_inferred_by_premise`, `remove_inferred`) all
present, unit-tested.

---

## Phase B — write-through hook in `rdf_owl_rl_materialise`

Extend `MaterialiseOptions` with:

```rust
#[serde(default)]
pub track_dependencies: bool,
```

When `track_dependencies: true`, the fixpoint loop's per-rule
derivation collects `(derived_quad, premise_quads)` pairs and
writes them to the index. The rule library's `apply_*` functions
currently return `Vec<Triple>` — extend to
`Vec<DerivedTriple>` where `DerivedTriple` carries the premise
quads it was derived from:

```rust
pub struct DerivedTriple {
    pub triple: Triple,
    pub premises: Vec<Quad>,  // quads from asserted+inferred whose union derived this
}
```

This is a non-trivial rule-library change — each `apply_*`
function needs to track which premise quads contributed to each
derived triple. For `transitive_closure`, the premise pair is
the (`(x, pred, y)`, `(y, pred, z)`) tuple that produced
`(x, pred, z)`. For `cax-sco`, the premise pair is
(`(s, type, c1)`, `(c1, subClassOf, c2)`). Mechanical but
touches every rule.

When `track_dependencies: false` (default), the existing
`Vec<Triple>` path is preserved — no overhead for callers who
don't want DRed. Implement via two parallel code paths or via a
type that always carries premises but ignores them when not
tracking. Pick whichever is less code.

### Exit criteria for Phase B

`cargo build` clean. Materialise with
`{"track_dependencies": true}` populates the index (assert via a
debug-only `rdf_dred_inspect_index()` SQL function, or a
test-only Rust API on the store module).

---

## Phase C — `rdf_dred_overdelete` scalar

`src/functions/rdf_dred.rs`:

```rust
pub fn rdf_dred_overdelete_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let inferred_iri = ...;
    let retracted_premises_json = ...;
    let premises: Vec<[String; 3]> = serde_json::from_str(retracted_premises_json)?;

    let mut removed = 0i64;
    let index = ...; // lock DEPENDENCY_INDEX
    let mut to_remove = HashSet::new();
    for [s, p, o] in &premises {
        let premise_key = parse_to_key(s, p, o, inferred_iri)?;
        if let Some(inferred_set) = index.reverse.get(&premise_key) {
            to_remove.extend(inferred_set.iter().cloned());
        }
    }
    for inferred_key in &to_remove {
        // transitively expand — any inferred triple whose support
        // was *only* via to-be-removed triples is also over-deleted.
        // (Cascade is bounded by the index depth.)
    }

    // Delete from store + index.
    let store = with_store(|s| s);
    for key in &to_remove {
        let quad = key_to_quad(key);
        store.remove(&quad)?;
        index.remove_inferred(key);
        removed += 1;
    }

    api::result_int64(context, removed);
    Ok(())
}
```

The transitive cascade is the subtle part — DRed's over-delete
removes not just direct dependents but also second-order
dependents whose support relied on a now-removed inferred triple.
The loop iterates until no new triples are added to `to_remove`.

### Exit criteria for Phase C

`cargo build` clean. Function registered, smoke test via SQLite
CLI confirms the basic shape.

---

## Phase D — integration tests

Add to `tests/integration_test.rs` under
`// ── 0.12.0 rdf_dred_overdelete ──`:

1. `test_rdf_dred_overdelete_direct_dependency` — single
   premise removed; one direct inferred dependent removed.
2. `test_rdf_dred_overdelete_transitive_cascade` — premise removed
   that has 2 levels of inferred descendants; all transitively
   removed.
3. `test_rdf_dred_overdelete_preserves_other_inferences` —
   inferred triples not depending on the retracted premise stay.
4. `test_rdf_dred_overdelete_no_op_when_no_dependents` — premise
   that doesn't appear in the index → return 0, no changes.
5. `test_rdf_dred_overdelete_requires_track_dependencies` —
   over-delete called against an index that wasn't populated
   (materialise without `track_dependencies: true`) → return 0
   with a warning, or error with a fixed-prefix
   "rdf_dred_overdelete: no dependency index — re-run
   `rdf_owl_rl_materialise` with `track_dependencies: true`."
   Pick one; document.
6. `test_rdf_dred_full_cycle_overdelete_then_rematerialise` —
   the canonical DRed cycle: materialise, retract a premise,
   over-delete (count N), re-materialise — re-derivable triples
   reappear.
7. `test_rdf_dred_overdelete_multi_derivation` — an inferred
   triple with two independent derivations. Removing one premise
   chain keeps the inferred triple (still derivable via the
   other). The index correctly accounts for this.
8. `test_rdf_dred_overdelete_clears_index_entry` — after
   over-delete, the removed inferred triples no longer appear in
   the index (re-materialise without the premise → empty;
   re-materialise with the premise added back → reappears).

### Exit criteria for Phase D

```
cargo test                # all green
cargo build --release && cargo test --release    # see CLAUDE.md footgun
```

---

## Phase E — docs

- **`README.md`** — new "Incremental reasoning with DRed (since
  0.12.0)" section.
- **`CLAUDE.md`** — new SQL function entry under "Reasoning /
  validation" subsection.
- **`CHANGELOG.md`** — 0.12.0 entry leading with the
  "delete-and-rederive" framing, the new
  `track_dependencies` option on materialise, the trade-off
  ("opt-in because tracking is a real cost"), and the index
  semantics.
- **`CONSUMER_REQUIREMENT_VvGraph.md`** — item #8 graduates.
- **`CONSUMER_REQUIREMENT_MM.md`** — add line under "available
  upstream but not exercised by MM."
- **`PLAN_0.2.0.md`** — roadmap renumber.

---

## Phase F — tag

Same as PLAN_0.9.0/0.9.1/0.11.0. Chain `git add … && git commit …`
in a single invocation.

---

## Risks

- **Index storage cost.** For an `N`-quad inferred graph with
  `k`-fan-in premise sets, the index is `O(N * k)` quad
  references. For dense ontologies this can dwarf the inferred
  graph itself. Mitigation: store quad references as compact
  `(u32, u32)` IDs (Oxigraph internal IDs) rather than full
  Quad clones. Requires accessing Oxigraph's internal ID space,
  which isn't part of its public API in 0.4. If that's not
  available, fall back to a content-hash key
  (`u64` BLAKE3 of the canonical quad string) — collisions are
  cosmologically improbable.
- **Write-through cost.** Tracking premises per derivation roughly
  doubles `rdf_owl_rl_materialise`'s allocation cost — every
  derived triple now carries a `Vec<Quad>` instead of being
  built and dropped. Telemetry should confirm the over-delete
  speedup justifies the materialise slowdown. Keep
  `track_dependencies: false` as the default.
- **Transitive cascade correctness.** The over-delete cascade is
  easy to get wrong — must distinguish "this inferred triple's
  *only* support was via removed triples" from "this inferred
  triple has *some* support via removed triples but also other
  support." Only the former should cascade. The forward index
  (`inferred → premises`) is needed to check "are there other
  supports?" — this is why we maintain both forward and reverse
  indexes.
- **Stale index across `rdf_clear`.** If `rdf_clear()` is called,
  the dependency index must be cleared too — otherwise
  subsequent over-delete operations reference stale quad keys.
  Phase A's `DependencyIndex` needs a `clear()` method and the
  `rdf_clear` function needs to call it.
- **Index persistence question.** The store is in-memory (no
  RocksDB per PLAN_0.7.0 / PLAN_0.9.0). The index is also
  in-memory. Both vanish on process restart. After restart,
  `rdf_dred_overdelete` returns 0 until a fresh
  `rdf_owl_rl_materialise(track_dependencies: true)` repopulates
  it. Document this.
- **No SHACL Rules write-through.** PLAN_0.8.0's
  `rdf_construct_many` doesn't currently feed into the
  dependency index — only `rdf_owl_rl_materialise` does. If VG
  ever uses `rdf_construct_many` for SHACL Rules materialisation
  AND wants DRed over those derivations, this plan would expand.
  Deferred until that consumer pull surfaces.

---

## Out of scope

- **Cross-process / cross-restart index persistence.** Tied to
  the deferred RocksDB plan; revives only if the store backend
  becomes persistent.
- **Native re-materialise after over-delete.** The
  `rdf_dred_overdelete` scalar returns; the consumer calls
  `rdf_owl_rl_materialise` again. Combining the two into a
  single `rdf_dred_step(inferred, retracted_premises) → INTEGER`
  scalar would be ergonomically nicer, but the engine stays
  composable by keeping them separate. Add the combined scalar
  if a consumer asks.
- **Differential dataflow** (VG CR #10) — explicitly out-of-reach
  per the VG CR.

---

## Re-numbering downstream milestones

| Version | Topic |
|---|---|
| 0.12.0 | Native DRed dependency index (this file) |
| 0.13.0 | `sqlite-sparql-ruby` gem wrapper |
| 0.14.0 | SPARQL HTTP endpoint |
| Deferred | Persistent RocksDB; differential dataflow |

Stays unchanged from PLAN_0.11.0's renumber unless 0.10.0 or
0.11.0 split across sub-releases first.
