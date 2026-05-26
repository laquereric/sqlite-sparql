//! OWL 2 RL/RDF inconsistency rules (since 0.13.0).
//!
//! The 17 W3C OWL 2 RL inconsistency rules that PLAN_0.10.0 deferred —
//! Prp / Cls / Cax / Eq / Dt families that conclude "false" rather than
//! deriving new quads. Each rule's `detect_*` function scans the asserted
//! + inferred graphs for witness configurations and returns one
//! `ViolationRecord` per witness. The dispatch loop in
//! `super::super::rdf_owl_rl_consistent` collects and sorts the records,
//! then serialises the lot to a JSON array.
//!
//! Naming mirrors the W3C rule table (`cax-dw` → `detect_cax_dw`).
//! Sorting is per-rule and deterministic across runs — `HashMap`
//! iteration order is non-deterministic, so the final result vector
//! is sorted by `(rule, s, p, o)` before being returned.

use oxigraph::model::*;
use oxigraph::store::Store;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::rdf_lists::walk_list;
use super::rules::{
    all_quads, collect_cardinality_restrictions, graph_to_ref, graphs_to_query,
    instances_of, literal_int_value_eq, pairs_for_predicate, subj_to_term,
    term_to_named, term_to_subj, type_pairs_index_by_class,
    type_pairs_index_by_subject, OWL_MAX_CARDINALITY,
    OWL_MAX_QUALIFIED_CARDINALITY, OWL_NOTHING, OWL_ON_CLASS, OWL_ON_PROPERTY,
    OWL_SAME_AS, OWL_THING, RDF_TYPE,
};
use crate::error::{Result, SparqlError};

// ── IRI constants unique to the inconsistency surface ───────────────────────

const OWL_IRREFLEXIVE_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#IrreflexiveProperty");
const OWL_ASYMMETRIC_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#AsymmetricProperty");
const OWL_PROPERTY_DISJOINT_WITH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#propertyDisjointWith");
const OWL_ALL_DISJOINT_PROPERTIES: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#AllDisjointProperties");
const OWL_SOURCE_INDIVIDUAL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#sourceIndividual");
const OWL_ASSERTION_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#assertionProperty");
const OWL_TARGET_INDIVIDUAL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#targetIndividual");
const OWL_TARGET_VALUE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#targetValue");
const OWL_COMPLEMENT_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#complementOf");
const OWL_DISJOINT_WITH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#disjointWith");
const OWL_ALL_DISJOINT_CLASSES: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#AllDisjointClasses");
const OWL_DIFFERENT_FROM: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#differentFrom");
const OWL_ALL_DIFFERENT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#AllDifferent");
const OWL_MEMBERS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#members");
const OWL_DISTINCT_MEMBERS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#distinctMembers");

// ── Public surface ──────────────────────────────────────────────────────────

/// One inconsistency witness. The `s`/`p`/`o` strings use N-Triples-style
/// serialisation (`<iri>`, `_:b0`, `"lit"^^<dt>`, `<< s p o >>`) so the
/// consumer can round-trip them through the existing `rdf_term_value` /
/// `rdf_triple_subject` helpers without an extra parse pass.
#[derive(Serialize, Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct ViolationRecord {
    pub rule: &'static str,
    pub s: String,
    pub p: String,
    pub o: String,
}

pub struct InconsistencyRule {
    pub iri: &'static str,
    pub detect: fn(&Store, &GraphName, &GraphName) -> Result<Vec<ViolationRecord>>,
}

pub static INCONSISTENCY_RULES: &[InconsistencyRule] = &[
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

// ── Formatting helpers ──────────────────────────────────────────────────────

fn nt_subject(s: &Subject) -> String {
    // Oxigraph's Display impl on Subject matches N-Triples for NamedNode
    // (`<iri>`) and BlankNode (`_:b`); RDF-star triple terms are
    // formatted as `<< s p o >>`. Same shape as our other surfaces.
    s.to_string()
}

fn nt_named(p: NamedNodeRef<'_>) -> String {
    format!("<{}>", p.as_str())
}

fn nt_named_owned(p: &NamedNode) -> String {
    format!("<{}>", p.as_str())
}

fn nt_term(t: &Term) -> String {
    t.to_string()
}

fn record(
    rule: &'static str,
    s: impl Into<String>,
    p: impl Into<String>,
    o: impl Into<String>,
) -> ViolationRecord {
    ViolationRecord {
        rule,
        s: s.into(),
        p: p.into(),
        o: o.into(),
    }
}

fn sort_and_return(mut v: Vec<ViolationRecord>) -> Vec<ViolationRecord> {
    v.sort();
    v.dedup();
    v
}

// ── prp-irp: irreflexive property used reflexively ──────────────────────────

fn detect_prp_irp(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let irreflexive_props = instances_of(store, OWL_IRREFLEXIVE_PROPERTY, a, i)?;
    for p_subj in irreflexive_props {
        let Subject::NamedNode(p) = p_subj else { continue };
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                if subj_to_term(&q.subject) == q.object {
                    out.push(record(
                        "prp-irp",
                        nt_subject(&q.subject),
                        nt_named_owned(&p),
                        nt_term(&q.object),
                    ));
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── prp-asyp: asymmetric property used symmetrically ────────────────────────

fn detect_prp_asyp(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let asymmetric_props = instances_of(store, OWL_ASYMMETRIC_PROPERTY, a, i)?;
    for p_subj in asymmetric_props {
        let Subject::NamedNode(p) = p_subj else { continue };
        // Collect every (x, p, y) quad and check for the reverse.
        let mut pairs: HashSet<(Subject, Term)> = HashSet::new();
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                pairs.insert((q.subject, q.object));
            }
        }
        for (x, y) in &pairs {
            let Some(y_subj) = term_to_subj(y) else { continue };
            let x_term = subj_to_term(x);
            if x_term == *y {
                continue; // reflexive case is prp-irp's domain when applicable
            }
            if pairs.contains(&(y_subj, x_term.clone())) {
                // Deterministic witness selection: emit only when x's
                // N-Triples form is lex-smaller, so the symmetric pair
                // produces exactly one record.
                let x_nt = nt_subject(x);
                let y_nt = nt_term(y);
                if x_nt < y_nt {
                    out.push(record("prp-asyp", x_nt, nt_named_owned(&p), y_nt));
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── prp-pdw: property disjoint with — same (x, y) under both p1 and p2 ─────

fn detect_prp_pdw(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let pairs = pairs_for_predicate(store, OWL_PROPERTY_DISJOINT_WITH, a, i)?;
    let mut out = Vec::new();
    for (p1, p2_term) in pairs {
        let Subject::NamedNode(p1_n) = p1 else { continue };
        let Some(p2_n) = term_to_named(&p2_term) else { continue };
        // Choose the lex-smaller predicate for witness determinism.
        let (lo, hi) = if p1_n.as_str() < p2_n.as_str() {
            (p1_n.clone(), p2_n.clone())
        } else if p1_n.as_str() > p2_n.as_str() {
            (p2_n.clone(), p1_n.clone())
        } else {
            continue; // self-disjoint is vacuous; W3C doesn't define it
        };
        // Index (x, y) under both predicates and intersect.
        let lo_pairs = collect_pairs(store, lo.as_ref(), a, i)?;
        let hi_pairs = collect_pairs(store, hi.as_ref(), a, i)?;
        for (x, y) in &lo_pairs {
            if hi_pairs.contains(&(x.clone(), y.clone())) {
                out.push(record(
                    "prp-pdw",
                    nt_subject(x),
                    nt_named_owned(&lo),
                    nt_term(y),
                ));
            }
        }
    }
    Ok(sort_and_return(out))
}

fn collect_pairs(
    store: &Store,
    p: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<HashSet<(Subject, Term)>> {
    let mut out = HashSet::new();
    for graph in graphs_to_query(a, i) {
        let g_ref = graph_to_ref(graph);
        for q in store.quads_for_pattern(None, Some(p), None, Some(g_ref)) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.insert((q.subject, q.object));
        }
    }
    Ok(out)
}

// ── prp-adp: AllDisjointProperties — n-ary property disjointness ──────────

fn detect_prp_adp(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let containers = instances_of(store, OWL_ALL_DISJOINT_PROPERTIES, a, i)?;
    let graphs = graphs_to_query(a, i);
    let members_pairs = pairs_for_predicate(store, OWL_MEMBERS, a, i)?;
    let members_of: HashMap<Subject, Vec<Term>> = group_by_subject(&members_pairs);

    for z in containers {
        let Some(heads) = members_of.get(&z) else { continue };
        for head_term in heads {
            let Some(head_subj) = term_to_subj(head_term) else { continue };
            let Some(list) = walk_list(store, &head_subj, &graphs)? else { continue };
            // Pairwise: every (pi, pj) for i<j must be disjoint.
            let predicates: Vec<NamedNode> =
                list.iter().filter_map(term_to_named).collect();
            for (idx, p_i) in predicates.iter().enumerate() {
                for p_j in predicates.iter().skip(idx + 1) {
                    let (lo, hi) = if p_i.as_str() < p_j.as_str() {
                        (p_i.clone(), p_j.clone())
                    } else if p_i.as_str() > p_j.as_str() {
                        (p_j.clone(), p_i.clone())
                    } else {
                        continue;
                    };
                    let lo_pairs = collect_pairs(store, lo.as_ref(), a, i)?;
                    let hi_pairs = collect_pairs(store, hi.as_ref(), a, i)?;
                    for (x, y) in &lo_pairs {
                        if hi_pairs.contains(&(x.clone(), y.clone())) {
                            out.push(record(
                                "prp-adp",
                                nt_subject(x),
                                nt_named_owned(&lo),
                                nt_term(y),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

fn group_by_subject(pairs: &HashSet<(Subject, Term)>) -> HashMap<Subject, Vec<Term>> {
    let mut out: HashMap<Subject, Vec<Term>> = HashMap::new();
    for (s, t) in pairs {
        out.entry(s.clone()).or_default().push(t.clone());
    }
    out
}

// ── prp-npa1 / prp-npa2: negative property assertions contradicted ─────────

fn detect_prp_npa1(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_npa(store, a, i, /* literal_target = */ false)
}

fn detect_prp_npa2(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_npa(store, a, i, /* literal_target = */ true)
}

fn detect_npa(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
    literal_target: bool,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let source_pairs = pairs_for_predicate(store, OWL_SOURCE_INDIVIDUAL, a, i)?;
    let property_pairs = pairs_for_predicate(store, OWL_ASSERTION_PROPERTY, a, i)?;
    let target_pred = if literal_target { OWL_TARGET_VALUE } else { OWL_TARGET_INDIVIDUAL };
    let target_pairs = pairs_for_predicate(store, target_pred, a, i)?;

    let property_of: HashMap<Subject, NamedNode> = property_pairs
        .into_iter()
        .filter_map(|(b, p)| term_to_named(&p).map(|n| (b, n)))
        .collect();
    let target_of: HashMap<Subject, Term> = target_pairs.into_iter().collect();

    let rule_name = if literal_target { "prp-npa2" } else { "prp-npa1" };

    for (npa_node, source_term) in source_pairs {
        let Some(source_subj) = term_to_subj(&source_term) else { continue };
        let Some(p) = property_of.get(&npa_node) else { continue };
        let Some(target) = target_of.get(&npa_node) else { continue };

        // Validate the target term type matches the variant.
        let target_is_literal = matches!(target, Term::Literal(_));
        if target_is_literal != literal_target {
            continue;
        }

        // Does the asserted graph (or inferred) actually contain
        // (source, p, target)? If so, the NPA is contradicted.
        let mut found = false;
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            let mut iter = store.quads_for_pattern(
                Some(subject_ref(&source_subj)),
                Some(p.as_ref()),
                Some(term_ref(target)),
                Some(g_ref),
            );
            if iter.next().is_some() {
                found = true;
                break;
            }
        }
        if found {
            out.push(record(
                rule_name,
                nt_subject(&source_subj),
                nt_named_owned(p),
                nt_term(target),
            ));
        }
    }
    Ok(sort_and_return(out))
}

fn subject_ref(s: &Subject) -> SubjectRef<'_> {
    match s {
        Subject::NamedNode(n) => SubjectRef::NamedNode(n.as_ref()),
        Subject::BlankNode(b) => SubjectRef::BlankNode(b.as_ref()),
        Subject::Triple(t) => SubjectRef::Triple(t),
    }
}

fn term_ref(t: &Term) -> TermRef<'_> {
    match t {
        Term::NamedNode(n) => TermRef::NamedNode(n.as_ref()),
        Term::BlankNode(b) => TermRef::BlankNode(b.as_ref()),
        Term::Literal(l) => TermRef::Literal(l.as_ref()),
        Term::Triple(t) => TermRef::Triple(t),
    }
}

// ── cls-nothing2: anything typed as owl:Nothing ────────────────────────────

fn detect_cls_nothing2(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let nothing_term = TermRef::NamedNode(OWL_NOTHING);
    for graph in graphs_to_query(a, i) {
        let g_ref = graph_to_ref(graph);
        for q in store.quads_for_pattern(None, Some(RDF_TYPE), Some(nothing_term), Some(g_ref))
        {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.push(record(
                "cls-nothing2",
                nt_subject(&q.subject),
                nt_named(RDF_TYPE),
                nt_named(OWL_NOTHING),
            ));
        }
    }
    Ok(sort_and_return(out))
}

// ── cls-com: x typed as both a class and its complement ────────────────────

fn detect_cls_com(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let complement_pairs = pairs_for_predicate(store, OWL_COMPLEMENT_OF, a, i)?;
    let types_of = type_pairs_index_by_subject(store, a, i)?;
    let mut out = Vec::new();
    for (c1, c2_term) in complement_pairs {
        let c1_term = subj_to_term(&c1);
        // Symmetric: (c1, complementOf, c2) and (c2, complementOf, c1) are
        // both legitimate. Pick a canonical ordering for the witness.
        let (lo, hi) = if nt_term(&c1_term) < nt_term(&c2_term) {
            (c1_term.clone(), c2_term.clone())
        } else if nt_term(&c1_term) > nt_term(&c2_term) {
            (c2_term.clone(), c1_term.clone())
        } else {
            continue;
        };
        // Find every x typed as both lo and hi.
        for (subj, types) in &types_of {
            if types.contains(&lo) && types.contains(&hi) {
                out.push(record(
                    "cls-com",
                    nt_subject(subj),
                    nt_named(RDF_TYPE),
                    nt_term(&lo),
                ));
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── cls-maxc1: maxCardinality 0 with an existing instance ──────────────────

fn detect_cls_maxc1(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let restrictions = collect_cardinality_restrictions(store, OWL_MAX_CARDINALITY, 0, a, i)?;
    let instances_idx = type_pairs_index_by_class(store, a, i)?;
    let mut out = Vec::new();
    for (c, p) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(insts) = instances_idx.get(&c_term) else { continue };
        let p_pairs = collect_pairs(store, p.as_ref(), a, i)?;
        let mut by_subj: HashMap<Subject, Vec<Term>> = HashMap::new();
        for (s, o) in p_pairs {
            by_subj.entry(s).or_default().push(o);
        }
        for x in insts {
            if let Some(objs) = by_subj.get(x) {
                // Lex-smallest object for deterministic witness.
                if let Some(o) = objs.iter().min_by(|a, b| nt_term(a).cmp(&nt_term(b))) {
                    out.push(record(
                        "cls-maxc1",
                        nt_subject(x),
                        nt_named_owned(&p),
                        nt_term(o),
                    ));
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── cls-maxqc1: maxQualifiedCardinality 0 with onClass filter ──────────────

fn detect_cls_maxqc1(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_maxqc_zero(store, a, i, /* thing_only = */ false)
}

// ── cls-maxqc2: maxQualifiedCardinality 0 with onClass owl:Thing ───────────

fn detect_cls_maxqc2(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_maxqc_zero(store, a, i, /* thing_only = */ true)
}

fn detect_maxqc_zero(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
    thing_only: bool,
) -> Result<Vec<ViolationRecord>> {
    let on_property = pairs_for_predicate(store, OWL_ON_PROPERTY, a, i)?;
    let on_class = pairs_for_predicate(store, OWL_ON_CLASS, a, i)?;
    let max_qc = pairs_for_predicate(store, OWL_MAX_QUALIFIED_CARDINALITY, a, i)?;
    let thing_term = Term::NamedNode(OWL_THING.into_owned());

    let on_of: HashMap<Subject, NamedNode> = on_property
        .into_iter()
        .filter_map(|(c, t)| term_to_named(&t).map(|p| (c, p)))
        .collect();
    let on_class_of: HashMap<Subject, Term> = on_class.into_iter().collect();

    let mut restrictions: Vec<(Subject, NamedNode, Term)> = Vec::new();
    for (c, v) in max_qc {
        if !literal_int_value_eq(&v, 0) { continue }
        let Some(p) = on_of.get(&c) else { continue };
        let Some(cls) = on_class_of.get(&c) else { continue };
        let is_thing = cls == &thing_term;
        if thing_only != is_thing { continue }
        restrictions.push((c, p.clone(), cls.clone()));
    }

    let instances_idx = type_pairs_index_by_class(store, a, i)?;
    let types_of = type_pairs_index_by_subject(store, a, i)?;
    let rule_name = if thing_only { "cls-maxqc2" } else { "cls-maxqc1" };
    let mut out = Vec::new();

    for (c, p, cls) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(insts) = instances_idx.get(&c_term) else { continue };
        let p_pairs = collect_pairs(store, p.as_ref(), a, i)?;
        let mut by_subj: HashMap<Subject, Vec<Term>> = HashMap::new();
        for (s, o) in p_pairs {
            by_subj.entry(s).or_default().push(o);
        }
        for x in insts {
            let Some(objs) = by_subj.get(x) else { continue };
            // For maxqc1 (non-Thing), only count objects typed as cls.
            let qualified: Vec<&Term> = if thing_only {
                objs.iter().collect()
            } else {
                objs.iter()
                    .filter(|y| {
                        let Some(y_subj) = term_to_subj(y) else { return false };
                        types_of
                            .get(&y_subj)
                            .map(|set| set.contains(&cls))
                            .unwrap_or(false)
                    })
                    .collect()
            };
            if let Some(o) = qualified.iter().min_by(|a, b| nt_term(a).cmp(&nt_term(b))) {
                out.push(record(
                    rule_name,
                    nt_subject(x),
                    nt_named_owned(&p),
                    nt_term(o),
                ));
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── cax-dw: pairwise class disjointness ────────────────────────────────────

fn detect_cax_dw(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let disjoint_pairs = pairs_for_predicate(store, OWL_DISJOINT_WITH, a, i)?;
    let types_of = type_pairs_index_by_subject(store, a, i)?;
    let mut out = Vec::new();
    for (c1, c2_term) in disjoint_pairs {
        let c1_term = subj_to_term(&c1);
        // Canonical ordering — the disjointness is symmetric, emit once.
        let (lo, hi) = if nt_term(&c1_term) < nt_term(&c2_term) {
            (c1_term, c2_term)
        } else if nt_term(&c1_term) > nt_term(&c2_term) {
            (c2_term, c1_term)
        } else {
            continue;
        };
        for (subj, types) in &types_of {
            if types.contains(&lo) && types.contains(&hi) {
                out.push(record(
                    "cax-dw",
                    nt_subject(subj),
                    nt_named(RDF_TYPE),
                    nt_term(&lo),
                ));
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── cax-adc: AllDisjointClasses (n-ary class disjointness) ─────────────────

fn detect_cax_adc(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let containers = instances_of(store, OWL_ALL_DISJOINT_CLASSES, a, i)?;
    let graphs = graphs_to_query(a, i);
    let members_pairs = pairs_for_predicate(store, OWL_MEMBERS, a, i)?;
    let distinct_pairs = pairs_for_predicate(store, OWL_DISTINCT_MEMBERS, a, i)?;
    let mut heads_of: HashMap<Subject, Vec<Term>> = group_by_subject(&members_pairs);
    // W3C accepts both owl:members and owl:distinctMembers on the container.
    for (s, t) in distinct_pairs {
        heads_of.entry(s).or_default().push(t);
    }
    let types_of = type_pairs_index_by_subject(store, a, i)?;

    for z in containers {
        let Some(heads) = heads_of.get(&z) else { continue };
        for head_term in heads {
            let Some(head_subj) = term_to_subj(head_term) else { continue };
            let Some(list) = walk_list(store, &head_subj, &graphs)? else { continue };
            for (idx, c_i) in list.iter().enumerate() {
                for c_j in list.iter().skip(idx + 1) {
                    let (lo, hi) = if nt_term(c_i) < nt_term(c_j) {
                        (c_i.clone(), c_j.clone())
                    } else if nt_term(c_i) > nt_term(c_j) {
                        (c_j.clone(), c_i.clone())
                    } else {
                        continue;
                    };
                    for (subj, types) in &types_of {
                        if types.contains(&lo) && types.contains(&hi) {
                            out.push(record(
                                "cax-adc",
                                nt_subject(subj),
                                nt_named(RDF_TYPE),
                                nt_term(&lo),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── eq-diff1: differentFrom + sameAs on the same pair ──────────────────────

fn detect_eq_diff1(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let diff_pairs = pairs_for_predicate(store, OWL_DIFFERENT_FROM, a, i)?;
    let same_pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut same_set: HashSet<(String, String)> = HashSet::new();
    for (s, t) in &same_pairs {
        same_set.insert((nt_subject(s), nt_term(t)));
    }
    let mut out = Vec::new();
    for (x, y) in diff_pairs {
        let x_nt = nt_subject(&x);
        let y_nt = nt_term(&y);
        // sameAs is symmetric — check both directions.
        let directly = same_set.contains(&(x_nt.clone(), y_nt.clone()));
        let reverse = same_set.contains(&(y_nt.clone(), x_nt.clone()));
        if !directly && !reverse {
            continue;
        }
        let (lo, hi) = if x_nt < y_nt {
            (x_nt, y_nt)
        } else if x_nt > y_nt {
            (y_nt, x_nt)
        } else {
            continue;
        };
        out.push(record("eq-diff1", lo, nt_named(OWL_SAME_AS), hi));
    }
    Ok(sort_and_return(out))
}

// ── eq-diff2 / eq-diff3: AllDifferent + sameAs between two members ─────────

fn detect_eq_diff2(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_all_different(store, a, i, OWL_MEMBERS, "eq-diff2")
}

fn detect_eq_diff3(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    detect_all_different(store, a, i, OWL_DISTINCT_MEMBERS, "eq-diff3")
}

fn detect_all_different(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
    members_pred: NamedNodeRef<'_>,
    rule_name: &'static str,
) -> Result<Vec<ViolationRecord>> {
    let mut out = Vec::new();
    let containers = instances_of(store, OWL_ALL_DIFFERENT, a, i)?;
    let graphs = graphs_to_query(a, i);
    let members_pairs = pairs_for_predicate(store, members_pred, a, i)?;
    let heads_of = group_by_subject(&members_pairs);

    // Build a sameAs index in both directions for O(1) lookup.
    let same_pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut same_set: HashSet<(String, String)> = HashSet::new();
    for (s, t) in &same_pairs {
        let s_nt = nt_subject(s);
        let t_nt = nt_term(t);
        same_set.insert((s_nt.clone(), t_nt.clone()));
        same_set.insert((t_nt, s_nt));
    }

    for z in containers {
        let Some(heads) = heads_of.get(&z) else { continue };
        for head_term in heads {
            let Some(head_subj) = term_to_subj(head_term) else { continue };
            let Some(list) = walk_list(store, &head_subj, &graphs)? else { continue };
            for (idx, x_i) in list.iter().enumerate() {
                for x_j in list.iter().skip(idx + 1) {
                    let xi_nt = nt_term(x_i);
                    let xj_nt = nt_term(x_j);
                    if !same_set.contains(&(xi_nt.clone(), xj_nt.clone())) {
                        continue;
                    }
                    let (lo, hi) = if xi_nt < xj_nt {
                        (xi_nt, xj_nt)
                    } else if xi_nt > xj_nt {
                        (xj_nt, xi_nt)
                    } else {
                        continue;
                    };
                    out.push(record(rule_name, lo, nt_named(OWL_SAME_AS), hi));
                }
            }
        }
    }
    Ok(sort_and_return(out))
}

// ── dt-not-type: literal lexical form fails its datatype validation ────────

fn detect_dt_not_type(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<ViolationRecord>> {
    let quads = all_quads(store, a, i)?;
    let mut out = Vec::new();
    for (s, p, o) in quads {
        let Term::Literal(lit) = &o else { continue };
        if !literal_violates_datatype(lit) { continue }
        out.push(record(
            "dt-not-type",
            nt_subject(&s),
            nt_named_owned(&p),
            nt_term(&o),
        ));
    }
    Ok(sort_and_return(out))
}

/// Conservative XSD validation: only checks the integer family and
/// booleans. Other datatypes (string family, decimal, double, dates,
/// custom IRIs) are not validated — no false positives. Documented as
/// a 0.13.0 limitation in `rdf_owl_rl_consistent`'s docstring.
fn literal_violates_datatype(lit: &Literal) -> bool {
    let dt = lit.datatype();
    let v = lit.value();
    match dt.as_str() {
        "http://www.w3.org/2001/XMLSchema#integer"
        | "http://www.w3.org/2001/XMLSchema#long"
        | "http://www.w3.org/2001/XMLSchema#int"
        | "http://www.w3.org/2001/XMLSchema#short"
        | "http://www.w3.org/2001/XMLSchema#byte" => v.parse::<i64>().is_err(),
        "http://www.w3.org/2001/XMLSchema#nonNegativeInteger"
        | "http://www.w3.org/2001/XMLSchema#unsignedLong"
        | "http://www.w3.org/2001/XMLSchema#unsignedInt"
        | "http://www.w3.org/2001/XMLSchema#unsignedShort"
        | "http://www.w3.org/2001/XMLSchema#unsignedByte" => v.parse::<u64>().is_err(),
        "http://www.w3.org/2001/XMLSchema#positiveInteger" => {
            v.parse::<u64>().map(|n| n == 0).unwrap_or(true)
        }
        "http://www.w3.org/2001/XMLSchema#nonPositiveInteger" => {
            v.parse::<i64>().map(|n| n > 0).unwrap_or(true)
        }
        "http://www.w3.org/2001/XMLSchema#negativeInteger" => {
            v.parse::<i64>().map(|n| n >= 0).unwrap_or(true)
        }
        "http://www.w3.org/2001/XMLSchema#boolean" => {
            !matches!(v, "true" | "false" | "0" | "1")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{NamedNode, Quad};

    fn iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    fn insert_q(store: &Store, s: &str, p: NamedNodeRef<'_>, o: &str) {
        store
            .insert(&Quad::new(
                iri(s),
                p.into_owned(),
                iri(o),
                GraphName::DefaultGraph,
            ))
            .unwrap();
    }

    fn insert_typed(store: &Store, s: &str, t: &str) {
        insert_q(store, s, RDF_TYPE, t);
    }

    fn graphs() -> (GraphName, GraphName) {
        (
            GraphName::DefaultGraph,
            GraphName::NamedNode(iri("urn:g:inferred")),
        )
    }

    #[test]
    fn cax_dw_smoke() {
        let store = Store::new().unwrap();
        insert_q(&store, "http://e/Animal", OWL_DISJOINT_WITH, "http://e/Plant");
        insert_typed(&store, "http://e/alice", "http://e/Animal");
        insert_typed(&store, "http://e/alice", "http://e/Plant");
        let (a, i) = graphs();
        let v = detect_cax_dw(&store, &a, &i).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "cax-dw");
    }

    #[test]
    fn cls_nothing2_smoke() {
        let store = Store::new().unwrap();
        insert_typed(&store, "http://e/x", OWL_NOTHING.as_str());
        let (a, i) = graphs();
        let v = detect_cls_nothing2(&store, &a, &i).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "cls-nothing2");
    }

    #[test]
    fn prp_irp_smoke() {
        let store = Store::new().unwrap();
        insert_q(
            &store,
            "http://e/parentOf",
            RDF_TYPE,
            OWL_IRREFLEXIVE_PROPERTY.as_str(),
        );
        insert_q(&store, "http://e/alice", iri("http://e/parentOf").as_ref(), "http://e/alice");
        let (a, i) = graphs();
        let v = detect_prp_irp(&store, &a, &i).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "prp-irp");
    }

    #[test]
    fn dt_not_type_smoke() {
        let store = Store::new().unwrap();
        store
            .insert(&Quad::new(
                iri("http://e/alice"),
                iri("http://e/age"),
                Literal::new_typed_literal(
                    "thirty",
                    iri("http://www.w3.org/2001/XMLSchema#integer"),
                ),
                GraphName::DefaultGraph,
            ))
            .unwrap();
        let (a, i) = graphs();
        let v = detect_dt_not_type(&store, &a, &i).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "dt-not-type");
    }

    #[test]
    fn consistent_store_returns_empty() {
        let store = Store::new().unwrap();
        insert_typed(&store, "http://e/alice", "http://e/Person");
        let (a, i) = graphs();
        for rule in INCONSISTENCY_RULES {
            let v = (rule.detect)(&store, &a, &i).unwrap();
            assert!(v.is_empty(), "rule {} flagged a consistent graph: {:?}", rule.iri, v);
        }
    }
}
