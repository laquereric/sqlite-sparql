//! OWL 2 RL/RDF rule library — 15-rule subset matching VG's Phase B.
//!
//! Each rule queries the union of `asserted_graph` and `inferred_graph`
//! for its premises and returns `Vec<Triple>` of newly-derivable triples.
//! Dedup against existing inferred-graph contents happens in the fixpoint
//! loop (`super::execute_materialise`), not here — rules can freely
//! re-emit triples; the loop's `Store::contains` check filters.
//!
//! Naming: function names mirror the W3C OWL 2 RL/RDF rule table verbatim
//! (`scm-sco` → `apply_scm_sco`, etc.). See
//! <https://www.w3.org/TR/owl2-profiles/#Reasoning_in_OWL_2_RL_and_RDF_Graphs_using_Rules>.

use crate::error::{Result, SparqlError};
use oxigraph::model::{
    BlankNodeRef, GraphName, GraphNameRef, NamedNode, NamedNodeRef, Subject, SubjectRef, Term,
    TermRef, Triple,
};
use oxigraph::store::Store;
use std::collections::{HashMap, HashSet};

// ── Constant IRIs (well-known, zero validation cost via `_unchecked`) ────────

const RDF_TYPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
const RDFS_SUB_CLASS_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subClassOf");
const RDFS_SUB_PROPERTY_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subPropertyOf");
const RDFS_DOMAIN: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#domain");
const RDFS_RANGE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#range");
const OWL_EQUIVALENT_CLASS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#equivalentClass");
const OWL_EQUIVALENT_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#equivalentProperty");
const OWL_INVERSE_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#inverseOf");
const OWL_SAME_AS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#sameAs");
const OWL_TRANSITIVE_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#TransitiveProperty");
const OWL_SYMMETRIC_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#SymmetricProperty");
const OWL_FUNCTIONAL_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#FunctionalProperty");

// ── Rule dispatch ────────────────────────────────────────────────────────────

/// One OWL 2 RL rule. `iri` is the short name from the W3C rule table
/// (e.g. `"scm-sco"`); the fixpoint loop concatenates it with the
/// `rule_iri_prefix` option to form the full provenance IRI.
pub(crate) struct Rule {
    pub iri: &'static str,
    pub apply: fn(&Store, &GraphName, &GraphName) -> Result<Vec<Triple>>,
}

pub(crate) static RULES: &[Rule] = &[
    Rule { iri: "scm-sco",   apply: apply_scm_sco   },
    Rule { iri: "scm-spo",   apply: apply_scm_spo   },
    Rule { iri: "scm-eqc1",  apply: apply_scm_eqc1  },
    Rule { iri: "scm-eqp1",  apply: apply_scm_eqp1  },
    Rule { iri: "cax-sco",   apply: apply_cax_sco   },
    Rule { iri: "prp-spo1",  apply: apply_prp_spo1  },
    Rule { iri: "prp-dom",   apply: apply_prp_dom   },
    Rule { iri: "prp-rng",   apply: apply_prp_rng   },
    Rule { iri: "prp-trp",   apply: apply_prp_trp   },
    Rule { iri: "prp-symp",  apply: apply_prp_symp  },
    Rule { iri: "prp-inv1",  apply: apply_prp_inv1  },
    Rule { iri: "prp-inv2",  apply: apply_prp_inv2  },
    Rule { iri: "prp-fp",    apply: apply_prp_fp    },
    Rule { iri: "eq-sym",    apply: apply_eq_sym    },
    Rule { iri: "eq-trans",  apply: apply_eq_trans  },
];

// ── Helpers ──────────────────────────────────────────────────────────────────

fn graph_to_ref(g: &GraphName) -> GraphNameRef<'_> {
    match g {
        GraphName::DefaultGraph => GraphNameRef::DefaultGraph,
        GraphName::NamedNode(n) => GraphNameRef::NamedNode(n.as_ref()),
        GraphName::BlankNode(b) => GraphNameRef::BlankNode(b.as_ref()),
    }
}

/// Iterate the asserted graph then the inferred graph. If they're the
/// same graph (operator-supplied edge case from PLAN_0.9.0 Phase D),
/// iterate it once.
fn graphs_to_query<'a>(a: &'a GraphName, i: &'a GraphName) -> Vec<&'a GraphName> {
    if a == i {
        vec![a]
    } else {
        vec![a, i]
    }
}

/// Collect every `(subject, object)` for the given predicate from the
/// premise graphs. Deduplicates across asserted + inferred.
fn pairs_for_predicate(
    store: &Store,
    predicate: NamedNodeRef<'_>,
    asserted: &GraphName,
    inferred: &GraphName,
) -> Result<HashSet<(Subject, Term)>> {
    let mut out = HashSet::new();
    for graph in graphs_to_query(asserted, inferred) {
        let g_ref = graph_to_ref(graph);
        for q in store.quads_for_pattern(None, Some(predicate), None, Some(g_ref)) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.insert((q.subject, q.object));
        }
    }
    Ok(out)
}

/// Collect every subject `?s` whose `(?s rdf:type <target_class>)` holds.
fn instances_of(
    store: &Store,
    target_class: NamedNodeRef<'_>,
    asserted: &GraphName,
    inferred: &GraphName,
) -> Result<HashSet<Subject>> {
    let mut out = HashSet::new();
    let target_term = TermRef::NamedNode(target_class);
    for graph in graphs_to_query(asserted, inferred) {
        let g_ref = graph_to_ref(graph);
        for q in store.quads_for_pattern(None, Some(RDF_TYPE), Some(target_term), Some(g_ref)) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.insert(q.subject);
        }
    }
    Ok(out)
}

/// Collect every quad in the asserted+inferred union. Used by rules whose
/// premise pattern is `?s ?p ?o` (e.g. `prp-spo1`, `prp-dom`, `prp-rng`).
fn all_quads(
    store: &Store,
    asserted: &GraphName,
    inferred: &GraphName,
) -> Result<HashSet<(Subject, NamedNode, Term)>> {
    let mut out = HashSet::new();
    for graph in graphs_to_query(asserted, inferred) {
        let g_ref = graph_to_ref(graph);
        for q in store.quads_for_pattern(None, None, None, Some(g_ref)) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.insert((q.subject, q.predicate, q.object));
        }
    }
    Ok(out)
}

/// Subject → Term coercion for use as a join key. Always succeeds.
fn subj_to_term(s: &Subject) -> Term {
    match s {
        Subject::NamedNode(n) => Term::NamedNode(n.clone()),
        Subject::BlankNode(b) => Term::BlankNode(b.clone()),
        Subject::Triple(t) => Term::Triple(t.clone()),
    }
}

/// Term → Subject coercion; returns None when the term is a Literal
/// (literals are not allowed in subject position in RDF).
fn term_to_subj(t: &Term) -> Option<Subject> {
    match t {
        Term::NamedNode(n) => Some(Subject::NamedNode(n.clone())),
        Term::BlankNode(b) => Some(Subject::BlankNode(b.clone())),
        Term::Triple(t) => Some(Subject::Triple(t.clone())),
        Term::Literal(_) => None,
    }
}

/// Term → NamedNode coercion; returns None for any other term type. Used
/// when a rule needs to bind a predicate variable (predicates must be IRIs).
fn term_to_named(t: &Term) -> Option<NamedNode> {
    match t {
        Term::NamedNode(n) => Some(n.clone()),
        _ => None,
    }
}

/// Build a `Triple` (the predicate is always a NamedNode).
fn triple(s: Subject, p: NamedNodeRef<'_>, o: Term) -> Triple {
    Triple::new(s, p.into_owned(), o)
}

// ── Rules ────────────────────────────────────────────────────────────────────

/// `scm-sco`: `?c1 rdfs:subClassOf ?c2 . ?c2 rdfs:subClassOf ?c3` →
/// `?c1 rdfs:subClassOf ?c3`
fn apply_scm_sco(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    transitive_closure(store, RDFS_SUB_CLASS_OF, a, i)
}

/// `scm-spo`: transitive closure of `rdfs:subPropertyOf`.
fn apply_scm_spo(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    transitive_closure(store, RDFS_SUB_PROPERTY_OF, a, i)
}

/// `eq-trans`: transitive closure of `owl:sameAs`.
fn apply_eq_trans(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    transitive_closure(store, OWL_SAME_AS, a, i)
}

/// Generic transitive closure helper for predicates where subject and
/// object are both nodes (NamedNode or BlankNode).
fn transitive_closure(
    store: &Store,
    predicate: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let pairs = pairs_for_predicate(store, predicate, a, i)?;
    // adjacency: subject → set of objects
    let mut adjacency: HashMap<Subject, HashSet<Term>> = HashMap::new();
    for (s, o) in &pairs {
        adjacency.entry(s.clone()).or_default().insert(o.clone());
    }
    let mut derived = Vec::new();
    for (x, y) in &pairs {
        let Some(y_as_subj) = term_to_subj(y) else {
            continue;
        };
        let Some(z_set) = adjacency.get(&y_as_subj) else {
            continue;
        };
        for z in z_set {
            derived.push(triple(x.clone(), predicate, z.clone()));
        }
    }
    Ok(derived)
}

/// `scm-eqc1`: `?c1 owl:equivalentClass ?c2` →
/// `?c1 rdfs:subClassOf ?c2` AND `?c2 rdfs:subClassOf ?c1`
fn apply_scm_eqc1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_to_subsumption(store, OWL_EQUIVALENT_CLASS, RDFS_SUB_CLASS_OF, a, i)
}

/// `scm-eqp1`: `?p1 owl:equivalentProperty ?p2` →
/// `?p1 rdfs:subPropertyOf ?p2` AND `?p2 rdfs:subPropertyOf ?p1`
fn apply_scm_eqp1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_to_subsumption(store, OWL_EQUIVALENT_PROPERTY, RDFS_SUB_PROPERTY_OF, a, i)
}

fn equivalent_to_subsumption(
    store: &Store,
    equiv_predicate: NamedNodeRef<'_>,
    sub_predicate: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let pairs = pairs_for_predicate(store, equiv_predicate, a, i)?;
    let mut derived = Vec::new();
    for (c1, c2) in pairs {
        let Some(c2_as_subj) = term_to_subj(&c2) else {
            continue;
        };
        derived.push(triple(c1.clone(), sub_predicate, c2.clone()));
        derived.push(triple(c2_as_subj, sub_predicate, subj_to_term(&c1)));
    }
    Ok(derived)
}

/// `cax-sco`: `?s rdf:type ?c1 . ?c1 rdfs:subClassOf ?c2` →
/// `?s rdf:type ?c2`
fn apply_cax_sco(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let sub_class_pairs = pairs_for_predicate(store, RDFS_SUB_CLASS_OF, a, i)?;
    let type_pairs = pairs_for_predicate(store, RDF_TYPE, a, i)?;
    // class → set of superclasses (as Term)
    let mut super_of: HashMap<Subject, HashSet<Term>> = HashMap::new();
    for (c1, c2) in &sub_class_pairs {
        super_of.entry(c1.clone()).or_default().insert(c2.clone());
    }
    let mut derived = Vec::new();
    for (s, c1) in &type_pairs {
        let Some(c1_as_subj) = term_to_subj(c1) else {
            continue;
        };
        let Some(supers) = super_of.get(&c1_as_subj) else {
            continue;
        };
        for c2 in supers {
            derived.push(triple(s.clone(), RDF_TYPE, c2.clone()));
        }
    }
    Ok(derived)
}

/// `prp-spo1`: `?s ?p1 ?o . ?p1 rdfs:subPropertyOf ?p2` →
/// `?s ?p2 ?o`
fn apply_prp_spo1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let sub_prop_pairs = pairs_for_predicate(store, RDFS_SUB_PROPERTY_OF, a, i)?;
    // property IRI → set of super-property IRIs
    let mut super_of: HashMap<NamedNode, HashSet<NamedNode>> = HashMap::new();
    for (p1, p2) in sub_prop_pairs {
        // Both p1 and p2 must be IRIs (predicates) for this rule to apply.
        let Subject::NamedNode(p1_n) = p1 else {
            continue;
        };
        let Some(p2_n) = term_to_named(&p2) else {
            continue;
        };
        super_of.entry(p1_n).or_default().insert(p2_n);
    }
    let quads = all_quads(store, a, i)?;
    let mut derived = Vec::new();
    for (s, p1, o) in &quads {
        let Some(supers) = super_of.get(p1) else {
            continue;
        };
        for p2 in supers {
            derived.push(Triple::new(s.clone(), p2.clone(), o.clone()));
        }
    }
    Ok(derived)
}

/// `prp-dom`: `?s ?p ?o . ?p rdfs:domain ?c` → `?s rdf:type ?c`
fn apply_prp_dom(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range(store, RDFS_DOMAIN, /* use subject */ true, a, i)
}

/// `prp-rng`: `?s ?p ?o . ?p rdfs:range ?c` → `?o rdf:type ?c`
fn apply_prp_rng(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range(store, RDFS_RANGE, /* use subject */ false, a, i)
}

fn domain_or_range(
    store: &Store,
    dr_predicate: NamedNodeRef<'_>,
    use_subject: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let dr_pairs = pairs_for_predicate(store, dr_predicate, a, i)?;
    // property → set of domain/range classes
    let mut classes_of: HashMap<NamedNode, HashSet<Term>> = HashMap::new();
    for (p, c) in dr_pairs {
        let Subject::NamedNode(p_n) = p else {
            continue;
        };
        classes_of.entry(p_n).or_default().insert(c);
    }
    let quads = all_quads(store, a, i)?;
    let mut derived = Vec::new();
    for (s, p, o) in &quads {
        let Some(classes) = classes_of.get(p) else {
            continue;
        };
        let subject_for_type = if use_subject {
            Some(s.clone())
        } else {
            term_to_subj(o)
        };
        let Some(subj) = subject_for_type else {
            continue;
        };
        for c in classes {
            derived.push(triple(subj.clone(), RDF_TYPE, c.clone()));
        }
    }
    Ok(derived)
}

/// `prp-trp`: `?p rdf:type owl:TransitiveProperty . ?x ?p ?y . ?y ?p ?z` →
/// `?x ?p ?z`
fn apply_prp_trp(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let transitive_props = instances_of(store, OWL_TRANSITIVE_PROPERTY, a, i)?;
    let mut derived = Vec::new();
    for p_subj in transitive_props {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        // Collect all (?x ?p ?y) for this property; build adjacency.
        let mut pairs: Vec<(Subject, Term)> = Vec::new();
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                pairs.push((q.subject, q.object));
            }
        }
        let mut adjacency: HashMap<Subject, HashSet<Term>> = HashMap::new();
        for (s, o) in &pairs {
            adjacency.entry(s.clone()).or_default().insert(o.clone());
        }
        for (x, y) in &pairs {
            let Some(y_subj) = term_to_subj(y) else {
                continue;
            };
            let Some(z_set) = adjacency.get(&y_subj) else {
                continue;
            };
            for z in z_set {
                derived.push(Triple::new(x.clone(), p.clone(), z.clone()));
            }
        }
    }
    Ok(derived)
}

/// `prp-symp`: `?p rdf:type owl:SymmetricProperty . ?x ?p ?y` →
/// `?y ?p ?x`
fn apply_prp_symp(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let symmetric_props = instances_of(store, OWL_SYMMETRIC_PROPERTY, a, i)?;
    let mut derived = Vec::new();
    for p_subj in symmetric_props {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                let Some(y_subj) = term_to_subj(&q.object) else {
                    continue;
                };
                derived.push(Triple::new(y_subj, p.clone(), subj_to_term(&q.subject)));
            }
        }
    }
    Ok(derived)
}

/// `prp-inv1`: `?p1 owl:inverseOf ?p2 . ?x ?p1 ?y` → `?y ?p2 ?x`
fn apply_prp_inv1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    inverse_of(store, /* swap_direction */ false, a, i)
}

/// `prp-inv2`: `?p1 owl:inverseOf ?p2 . ?x ?p2 ?y` → `?y ?p1 ?x`
fn apply_prp_inv2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    inverse_of(store, /* swap_direction */ true, a, i)
}

fn inverse_of(
    store: &Store,
    swap_direction: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let inv_pairs = pairs_for_predicate(store, OWL_INVERSE_OF, a, i)?;
    let mut derived = Vec::new();
    for (p1_subj, p2_term) in inv_pairs {
        let Subject::NamedNode(p1) = p1_subj else {
            continue;
        };
        let Some(p2) = term_to_named(&p2_term) else {
            continue;
        };
        // prp-inv1: search by p1, emit with p2; prp-inv2 swaps the direction.
        let (search_pred, emit_pred) = if swap_direction {
            (p2.clone(), p1.clone())
        } else {
            (p1.clone(), p2.clone())
        };
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in
                store.quads_for_pattern(None, Some(search_pred.as_ref()), None, Some(g_ref))
            {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                let Some(y_subj) = term_to_subj(&q.object) else {
                    continue;
                };
                derived.push(Triple::new(
                    y_subj,
                    emit_pred.clone(),
                    subj_to_term(&q.subject),
                ));
            }
        }
    }
    Ok(derived)
}

/// `prp-fp`: `?p rdf:type owl:FunctionalProperty . ?x ?p ?y1 . ?x ?p ?y2` →
/// `?y1 owl:sameAs ?y2`
fn apply_prp_fp(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let functional_props = instances_of(store, OWL_FUNCTIONAL_PROPERTY, a, i)?;
    let mut derived = Vec::new();
    for p_subj in functional_props {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        // Build subject → set-of-objects for this property.
        let mut objects_of: HashMap<Subject, HashSet<Term>> = HashMap::new();
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                objects_of.entry(q.subject).or_default().insert(q.object);
            }
        }
        for (_, objects) in objects_of {
            // For every ordered pair of distinct objects, emit sameAs.
            let obj_vec: Vec<_> = objects.into_iter().collect();
            for (idx_a, y1) in obj_vec.iter().enumerate() {
                for y2 in obj_vec.iter().skip(idx_a + 1) {
                    let Some(y1_subj) = term_to_subj(y1) else {
                        continue;
                    };
                    let Some(y2_subj) = term_to_subj(y2) else {
                        continue;
                    };
                    derived.push(triple(y1_subj.clone(), OWL_SAME_AS, subj_to_term(&y2_subj)));
                    derived.push(triple(y2_subj, OWL_SAME_AS, subj_to_term(&y1_subj)));
                }
            }
        }
    }
    Ok(derived)
}

/// `eq-sym`: `?x owl:sameAs ?y` → `?y owl:sameAs ?x`
fn apply_eq_sym(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut derived = Vec::new();
    for (x, y) in pairs {
        let Some(y_subj) = term_to_subj(&y) else {
            continue;
        };
        derived.push(triple(y_subj, OWL_SAME_AS, subj_to_term(&x)));
    }
    Ok(derived)
}

// Silence dead-code warnings on helpers whose only consumer is the Phase C
// fixpoint loop (still landing in `super`).
#[allow(dead_code)]
fn _unused_subject_ref_marker(_: SubjectRef<'_>, _: BlankNodeRef<'_>) {}

#[cfg(test)]
mod tests {
    //! Sanity smoke tests against fresh per-test stores. The exhaustive
    //! per-rule coverage lives in `tests/integration_test.rs` under the
    //! 0.9.0 banner (Phase E); these probes pin only the most-likely-to-
    //! break shapes.
    use super::*;
    use oxigraph::model::{NamedNode, Quad};
    use oxigraph::store::Store;

    fn iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    fn insert(store: &Store, s: &str, p: NamedNodeRef<'_>, o: &str) {
        store
            .insert(&Quad::new(
                iri(s),
                p.into_owned(),
                iri(o),
                GraphName::DefaultGraph,
            ))
            .unwrap();
    }

    #[test]
    fn scm_sco_two_step_chain() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", RDFS_SUB_CLASS_OF, "http://e/B");
        insert(&store, "http://e/B", RDFS_SUB_CLASS_OF, "http://e/C");
        let derived =
            apply_scm_sco(&store, &GraphName::DefaultGraph, &GraphName::DefaultGraph).unwrap();
        let a_sub_c = Triple::new(
            iri("http://e/A"),
            RDFS_SUB_CLASS_OF.into_owned(),
            iri("http://e/C"),
        );
        assert!(derived.contains(&a_sub_c), "expected A ⊑ C; got {derived:?}");
    }

    #[test]
    fn cax_sco_propagates_type() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", RDFS_SUB_CLASS_OF, "http://e/B");
        insert(&store, "http://e/alice", RDF_TYPE, "http://e/A");
        let derived =
            apply_cax_sco(&store, &GraphName::DefaultGraph, &GraphName::DefaultGraph).unwrap();
        let alice_b = Triple::new(
            iri("http://e/alice"),
            RDF_TYPE.into_owned(),
            iri("http://e/B"),
        );
        assert!(derived.contains(&alice_b), "expected alice :type B; got {derived:?}");
    }

    #[test]
    fn eq_sym_swaps_subject_object() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/x", OWL_SAME_AS, "http://e/y");
        let derived =
            apply_eq_sym(&store, &GraphName::DefaultGraph, &GraphName::DefaultGraph).unwrap();
        let y_x = Triple::new(iri("http://e/y"), OWL_SAME_AS.into_owned(), iri("http://e/x"));
        assert!(derived.contains(&y_x));
    }
}
