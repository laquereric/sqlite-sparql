//! Native dependency index for DRed-style over-deletion (since 0.12.0).
//!
//! Maintained as a write-through during `rdf_owl_rl_materialise` when
//! `options_json` includes `"track_dependencies": true`. Consumed by
//! `rdf_dred_overdelete` to identify inferred quads whose support was
//! invalidated by a retracted premise.
//!
//! Per-derivation premise sets (rather than the union sketched in
//! `PLAN_0.12.0.md`) — multi-derivation correctness (test #7) requires
//! distinguishing "every derivation broken" from "some derivation
//! broken but one survives," and the per-derivation list lets the
//! cascade decide that without re-proving.
//!
//! Rule coverage in 0.12.0: the five W3C OWL 2 RL "core derivation"
//! rules whose forward shape is mechanical and which drive every
//! tracked test path — `scm-sco`, `scm-spo`, `eq-trans` (the three
//! `transitive_closure`-shape rules), `cax-sco`, and `prp-spo1`. The
//! remaining 55 rules from 0.10.0's set fire as before but do not
//! write through to the index; their derivations are invisible to
//! `rdf_dred_overdelete`. Expansion is mechanical and waits on a
//! consumer pull.

use oxigraph::model::Quad;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// One support derivation for an inferred quad — the set of premise
/// quads whose join produced this triple under some rule firing.
pub type Derivation = HashSet<Quad>;

#[derive(Default)]
pub struct DependencyIndex {
    /// Inferred quad → list of derivations (each a set of premise quads).
    /// A given inferred quad can have multiple independent derivations
    /// (e.g., `scm-sco` finds `:A ⊑ :C` via two intermediaries);
    /// over-delete only cascades when *every* derivation has been broken.
    derivations: HashMap<Quad, Vec<Derivation>>,
    /// Premise quad → set of inferred quads it supports under any
    /// derivation. Drives O(1) candidate lookup on retract.
    reverse: HashMap<Quad, HashSet<Quad>>,
}

impl DependencyIndex {
    /// Record one derivation: a rule fired on `premises` and produced
    /// `inferred`. Idempotent: an identical `(inferred, premises)` pair
    /// is not stored twice.
    pub fn record(&mut self, inferred: Quad, premises: Derivation) {
        let entries = self.derivations.entry(inferred.clone()).or_default();
        if entries.iter().any(|d| d == &premises) {
            return;
        }
        for p in &premises {
            self.reverse
                .entry(p.clone())
                .or_default()
                .insert(inferred.clone());
        }
        entries.push(premises);
    }

    /// Compute the over-delete cascade for a set of retracted premise
    /// quads. Returns the inferred quads whose every derivation became
    /// invalid (transitively). Does **not** mutate the index or the
    /// store — callers handle both.
    pub fn cascade(&self, retracted: &HashSet<Quad>) -> HashSet<Quad> {
        let mut removed: HashSet<Quad> = retracted.clone();
        let mut worklist: Vec<Quad> = retracted.iter().cloned().collect();
        let mut over_deleted: HashSet<Quad> = HashSet::new();

        while let Some(p) = worklist.pop() {
            let Some(candidates) = self.reverse.get(&p) else {
                continue;
            };
            // Snapshot candidate list to avoid borrowing through the loop.
            let candidates: Vec<Quad> = candidates.iter().cloned().collect();
            for c in candidates {
                if removed.contains(&c) {
                    continue;
                }
                let Some(derivations) = self.derivations.get(&c) else {
                    continue;
                };
                let any_survives = derivations
                    .iter()
                    .any(|d| d.iter().all(|q| !removed.contains(q)));
                if !any_survives {
                    removed.insert(c.clone());
                    over_deleted.insert(c.clone());
                    worklist.push(c);
                }
            }
        }
        over_deleted
    }

    /// Drop every reference to `inferred`. Called by `rdf_dred_overdelete`
    /// after the cascade has decided to remove this quad — keeps the
    /// reverse map from holding pointers into a quad that no longer
    /// exists in the store.
    pub fn forget(&mut self, inferred: &Quad) {
        // `inferred` may itself be a premise of other quads; the cascade
        // ensures those dependents are also over-deleted, but their
        // `forget` calls remove themselves from this reverse entry. By
        // the time we drop the entry it should be empty (or close to).
        self.reverse.remove(inferred);

        if let Some(derivations) = self.derivations.remove(inferred) {
            for d in derivations {
                for p in d {
                    let drop_premise = if let Some(set) = self.reverse.get_mut(&p) {
                        set.remove(inferred);
                        set.is_empty()
                    } else {
                        false
                    };
                    if drop_premise {
                        self.reverse.remove(&p);
                    }
                }
            }
        }
    }

    pub fn clear(&mut self) {
        self.derivations.clear();
        self.reverse.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.derivations.is_empty()
    }

    /// Test-only accessor for inspecting recorded derivations.
    #[cfg(test)]
    pub(crate) fn derivations_for(&self, inferred: &Quad) -> Option<&Vec<Derivation>> {
        self.derivations.get(inferred)
    }
}

static INDEX: OnceLock<Mutex<DependencyIndex>> = OnceLock::new();

fn index() -> &'static Mutex<DependencyIndex> {
    INDEX.get_or_init(|| Mutex::new(DependencyIndex::default()))
}

pub fn with_index<F, T>(f: F) -> T
where
    F: FnOnce(&mut DependencyIndex) -> T,
{
    let mut guard = index().lock().expect("dependency index mutex poisoned");
    f(&mut *guard)
}

pub fn clear_index() {
    with_index(|i| i.clear());
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{GraphName, NamedNode, Subject, Term};

    fn q(s: &str, p: &str, o: &str) -> Quad {
        Quad::new(
            Subject::NamedNode(NamedNode::new(s).unwrap()),
            NamedNode::new(p).unwrap(),
            Term::NamedNode(NamedNode::new(o).unwrap()),
            GraphName::DefaultGraph,
        )
    }

    #[test]
    fn record_and_cascade_single_derivation() {
        let mut idx = DependencyIndex::default();
        let p1 = q("urn:a", "urn:r", "urn:b");
        let p2 = q("urn:b", "urn:r", "urn:c");
        let inferred = q("urn:a", "urn:r", "urn:c");
        idx.record(inferred.clone(), [p1.clone(), p2.clone()].into_iter().collect());

        let mut retracted = HashSet::new();
        retracted.insert(p1.clone());
        let cascade = idx.cascade(&retracted);
        assert!(cascade.contains(&inferred));
        assert_eq!(cascade.len(), 1);
    }

    #[test]
    fn multi_derivation_survives_partial_retract() {
        // The inferred triple has two derivations; retract one premise
        // chain → the other derivation keeps the inferred alive.
        let mut idx = DependencyIndex::default();
        let inferred = q("urn:a", "urn:r", "urn:c");
        let p1a = q("urn:a", "urn:r", "urn:b1");
        let p1b = q("urn:b1", "urn:r", "urn:c");
        let p2a = q("urn:a", "urn:r", "urn:b2");
        let p2b = q("urn:b2", "urn:r", "urn:c");

        idx.record(
            inferred.clone(),
            [p1a.clone(), p1b.clone()].into_iter().collect(),
        );
        idx.record(
            inferred.clone(),
            [p2a.clone(), p2b.clone()].into_iter().collect(),
        );

        let mut retracted = HashSet::new();
        retracted.insert(p1a.clone());
        let cascade = idx.cascade(&retracted);
        assert!(!cascade.contains(&inferred), "derivation via p2 must keep inferred alive");
    }

    #[test]
    fn transitive_cascade_two_levels() {
        // p1 supports c1; c1 supports c2. Retract p1 → both cascade.
        let mut idx = DependencyIndex::default();
        let p1 = q("urn:p1", "urn:r", "urn:x");
        let c1 = q("urn:c1", "urn:r", "urn:y");
        let c2 = q("urn:c2", "urn:r", "urn:z");

        idx.record(c1.clone(), [p1.clone()].into_iter().collect());
        idx.record(c2.clone(), [c1.clone()].into_iter().collect());

        let mut retracted = HashSet::new();
        retracted.insert(p1);
        let cascade = idx.cascade(&retracted);
        assert!(cascade.contains(&c1));
        assert!(cascade.contains(&c2));
    }

    #[test]
    fn forget_cleans_reverse_index() {
        let mut idx = DependencyIndex::default();
        let p1 = q("urn:p", "urn:r", "urn:x");
        let inferred = q("urn:c", "urn:r", "urn:y");
        idx.record(inferred.clone(), [p1.clone()].into_iter().collect());
        idx.forget(&inferred);
        assert!(idx.is_empty());
        assert!(idx.reverse.get(&p1).is_none());
    }

    #[test]
    fn clear_empties_everything() {
        let mut idx = DependencyIndex::default();
        idx.record(
            q("urn:c", "urn:r", "urn:y"),
            [q("urn:p", "urn:r", "urn:x")].into_iter().collect(),
        );
        idx.clear();
        assert!(idx.is_empty());
    }
}
