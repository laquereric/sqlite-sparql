//! OWL 2 RL/RDF rule library.
//!
//! Each rule queries the union of `asserted_graph` and `inferred_graph`
//! for its premises and returns `Vec<Triple>` of newly-derivable triples.
//! Dedup against existing inferred-graph contents happens in the fixpoint
//! loop (`super::execute_materialise`), not here — rules can freely
//! re-emit triples; the loop's `Store::contains` check filters.
//!
//! Coverage:
//!
//! - 0.9.0 — 15 rules matching VG's `Reasoner::Rules::OwlRl` exactly
//!   (scm-sco/spo/eqc1/eqp1, cax-sco, prp-spo1/dom/rng/trp/symp/inv1/inv2/fp,
//!   eq-sym/trans).
//! - 0.10.0 Phase B — 16 scm-* T-Box rules
//!   (scm-cls/op/dp/eqc2/eqp2/dom1/dom2/rng1/rng2/hv/svf1/svf2/avf1/avf2/int/uni).
//! - 0.10.0 Phase C — 16 class-expression A-Box rules
//!   (cls-thing/nothing1/int1/int2/uni/svf1/svf2/avf/hv1/hv2/maxc2/maxqc3/maxqc4/oo,
//!   cax-eqc1/eqc2).
//! - 0.10.0 Phase D — 9 property + equality rules
//!   (prp-ifp/spo2/eqp1/eqp2/key, eq-ref, eq-rep-s/p/o). The eq-rep-* trio
//!   is gated by `MaterialiseOptions::equality_saturation` (default `true`);
//!   `eq-ref` is gated by `eq_reflexive` (default `false`).
//! - 0.10.0 Phase E — 4 datatype rules (dt-type1/type2/eq/diff). The two
//!   "rdf:type rdfs:Datatype" axioms fire fully; `dt-eq` / `dt-diff` are
//!   functional no-ops in Oxigraph 0.4 (literals are not representable as
//!   subjects of derived quads — see each rule's docstring).
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
const OWL_CLASS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#Class");
const OWL_OBJECT_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#ObjectProperty");
const OWL_DATATYPE_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#DatatypeProperty");
const OWL_THING: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#Thing");
const OWL_NOTHING: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#Nothing");
const OWL_HAS_VALUE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#hasValue");
const OWL_ON_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#onProperty");
const OWL_SOME_VALUES_FROM: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#someValuesFrom");
const OWL_ALL_VALUES_FROM: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#allValuesFrom");
const OWL_INTERSECTION_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#intersectionOf");
const OWL_UNION_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#unionOf");
const OWL_ONE_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#oneOf");
const OWL_MAX_CARDINALITY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#maxCardinality");
const OWL_MAX_QUALIFIED_CARDINALITY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#maxQualifiedCardinality");
const OWL_ON_CLASS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#onClass");
const OWL_INVERSE_FUNCTIONAL_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#InverseFunctionalProperty");
const OWL_PROPERTY_CHAIN_AXIOM: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#propertyChainAxiom");
const OWL_HAS_KEY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2002/07/owl#hasKey");
const RDFS_DATATYPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#Datatype");

/// W3C OWL 2 RL/RDF Table 9 — XSD + RDF datatypes that ship as
/// `?dt rdf:type rdfs:Datatype` axioms under `dt-type1`. The list is
/// closed; consumer-defined datatypes pick up the `rdf:type rdfs:Datatype`
/// fact through `dt-type2` instead (which scans literals in the store).
const DT_TYPE1_DATATYPES: &[&str] = &[
    "http://www.w3.org/2001/XMLSchema#decimal",
    "http://www.w3.org/2001/XMLSchema#integer",
    "http://www.w3.org/2001/XMLSchema#nonNegativeInteger",
    "http://www.w3.org/2001/XMLSchema#positiveInteger",
    "http://www.w3.org/2001/XMLSchema#long",
    "http://www.w3.org/2001/XMLSchema#int",
    "http://www.w3.org/2001/XMLSchema#short",
    "http://www.w3.org/2001/XMLSchema#byte",
    "http://www.w3.org/2001/XMLSchema#nonPositiveInteger",
    "http://www.w3.org/2001/XMLSchema#negativeInteger",
    "http://www.w3.org/2001/XMLSchema#unsignedLong",
    "http://www.w3.org/2001/XMLSchema#unsignedInt",
    "http://www.w3.org/2001/XMLSchema#unsignedShort",
    "http://www.w3.org/2001/XMLSchema#unsignedByte",
    "http://www.w3.org/2001/XMLSchema#double",
    "http://www.w3.org/2001/XMLSchema#float",
    "http://www.w3.org/2001/XMLSchema#string",
    "http://www.w3.org/2001/XMLSchema#normalizedString",
    "http://www.w3.org/2001/XMLSchema#token",
    "http://www.w3.org/2001/XMLSchema#language",
    "http://www.w3.org/2001/XMLSchema#Name",
    "http://www.w3.org/2001/XMLSchema#NCName",
    "http://www.w3.org/2001/XMLSchema#NMTOKEN",
    "http://www.w3.org/2001/XMLSchema#boolean",
    "http://www.w3.org/2001/XMLSchema#hexBinary",
    "http://www.w3.org/2001/XMLSchema#base64Binary",
    "http://www.w3.org/2001/XMLSchema#anyURI",
    "http://www.w3.org/2001/XMLSchema#dateTime",
    "http://www.w3.org/2001/XMLSchema#dateTimeStamp",
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#XMLLiteral",
    "http://www.w3.org/1999/02/22-rdf-syntax-ns#PlainLiteral",
];

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
    // ── 0.10.0 Phase B — scm-* T-Box rules ───────────────────────────────────
    Rule { iri: "scm-cls",   apply: apply_scm_cls   },
    Rule { iri: "scm-op",    apply: apply_scm_op    },
    Rule { iri: "scm-dp",    apply: apply_scm_dp    },
    Rule { iri: "scm-eqc2",  apply: apply_scm_eqc2  },
    Rule { iri: "scm-eqp2",  apply: apply_scm_eqp2  },
    Rule { iri: "scm-dom1",  apply: apply_scm_dom1  },
    Rule { iri: "scm-dom2",  apply: apply_scm_dom2  },
    Rule { iri: "scm-rng1",  apply: apply_scm_rng1  },
    Rule { iri: "scm-rng2",  apply: apply_scm_rng2  },
    Rule { iri: "scm-hv",    apply: apply_scm_hv    },
    Rule { iri: "scm-svf1",  apply: apply_scm_svf1  },
    Rule { iri: "scm-svf2",  apply: apply_scm_svf2  },
    Rule { iri: "scm-avf1",  apply: apply_scm_avf1  },
    Rule { iri: "scm-avf2",  apply: apply_scm_avf2  },
    Rule { iri: "scm-int",   apply: apply_scm_int   },
    Rule { iri: "scm-uni",   apply: apply_scm_uni   },
    // ── 0.10.0 Phase C — class-expression A-Box rules ────────────────────────
    Rule { iri: "cls-thing",    apply: apply_cls_thing    },
    Rule { iri: "cls-nothing1", apply: apply_cls_nothing1 },
    Rule { iri: "cls-int1",     apply: apply_cls_int1     },
    Rule { iri: "cls-int2",     apply: apply_cls_int2     },
    Rule { iri: "cls-uni",      apply: apply_cls_uni      },
    Rule { iri: "cls-svf1",     apply: apply_cls_svf1     },
    Rule { iri: "cls-svf2",     apply: apply_cls_svf2     },
    Rule { iri: "cls-avf",      apply: apply_cls_avf      },
    Rule { iri: "cls-hv1",      apply: apply_cls_hv1      },
    Rule { iri: "cls-hv2",      apply: apply_cls_hv2      },
    Rule { iri: "cls-maxc2",    apply: apply_cls_maxc2    },
    Rule { iri: "cls-maxqc3",   apply: apply_cls_maxqc3   },
    Rule { iri: "cls-maxqc4",   apply: apply_cls_maxqc4   },
    Rule { iri: "cls-oo",       apply: apply_cls_oo       },
    Rule { iri: "cax-eqc1",     apply: apply_cax_eqc1     },
    Rule { iri: "cax-eqc2",     apply: apply_cax_eqc2     },
    // ── 0.10.0 Phase D — remaining property + equality rules ────────────────
    Rule { iri: "prp-ifp",   apply: apply_prp_ifp   },
    Rule { iri: "prp-spo2",  apply: apply_prp_spo2  },
    Rule { iri: "prp-eqp1",  apply: apply_prp_eqp1  },
    Rule { iri: "prp-eqp2",  apply: apply_prp_eqp2  },
    Rule { iri: "prp-key",   apply: apply_prp_key   },
    Rule { iri: "eq-ref",    apply: apply_eq_ref    },
    Rule { iri: "eq-rep-s",  apply: apply_eq_rep_s  },
    Rule { iri: "eq-rep-p",  apply: apply_eq_rep_p  },
    Rule { iri: "eq-rep-o",  apply: apply_eq_rep_o  },
    // ── 0.10.0 Phase E — datatype rules ──────────────────────────────────────
    Rule { iri: "dt-type1",  apply: apply_dt_type1  },
    Rule { iri: "dt-type2",  apply: apply_dt_type2  },
    Rule { iri: "dt-eq",     apply: apply_dt_eq     },
    Rule { iri: "dt-diff",   apply: apply_dt_diff   },
];

/// Rule IRIs of the eq-rep-* family. Skipped by the fixpoint loop when
/// `MaterialiseOptions::equality_saturation` is `false`.
pub(crate) const EQ_REP_RULE_IRIS: &[&str] = &["eq-rep-s", "eq-rep-p", "eq-rep-o"];

/// Rule IRI for `eq-ref`. Skipped by the fixpoint loop when
/// `MaterialiseOptions::eq_reflexive` is `false` (the default — see the
/// option's docstring for the non-convergence rationale under
/// `provenance: true`).
pub(crate) const EQ_REF_RULE_IRI: &str = "eq-ref";

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

// ── 0.10.0 Phase B — scm-* T-Box rules ──────────────────────────────────────

/// `scm-cls`: `?c rdf:type owl:Class` →
/// `?c rdfs:subClassOf ?c`, `?c owl:equivalentClass ?c`,
/// `?c rdfs:subClassOf owl:Thing`, `owl:Nothing rdfs:subClassOf ?c`.
fn apply_scm_cls(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let classes = instances_of(store, OWL_CLASS, a, i)?;
    let thing = Term::NamedNode(OWL_THING.into_owned());
    let nothing = Subject::NamedNode(OWL_NOTHING.into_owned());
    let mut derived = Vec::with_capacity(classes.len() * 4);
    for c_subj in classes {
        let c_term = subj_to_term(&c_subj);
        derived.push(triple(c_subj.clone(), RDFS_SUB_CLASS_OF, c_term.clone()));
        derived.push(triple(c_subj.clone(), OWL_EQUIVALENT_CLASS, c_term.clone()));
        derived.push(triple(c_subj, RDFS_SUB_CLASS_OF, thing.clone()));
        derived.push(triple(nothing.clone(), RDFS_SUB_CLASS_OF, c_term));
    }
    Ok(derived)
}

/// `scm-op`: `?p rdf:type owl:ObjectProperty` →
/// `?p rdfs:subPropertyOf ?p`, `?p owl:equivalentProperty ?p`.
fn apply_scm_op(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    reflexive_property_axioms(store, OWL_OBJECT_PROPERTY, a, i)
}

/// `scm-dp`: `?p rdf:type owl:DatatypeProperty` →
/// `?p rdfs:subPropertyOf ?p`, `?p owl:equivalentProperty ?p`.
fn apply_scm_dp(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    reflexive_property_axioms(store, OWL_DATATYPE_PROPERTY, a, i)
}

fn reflexive_property_axioms(
    store: &Store,
    property_class: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let props = instances_of(store, property_class, a, i)?;
    let mut derived = Vec::with_capacity(props.len() * 2);
    for p_subj in props {
        let p_term = subj_to_term(&p_subj);
        derived.push(triple(p_subj.clone(), RDFS_SUB_PROPERTY_OF, p_term.clone()));
        derived.push(triple(p_subj, OWL_EQUIVALENT_PROPERTY, p_term));
    }
    Ok(derived)
}

/// `scm-eqc2`: `?c1 rdfs:subClassOf ?c2 . ?c2 rdfs:subClassOf ?c1` →
/// `?c1 owl:equivalentClass ?c2`.
fn apply_scm_eqc2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    bidirectional_subsumption_to_equivalent(store, RDFS_SUB_CLASS_OF, OWL_EQUIVALENT_CLASS, a, i)
}

/// `scm-eqp2`: bidirectional `rdfs:subPropertyOf` → `owl:equivalentProperty`.
fn apply_scm_eqp2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    bidirectional_subsumption_to_equivalent(
        store,
        RDFS_SUB_PROPERTY_OF,
        OWL_EQUIVALENT_PROPERTY,
        a,
        i,
    )
}

fn bidirectional_subsumption_to_equivalent(
    store: &Store,
    sub_pred: NamedNodeRef<'_>,
    equiv_pred: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let pairs = pairs_for_predicate(store, sub_pred, a, i)?;
    let mut derived = Vec::new();
    for (x, y) in &pairs {
        let Some(y_subj) = term_to_subj(y) else {
            continue;
        };
        if pairs.contains(&(y_subj, subj_to_term(x))) {
            derived.push(triple(x.clone(), equiv_pred, y.clone()));
        }
    }
    Ok(derived)
}

/// `scm-dom1`: `?p rdfs:domain ?c1 . ?c1 rdfs:subClassOf ?c2` → `?p rdfs:domain ?c2`.
fn apply_scm_dom1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range_via_subclass(store, RDFS_DOMAIN, a, i)
}

/// `scm-rng1`: `?p rdfs:range ?c1 . ?c1 rdfs:subClassOf ?c2` → `?p rdfs:range ?c2`.
fn apply_scm_rng1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range_via_subclass(store, RDFS_RANGE, a, i)
}

fn domain_or_range_via_subclass(
    store: &Store,
    dr_pred: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let dr_pairs = pairs_for_predicate(store, dr_pred, a, i)?;
    let sub_class_pairs = pairs_for_predicate(store, RDFS_SUB_CLASS_OF, a, i)?;
    let mut super_of: HashMap<Subject, HashSet<Term>> = HashMap::new();
    for (c1, c2) in &sub_class_pairs {
        super_of.entry(c1.clone()).or_default().insert(c2.clone());
    }
    let mut derived = Vec::new();
    for (p, c1) in dr_pairs {
        let Some(c1_subj) = term_to_subj(&c1) else {
            continue;
        };
        let Some(supers) = super_of.get(&c1_subj) else {
            continue;
        };
        for c2 in supers {
            derived.push(triple(p.clone(), dr_pred, c2.clone()));
        }
    }
    Ok(derived)
}

/// `scm-dom2`: `?p2 rdfs:domain ?c . ?p1 rdfs:subPropertyOf ?p2` → `?p1 rdfs:domain ?c`.
fn apply_scm_dom2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range_via_subproperty(store, RDFS_DOMAIN, a, i)
}

/// `scm-rng2`: `?p2 rdfs:range ?c . ?p1 rdfs:subPropertyOf ?p2` → `?p1 rdfs:range ?c`.
fn apply_scm_rng2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    domain_or_range_via_subproperty(store, RDFS_RANGE, a, i)
}

fn domain_or_range_via_subproperty(
    store: &Store,
    dr_pred: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let dr_pairs = pairs_for_predicate(store, dr_pred, a, i)?;
    let sub_prop_pairs = pairs_for_predicate(store, RDFS_SUB_PROPERTY_OF, a, i)?;
    // p2 (as Subject — matches dr_pairs key shape) → set of sub-properties p1.
    let mut subs_of: HashMap<Subject, HashSet<Subject>> = HashMap::new();
    for (p1, p2) in sub_prop_pairs {
        let Some(p2_subj) = term_to_subj(&p2) else {
            continue;
        };
        subs_of.entry(p2_subj).or_default().insert(p1);
    }
    let mut derived = Vec::new();
    for (p2, c) in dr_pairs {
        let Some(subs) = subs_of.get(&p2) else {
            continue;
        };
        for p1 in subs {
            derived.push(triple(p1.clone(), dr_pred, c.clone()));
        }
    }
    Ok(derived)
}

/// `scm-hv`: `?c1 owl:hasValue ?v ; owl:onProperty ?p1 . ?c2 owl:hasValue ?v ;
///           owl:onProperty ?p2 . ?p1 rdfs:subPropertyOf ?p2` →
/// `?c1 rdfs:subClassOf ?c2`.
fn apply_scm_hv(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_HAS_VALUE, a, i)?;
    let sub_prop_pairs = pairs_for_predicate(store, RDFS_SUB_PROPERTY_OF, a, i)?;
    let mut supers_of_prop: HashMap<NamedNode, HashSet<NamedNode>> = HashMap::new();
    for (p1, p2) in sub_prop_pairs {
        let Subject::NamedNode(p1_n) = p1 else {
            continue;
        };
        let Some(p2_n) = term_to_named(&p2) else {
            continue;
        };
        supers_of_prop.entry(p1_n).or_default().insert(p2_n);
    }
    // Group restrictions by (on_property, has_value).
    let mut by_p_v: HashMap<(NamedNode, Term), Vec<Subject>> = HashMap::new();
    for (c, p, v) in &restrictions {
        by_p_v
            .entry((p.clone(), v.clone()))
            .or_default()
            .push(c.clone());
    }
    let mut derived = Vec::new();
    for (c1, p1, v) in &restrictions {
        let Some(p2_set) = supers_of_prop.get(p1) else {
            continue;
        };
        for p2 in p2_set {
            if let Some(c2_list) = by_p_v.get(&(p2.clone(), v.clone())) {
                for c2 in c2_list {
                    derived.push(triple(c1.clone(), RDFS_SUB_CLASS_OF, subj_to_term(c2)));
                }
            }
        }
    }
    Ok(derived)
}

/// `scm-svf1`: same onProperty between two `owl:someValuesFrom` restrictions,
/// filler classes related by `rdfs:subClassOf` → corresponding subsumption
/// between the restriction classes.
fn apply_scm_svf1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_restriction_value_subclass(store, OWL_SOME_VALUES_FROM, a, i)
}

/// `scm-avf1`: same as `scm-svf1` but for `owl:allValuesFrom`.
fn apply_scm_avf1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_restriction_value_subclass(store, OWL_ALL_VALUES_FROM, a, i)
}

fn scm_restriction_value_subclass(
    store: &Store,
    constraint_pred: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, constraint_pred, a, i)?;
    let sub_class_pairs = pairs_for_predicate(store, RDFS_SUB_CLASS_OF, a, i)?;
    let mut supers_of_class: HashMap<Subject, HashSet<Term>> = HashMap::new();
    for (y1, y2) in &sub_class_pairs {
        supers_of_class
            .entry(y1.clone())
            .or_default()
            .insert(y2.clone());
    }
    let mut by_p: HashMap<NamedNode, Vec<(Subject, Term)>> = HashMap::new();
    for (c, p, v) in restrictions {
        by_p.entry(p).or_default().push((c, v));
    }
    let mut derived = Vec::new();
    for group in by_p.values() {
        for (c1, y1) in group {
            let Some(y1_subj) = term_to_subj(y1) else {
                continue;
            };
            let Some(y2_set) = supers_of_class.get(&y1_subj) else {
                continue;
            };
            for (c2, y2) in group {
                if y2_set.contains(y2) {
                    derived.push(triple(c1.clone(), RDFS_SUB_CLASS_OF, subj_to_term(c2)));
                }
            }
        }
    }
    Ok(derived)
}

/// `scm-svf2`: same filler between two `owl:someValuesFrom` restrictions,
/// sub-property relation between their onProperties → restriction with
/// sub-property is sub-class of restriction with super-property.
fn apply_scm_svf2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_restriction_property_subclass(store, OWL_SOME_VALUES_FROM, /* flip = */ false, a, i)
}

/// `scm-avf2`: same shape as `scm-svf2` but with `owl:allValuesFrom` — and
/// **direction flips**: restriction with super-property is sub-class of the
/// restriction with sub-property.
fn apply_scm_avf2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_restriction_property_subclass(store, OWL_ALL_VALUES_FROM, /* flip = */ true, a, i)
}

fn scm_restriction_property_subclass(
    store: &Store,
    constraint_pred: NamedNodeRef<'_>,
    flip: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, constraint_pred, a, i)?;
    let sub_prop_pairs = pairs_for_predicate(store, RDFS_SUB_PROPERTY_OF, a, i)?;
    let mut supers_of_prop: HashMap<NamedNode, HashSet<NamedNode>> = HashMap::new();
    for (p1, p2) in sub_prop_pairs {
        let Subject::NamedNode(p1_n) = p1 else {
            continue;
        };
        let Some(p2_n) = term_to_named(&p2) else {
            continue;
        };
        supers_of_prop.entry(p1_n).or_default().insert(p2_n);
    }
    let mut by_p_v: HashMap<(NamedNode, Term), Vec<Subject>> = HashMap::new();
    for (c, p, v) in &restrictions {
        by_p_v
            .entry((p.clone(), v.clone()))
            .or_default()
            .push(c.clone());
    }
    let mut derived = Vec::new();
    for (c1, p1, v) in &restrictions {
        let Some(p2_set) = supers_of_prop.get(p1) else {
            continue;
        };
        for p2 in p2_set {
            if let Some(c2_list) = by_p_v.get(&(p2.clone(), v.clone())) {
                for c2 in c2_list {
                    if flip {
                        derived.push(triple(c2.clone(), RDFS_SUB_CLASS_OF, subj_to_term(c1)));
                    } else {
                        derived.push(triple(c1.clone(), RDFS_SUB_CLASS_OF, subj_to_term(c2)));
                    }
                }
            }
        }
    }
    Ok(derived)
}

/// `scm-int`: `?c owl:intersectionOf (?c1 … ?cn)` → `?c rdfs:subClassOf ?ci` for each i.
fn apply_scm_int(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_class_list(store, OWL_INTERSECTION_OF, /* parent_is_super = */ true, a, i)
}

/// `scm-uni`: `?c owl:unionOf (?c1 … ?cn)` → `?ci rdfs:subClassOf ?c` for each i.
fn apply_scm_uni(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    scm_class_list(store, OWL_UNION_OF, /* parent_is_super = */ false, a, i)
}

fn scm_class_list(
    store: &Store,
    list_predicate: NamedNodeRef<'_>,
    parent_is_super: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let pairs = pairs_for_predicate(store, list_predicate, a, i)?;
    let graphs = graphs_to_query(a, i);
    let mut derived = Vec::new();
    for (c, list_head_term) in pairs {
        let Some(head_subj) = term_to_subj(&list_head_term) else {
            continue;
        };
        let Some(members) = super::rdf_lists::walk_list(store, &head_subj, &graphs)? else {
            continue;
        };
        let c_term = subj_to_term(&c);
        for member in members {
            if parent_is_super {
                derived.push(triple(c.clone(), RDFS_SUB_CLASS_OF, member));
            } else {
                let Some(m_subj) = term_to_subj(&member) else {
                    continue;
                };
                derived.push(triple(m_subj, RDFS_SUB_CLASS_OF, c_term.clone()));
            }
        }
    }
    Ok(derived)
}

/// Collect every restriction class `c` in the graph along with its
/// `owl:onProperty` (the property the restriction constrains) and the
/// supplied `constraint_predicate`'s value (`owl:hasValue` /
/// `owl:someValuesFrom` / `owl:allValuesFrom`).
///
/// Returns one entry per `(class, onProperty, constraintValue)` combination;
/// well-formed restriction classes yield one entry, but a graph with
/// multiple `owl:onProperty` values for a single class (malformed OWL but
/// legal RDF) yields one row per onProperty.
fn collect_restrictions(
    store: &Store,
    constraint_predicate: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<(Subject, NamedNode, Term)>> {
    let on_property_pairs = pairs_for_predicate(store, OWL_ON_PROPERTY, a, i)?;
    let mut on_of: HashMap<Subject, HashSet<NamedNode>> = HashMap::new();
    for (c, p_term) in on_property_pairs {
        let Some(p_n) = term_to_named(&p_term) else {
            continue;
        };
        on_of.entry(c).or_default().insert(p_n);
    }
    let constraint_pairs = pairs_for_predicate(store, constraint_predicate, a, i)?;
    let mut out = Vec::new();
    for (c, v) in constraint_pairs {
        if let Some(props) = on_of.get(&c) {
            for p in props {
                out.push((c.clone(), p.clone(), v.clone()));
            }
        }
    }
    Ok(out)
}

// ── 0.10.0 Phase C — class-expression A-Box rules ───────────────────────────

/// `cls-thing`: axiomatic — `owl:Thing rdf:type owl:Class`.
fn apply_cls_thing(_store: &Store, _a: &GraphName, _i: &GraphName) -> Result<Vec<Triple>> {
    Ok(vec![Triple::new(
        Subject::NamedNode(OWL_THING.into_owned()),
        RDF_TYPE.into_owned(),
        Term::NamedNode(OWL_CLASS.into_owned()),
    )])
}

/// `cls-nothing1`: axiomatic — `owl:Nothing rdf:type owl:Class`.
fn apply_cls_nothing1(_store: &Store, _a: &GraphName, _i: &GraphName) -> Result<Vec<Triple>> {
    Ok(vec![Triple::new(
        Subject::NamedNode(OWL_NOTHING.into_owned()),
        RDF_TYPE.into_owned(),
        Term::NamedNode(OWL_CLASS.into_owned()),
    )])
}

/// `cls-int1`: `?c owl:intersectionOf (?c1 … ?cn) . ?x rdf:type ?ci (all i)` →
/// `?x rdf:type ?c`.
fn apply_cls_int1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let intersections = collect_class_lists(store, OWL_INTERSECTION_OF, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, members) in intersections {
        if members.is_empty() {
            continue;
        }
        // x ∈ instances iff x has every ci in members as a type.
        let mut intersection: Option<HashSet<Subject>> = None;
        for m in &members {
            let inst = instances_of.get(m).cloned().unwrap_or_default();
            intersection = Some(match intersection {
                None => inst,
                Some(prev) => prev.intersection(&inst).cloned().collect(),
            });
        }
        let Some(set) = intersection else { continue };
        let c_term = subj_to_term(&c);
        for x in set {
            derived.push(triple(x, RDF_TYPE, c_term.clone()));
        }
    }
    Ok(derived)
}

/// `cls-int2`: `?c owl:intersectionOf (?c1 … ?cn) . ?x rdf:type ?c` →
/// `?x rdf:type ?ci` for each i.
fn apply_cls_int2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let intersections = collect_class_lists(store, OWL_INTERSECTION_OF, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, members) in intersections {
        let c_term = subj_to_term(&c);
        let Some(insts) = instances_of.get(&c_term) else {
            continue;
        };
        for x in insts {
            for m in &members {
                derived.push(triple(x.clone(), RDF_TYPE, m.clone()));
            }
        }
    }
    Ok(derived)
}

/// `cls-uni`: `?c owl:unionOf (?c1 … ?cn) . ?x rdf:type ?ci (some i)` →
/// `?x rdf:type ?c`.
fn apply_cls_uni(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let unions = collect_class_lists(store, OWL_UNION_OF, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, members) in unions {
        let c_term = subj_to_term(&c);
        for m in members {
            if let Some(insts) = instances_of.get(&m) {
                for x in insts {
                    derived.push(triple(x.clone(), RDF_TYPE, c_term.clone()));
                }
            }
        }
    }
    Ok(derived)
}

/// `cls-svf1`: `?c owl:someValuesFrom ?y ; owl:onProperty ?p . ?u ?p ?v .
///             ?v rdf:type ?y` → `?u rdf:type ?c`.
fn apply_cls_svf1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_SOME_VALUES_FROM, a, i)?;
    let type_pairs = pairs_for_predicate(store, RDF_TYPE, a, i)?;
    let mut derived = Vec::new();
    for (c, p, y_term) in restrictions {
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        let c_term = subj_to_term(&c);
        for (u, v_term) in p_pairs {
            let Some(v_subj) = term_to_subj(&v_term) else {
                continue;
            };
            if type_pairs.contains(&(v_subj, y_term.clone())) {
                derived.push(triple(u, RDF_TYPE, c_term.clone()));
            }
        }
    }
    Ok(derived)
}

/// `cls-svf2`: `?c owl:someValuesFrom owl:Thing ; owl:onProperty ?p . ?u ?p ?v` →
/// `?u rdf:type ?c`.
fn apply_cls_svf2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_SOME_VALUES_FROM, a, i)?;
    let thing = Term::NamedNode(OWL_THING.into_owned());
    let mut derived = Vec::new();
    for (c, p, y_term) in restrictions {
        if y_term != thing {
            continue;
        }
        let c_term = subj_to_term(&c);
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        for (u, _v) in p_pairs {
            derived.push(triple(u, RDF_TYPE, c_term.clone()));
        }
    }
    Ok(derived)
}

/// `cls-avf`: `?c owl:allValuesFrom ?y ; owl:onProperty ?p . ?x rdf:type ?c .
///            ?x ?p ?u` → `?u rdf:type ?y`.
fn apply_cls_avf(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_ALL_VALUES_FROM, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, p, y_term) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(c_instances) = instances_of.get(&c_term) else {
            continue;
        };
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        // Subject → set of property objects (for fast lookup per c-instance).
        let mut objs_of: HashMap<Subject, HashSet<Term>> = HashMap::new();
        for (s, o) in p_pairs {
            objs_of.entry(s).or_default().insert(o);
        }
        for x in c_instances {
            if let Some(us) = objs_of.get(x) {
                for u in us {
                    let Some(u_subj) = term_to_subj(u) else {
                        continue;
                    };
                    derived.push(triple(u_subj, RDF_TYPE, y_term.clone()));
                }
            }
        }
    }
    Ok(derived)
}

/// `cls-hv1`: `?c owl:hasValue ?v ; owl:onProperty ?p . ?x rdf:type ?c` →
/// `?x ?p ?v`.
fn apply_cls_hv1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_HAS_VALUE, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, p, v) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(insts) = instances_of.get(&c_term) else {
            continue;
        };
        for x in insts {
            derived.push(Triple::new(x.clone(), p.clone(), v.clone()));
        }
    }
    Ok(derived)
}

/// `cls-hv2`: `?c owl:hasValue ?v ; owl:onProperty ?p . ?x ?p ?v` →
/// `?x rdf:type ?c`.
fn apply_cls_hv2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_restrictions(store, OWL_HAS_VALUE, a, i)?;
    let mut derived = Vec::new();
    for (c, p, v) in restrictions {
        let c_term = subj_to_term(&c);
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        for (x, obj) in p_pairs {
            if obj == v {
                derived.push(triple(x, RDF_TYPE, c_term.clone()));
            }
        }
    }
    Ok(derived)
}

/// `cls-maxc2`: `?c owl:maxCardinality 1 ; owl:onProperty ?p . ?x rdf:type ?c .
///              ?x ?p ?y1 . ?x ?p ?y2` → `?y1 owl:sameAs ?y2`.
fn apply_cls_maxc2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let restrictions = collect_cardinality_restrictions(store, OWL_MAX_CARDINALITY, 1, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, p) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(insts) = instances_of.get(&c_term) else {
            continue;
        };
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        let mut objs_of: HashMap<Subject, Vec<Term>> = HashMap::new();
        for (s, o) in p_pairs {
            objs_of.entry(s).or_default().push(o);
        }
        for x in insts {
            let Some(objs) = objs_of.get(x) else {
                continue;
            };
            emit_pairwise_same_as(objs, &mut derived);
        }
    }
    Ok(derived)
}

/// `cls-maxqc3`: `?c owl:maxQualifiedCardinality 1 ; owl:onProperty ?p ;
///               owl:onClass ?cls . ?x rdf:type ?c . ?x ?p ?y1 .
///               ?y1 rdf:type ?cls . ?x ?p ?y2 . ?y2 rdf:type ?cls` →
/// `?y1 owl:sameAs ?y2`. Filler-class type filter is enforced.
fn apply_cls_maxqc3(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    cls_maxqc(store, a, i, /* thing_only = */ false)
}

/// `cls-maxqc4`: same as `cls-maxqc3` but with `owl:onClass owl:Thing` — and
/// the filler-class type filter is skipped (every term is implicitly `owl:Thing`).
fn apply_cls_maxqc4(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    cls_maxqc(store, a, i, /* thing_only = */ true)
}

fn cls_maxqc(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
    thing_only: bool,
) -> Result<Vec<Triple>> {
    // Build index: restriction class → (onProperty, onClass)
    let on_property_pairs = pairs_for_predicate(store, OWL_ON_PROPERTY, a, i)?;
    let on_class_pairs = pairs_for_predicate(store, OWL_ON_CLASS, a, i)?;
    let max_qc_pairs = pairs_for_predicate(store, OWL_MAX_QUALIFIED_CARDINALITY, a, i)?;

    let mut on_of: HashMap<Subject, NamedNode> = HashMap::new();
    for (c, t) in on_property_pairs {
        if let Some(p) = term_to_named(&t) {
            on_of.insert(c, p);
        }
    }
    let mut on_class_of: HashMap<Subject, Term> = HashMap::new();
    for (c, t) in on_class_pairs {
        on_class_of.insert(c, t);
    }
    let thing_term = Term::NamedNode(OWL_THING.into_owned());

    let mut restrictions: Vec<(Subject, NamedNode, Term)> = Vec::new();
    for (c, v) in max_qc_pairs {
        if !literal_int_value_eq(&v, 1) {
            continue;
        }
        let Some(p) = on_of.get(&c) else { continue };
        let Some(cls) = on_class_of.get(&c) else {
            continue;
        };
        let is_thing = cls == &thing_term;
        // Route maxqc3 to non-Thing fillers; maxqc4 only fires on Thing.
        if thing_only != is_thing {
            continue;
        }
        restrictions.push((c.clone(), p.clone(), cls.clone()));
    }

    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let types_of = type_pairs_index_by_subject(store, a, i)?;

    let mut derived = Vec::new();
    for (c, p, cls) in restrictions {
        let c_term = subj_to_term(&c);
        let Some(c_instances) = instances_of.get(&c_term) else {
            continue;
        };
        let p_pairs = pairs_for_predicate(store, p.as_ref(), a, i)?;
        let mut objs_of: HashMap<Subject, Vec<Term>> = HashMap::new();
        for (s, o) in p_pairs {
            objs_of.entry(s).or_default().push(o);
        }
        for x in c_instances {
            let Some(objs) = objs_of.get(x) else {
                continue;
            };
            // For maxqc3, filter to objects whose type contains the onClass.
            // For maxqc4 (thing_only), every object qualifies.
            let valid: Vec<&Term> = if thing_only {
                objs.iter().collect()
            } else {
                objs.iter()
                    .filter(|y| {
                        let Some(y_subj) = term_to_subj(y) else {
                            return false;
                        };
                        types_of
                            .get(&y_subj)
                            .map(|set| set.contains(&cls))
                            .unwrap_or(false)
                    })
                    .collect()
            };
            for (idx, y1) in valid.iter().enumerate() {
                for y2 in valid.iter().skip(idx + 1) {
                    let Some(y1_s) = term_to_subj(y1) else {
                        continue;
                    };
                    let Some(y2_s) = term_to_subj(y2) else {
                        continue;
                    };
                    derived.push(triple(y1_s.clone(), OWL_SAME_AS, subj_to_term(&y2_s)));
                    derived.push(triple(y2_s, OWL_SAME_AS, subj_to_term(&y1_s)));
                }
            }
        }
    }
    Ok(derived)
}

/// `cls-oo`: `?c owl:oneOf (?x1 … ?xn)` → `?xi rdf:type ?c` for each i.
fn apply_cls_oo(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let lists = collect_class_lists(store, OWL_ONE_OF, a, i)?;
    let mut derived = Vec::new();
    for (c, members) in lists {
        let c_term = subj_to_term(&c);
        for m in members {
            let Some(m_subj) = term_to_subj(&m) else {
                continue;
            };
            derived.push(triple(m_subj, RDF_TYPE, c_term.clone()));
        }
    }
    Ok(derived)
}

/// `cax-eqc1`: `?c1 owl:equivalentClass ?c2 . ?x rdf:type ?c1` →
/// `?x rdf:type ?c2`.
fn apply_cax_eqc1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_class_member_propagation(store, /* reverse = */ false, a, i)
}

/// `cax-eqc2`: `?c1 owl:equivalentClass ?c2 . ?x rdf:type ?c2` →
/// `?x rdf:type ?c1`.
fn apply_cax_eqc2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_class_member_propagation(store, /* reverse = */ true, a, i)
}

fn equivalent_class_member_propagation(
    store: &Store,
    reverse: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let eq_pairs = pairs_for_predicate(store, OWL_EQUIVALENT_CLASS, a, i)?;
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c1, c2) in eq_pairs {
        // cax-eqc1: propagate from c1 → c2 (instances of c1 also rdf:type c2)
        // cax-eqc2: propagate from c2 → c1
        let (source_class, target_class_term) = if reverse {
            (c2.clone(), subj_to_term(&c1))
        } else {
            (subj_to_term(&c1), c2)
        };
        if let Some(insts) = instances_of.get(&source_class) {
            for x in insts {
                derived.push(triple(x.clone(), RDF_TYPE, target_class_term.clone()));
            }
        }
    }
    Ok(derived)
}

// ── Phase C helpers ──────────────────────────────────────────────────────────

/// Walk every `(class, rdf:list head)` for the given list predicate
/// (`owl:intersectionOf` / `owl:unionOf` / `owl:oneOf`), returning the
/// expanded list of member Terms per class.
fn collect_class_lists(
    store: &Store,
    list_predicate: NamedNodeRef<'_>,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<(Subject, Vec<Term>)>> {
    let pairs = pairs_for_predicate(store, list_predicate, a, i)?;
    let graphs = graphs_to_query(a, i);
    let mut out = Vec::new();
    for (c, list_head_term) in pairs {
        let Some(head_subj) = term_to_subj(&list_head_term) else {
            continue;
        };
        let Some(members) = super::rdf_lists::walk_list(store, &head_subj, &graphs)? else {
            continue;
        };
        out.push((c, members));
    }
    Ok(out)
}

/// Index `(?s rdf:type ?c)` triples by the class term `?c`.
fn type_pairs_index_by_class(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<HashMap<Term, HashSet<Subject>>> {
    let pairs = pairs_for_predicate(store, RDF_TYPE, a, i)?;
    let mut out: HashMap<Term, HashSet<Subject>> = HashMap::new();
    for (x, c) in pairs {
        out.entry(c).or_default().insert(x);
    }
    Ok(out)
}

/// Index `(?s rdf:type ?c)` triples by the subject `?s`.
fn type_pairs_index_by_subject(
    store: &Store,
    a: &GraphName,
    i: &GraphName,
) -> Result<HashMap<Subject, HashSet<Term>>> {
    let pairs = pairs_for_predicate(store, RDF_TYPE, a, i)?;
    let mut out: HashMap<Subject, HashSet<Term>> = HashMap::new();
    for (x, c) in pairs {
        out.entry(x).or_default().insert(c);
    }
    Ok(out)
}

/// Collect `(restriction_class, on_property)` for cardinality restrictions
/// whose value equals `target_value` (e.g. `owl:maxCardinality 1`). The
/// value comparison parses the literal's lexical form as a `u32`, matching
/// any XSD integer-shaped datatype with the right numeric value.
fn collect_cardinality_restrictions(
    store: &Store,
    cardinality_predicate: NamedNodeRef<'_>,
    target_value: u32,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<(Subject, NamedNode)>> {
    let on_property_pairs = pairs_for_predicate(store, OWL_ON_PROPERTY, a, i)?;
    let mut on_of: HashMap<Subject, NamedNode> = HashMap::new();
    for (c, p_term) in on_property_pairs {
        if let Some(p_n) = term_to_named(&p_term) {
            on_of.insert(c, p_n);
        }
    }
    let card_pairs = pairs_for_predicate(store, cardinality_predicate, a, i)?;
    let mut out = Vec::new();
    for (c, v) in card_pairs {
        if !literal_int_value_eq(&v, target_value) {
            continue;
        }
        if let Some(p) = on_of.get(&c) {
            out.push((c, p.clone()));
        }
    }
    Ok(out)
}

/// True iff the term is a literal whose lexical form parses as the given
/// unsigned integer. Covers `"0"^^xsd:integer`, `"0"^^xsd:nonNegativeInteger`,
/// `"0"^^xsd:int`, plain `"0"`, etc. — the comparison is on the abstract
/// numeric value, not the datatype IRI.
fn literal_int_value_eq(t: &Term, target: u32) -> bool {
    let Term::Literal(lit) = t else {
        return false;
    };
    lit.value().parse::<u32>().map(|v| v == target).unwrap_or(false)
}

/// Emit pairwise `owl:sameAs` triples (both directions) for every pair of
/// distinct objects in the given slice. Skips literal objects (which can't
/// be subjects of `sameAs` in a Quad we'd insert).
fn emit_pairwise_same_as(objs: &[Term], out: &mut Vec<Triple>) {
    for (idx, y1) in objs.iter().enumerate() {
        for y2 in objs.iter().skip(idx + 1) {
            let Some(y1_s) = term_to_subj(y1) else {
                continue;
            };
            let Some(y2_s) = term_to_subj(y2) else {
                continue;
            };
            out.push(triple(y1_s.clone(), OWL_SAME_AS, subj_to_term(&y2_s)));
            out.push(triple(y2_s, OWL_SAME_AS, subj_to_term(&y1_s)));
        }
    }
}

// ── 0.10.0 Phase D — remaining property + equality rules ────────────────────

/// `prp-ifp`: `?p rdf:type owl:InverseFunctionalProperty . ?x1 ?p ?y .
///            ?x2 ?p ?y` → `?x1 owl:sameAs ?x2`.
fn apply_prp_ifp(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let ifps = instances_of(store, OWL_INVERSE_FUNCTIONAL_PROPERTY, a, i)?;
    let mut derived = Vec::new();
    for p_subj in ifps {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        // For each p, group subjects by object.
        let mut subjects_of: HashMap<Term, Vec<Subject>> = HashMap::new();
        for graph in graphs_to_query(a, i) {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                subjects_of.entry(q.object).or_default().push(q.subject);
            }
        }
        for (_, subjects) in subjects_of {
            for (idx, x1) in subjects.iter().enumerate() {
                for x2 in subjects.iter().skip(idx + 1) {
                    if x1 == x2 {
                        continue;
                    }
                    derived.push(triple(x1.clone(), OWL_SAME_AS, subj_to_term(x2)));
                    derived.push(triple(x2.clone(), OWL_SAME_AS, subj_to_term(x1)));
                }
            }
        }
    }
    Ok(derived)
}

/// `prp-spo2`: property-chain composition.
/// `?p owl:propertyChainAxiom (?p1 … ?pn) . ?u1 ?p1 ?u2 . … . ?un ?pn ?u(n+1)` →
/// `?u1 ?p ?u(n+1)`.
fn apply_prp_spo2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let chain_pairs = pairs_for_predicate(store, OWL_PROPERTY_CHAIN_AXIOM, a, i)?;
    let graphs = graphs_to_query(a, i);
    let mut derived = Vec::new();
    for (p_subj, chain_head_term) in chain_pairs {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        let Some(chain_head) = term_to_subj(&chain_head_term) else {
            continue;
        };
        let Some(members) = super::rdf_lists::walk_list(store, &chain_head, &graphs)? else {
            continue;
        };
        if members.is_empty() {
            continue;
        }
        let chain_props_opt: Option<Vec<NamedNode>> =
            members.iter().map(term_to_named).collect();
        let Some(chain_props) = chain_props_opt else {
            continue;
        };

        // Seed with pairs from the first property.
        let mut current: HashSet<(Subject, Term)> = HashSet::new();
        for graph in &graphs {
            let g_ref = graph_to_ref(graph);
            for q in
                store.quads_for_pattern(None, Some(chain_props[0].as_ref()), None, Some(g_ref))
            {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                current.insert((q.subject, q.object));
            }
        }
        // Sequentially join with each subsequent property.
        for prop in &chain_props[1..] {
            if current.is_empty() {
                break;
            }
            // Index current by end-term so we can match against next-property subjects.
            let mut by_end: HashMap<Term, Vec<Subject>> = HashMap::new();
            for (start, end) in &current {
                by_end.entry(end.clone()).or_default().push(start.clone());
            }
            let mut next: HashSet<(Subject, Term)> = HashSet::new();
            for graph in &graphs {
                let g_ref = graph_to_ref(graph);
                for q in store.quads_for_pattern(None, Some(prop.as_ref()), None, Some(g_ref)) {
                    let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                    let q_subj_as_term = subj_to_term(&q.subject);
                    if let Some(starts) = by_end.get(&q_subj_as_term) {
                        for s in starts {
                            next.insert((s.clone(), q.object.clone()));
                        }
                    }
                }
            }
            current = next;
        }
        for (u1, u_end) in current {
            derived.push(Triple::new(u1, p.clone(), u_end));
        }
    }
    Ok(derived)
}

/// `prp-eqp1`: `?p1 owl:equivalentProperty ?p2 . ?x ?p1 ?y` → `?x ?p2 ?y`.
fn apply_prp_eqp1(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_property_propagation(store, /* reverse = */ false, a, i)
}

/// `prp-eqp2`: `?p1 owl:equivalentProperty ?p2 . ?x ?p2 ?y` → `?x ?p1 ?y`.
fn apply_prp_eqp2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    equivalent_property_propagation(store, /* reverse = */ true, a, i)
}

fn equivalent_property_propagation(
    store: &Store,
    reverse: bool,
    a: &GraphName,
    i: &GraphName,
) -> Result<Vec<Triple>> {
    let eq_pairs = pairs_for_predicate(store, OWL_EQUIVALENT_PROPERTY, a, i)?;
    let graphs = graphs_to_query(a, i);
    let mut derived = Vec::new();
    for (p1_subj, p2_term) in eq_pairs {
        let Subject::NamedNode(p1) = p1_subj else {
            continue;
        };
        let Some(p2) = term_to_named(&p2_term) else {
            continue;
        };
        let (source, target) = if reverse {
            (p2.clone(), p1.clone())
        } else {
            (p1.clone(), p2.clone())
        };
        for graph in &graphs {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(None, Some(source.as_ref()), None, Some(g_ref)) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                derived.push(Triple::new(q.subject, target.clone(), q.object));
            }
        }
    }
    Ok(derived)
}

/// `prp-key`: `?c owl:hasKey (?p1 … ?pn) . ?x rdf:type ?c . ?x ?p1 ?z1 . … .
///            ?y rdf:type ?c . ?y ?p1 ?z1 . … . ?y ?pn ?zn` → `?x owl:sameAs ?y`.
///
/// Instances of `?c` whose key-property values agree are merged. When a
/// property has multiple values for an instance, every value-tuple in the
/// cartesian product is a candidate key, matching the W3C semantics.
fn apply_prp_key(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let has_key_pairs = pairs_for_predicate(store, OWL_HAS_KEY, a, i)?;
    let graphs = graphs_to_query(a, i);
    let instances_of = type_pairs_index_by_class(store, a, i)?;
    let mut derived = Vec::new();
    for (c, key_head_term) in has_key_pairs {
        let Some(key_head) = term_to_subj(&key_head_term) else {
            continue;
        };
        let Some(key_members) = super::rdf_lists::walk_list(store, &key_head, &graphs)? else {
            continue;
        };
        if key_members.is_empty() {
            continue;
        }
        let key_props_opt: Option<Vec<NamedNode>> =
            key_members.iter().map(term_to_named).collect();
        let Some(key_props) = key_props_opt else {
            continue;
        };

        let c_term = subj_to_term(&c);
        let Some(insts) = instances_of.get(&c_term) else {
            continue;
        };

        // Build one (subject → set-of-values) index per key property.
        let mut values_per_prop: Vec<HashMap<Subject, HashSet<Term>>> =
            Vec::with_capacity(key_props.len());
        for kp in &key_props {
            let mut v: HashMap<Subject, HashSet<Term>> = HashMap::new();
            for graph in &graphs {
                let g_ref = graph_to_ref(graph);
                for q in store.quads_for_pattern(None, Some(kp.as_ref()), None, Some(g_ref)) {
                    let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                    v.entry(q.subject).or_default().insert(q.object);
                }
            }
            values_per_prop.push(v);
        }

        // Bucket instances by cartesian-product key tuple.
        let mut by_tuple: HashMap<Vec<Term>, Vec<Subject>> = HashMap::new();
        for inst in insts {
            let mut value_sets: Vec<Vec<Term>> = Vec::with_capacity(values_per_prop.len());
            let mut complete = true;
            for vp in &values_per_prop {
                let Some(set) = vp.get(inst) else {
                    complete = false;
                    break;
                };
                if set.is_empty() {
                    complete = false;
                    break;
                }
                value_sets.push(set.iter().cloned().collect());
            }
            if !complete {
                continue;
            }
            for tup in cartesian_product(&value_sets) {
                by_tuple.entry(tup).or_default().push(inst.clone());
            }
        }
        // Each tuple shared by 2+ instances → pairwise sameAs (both directions).
        for group in by_tuple.values() {
            if group.len() < 2 {
                continue;
            }
            for (idx, x) in group.iter().enumerate() {
                for y in group.iter().skip(idx + 1) {
                    if x == y {
                        continue;
                    }
                    derived.push(triple(x.clone(), OWL_SAME_AS, subj_to_term(y)));
                    derived.push(triple(y.clone(), OWL_SAME_AS, subj_to_term(x)));
                }
            }
        }
    }
    Ok(derived)
}

/// `eq-ref`: `?s ?p ?o` → `?s owl:sameAs ?s . ?p owl:sameAs ?p . ?o owl:sameAs ?o`.
///
/// W3C semantics — every term used in any quad position derives a reflexive
/// `owl:sameAs`. Bounded after the first iteration by the fixpoint dedup, but
/// the storage cost on iteration 1 is `~3 × |store|`. Literals in object
/// position are skipped (they can't be the subject of a quad we insert).
fn apply_eq_ref(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let quads = all_quads(store, a, i)?;
    let mut derived: HashSet<Triple> = HashSet::new();
    for (s, p, o) in quads {
        let s_term = subj_to_term(&s);
        derived.insert(triple(s.clone(), OWL_SAME_AS, s_term));
        derived.insert(triple(
            Subject::NamedNode(p.clone()),
            OWL_SAME_AS,
            Term::NamedNode(p),
        ));
        if let Some(o_subj) = term_to_subj(&o) {
            derived.insert(triple(o_subj, OWL_SAME_AS, o));
        }
    }
    Ok(derived.into_iter().collect())
}

/// `eq-rep-s`: `?s owl:sameAs ?s2 . ?s ?p ?o` → `?s2 ?p ?o`.
fn apply_eq_rep_s(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let same_pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut substitutes: HashMap<Subject, HashSet<Subject>> = HashMap::new();
    for (s, s2_term) in same_pairs {
        let Some(s2_subj) = term_to_subj(&s2_term) else {
            continue;
        };
        if s == s2_subj {
            continue;
        }
        substitutes.entry(s).or_default().insert(s2_subj);
    }
    let quads = all_quads(store, a, i)?;
    let mut derived = Vec::new();
    for (s, p, o) in quads {
        let Some(subs) = substitutes.get(&s) else {
            continue;
        };
        for s2 in subs {
            derived.push(Triple::new(s2.clone(), p.clone(), o.clone()));
        }
    }
    Ok(derived)
}

/// `eq-rep-p`: `?p owl:sameAs ?p2 . ?s ?p ?o` → `?s ?p2 ?o`. Predicates must
/// be IRIs, so sameAs pairs whose `?p`/`?p2` is a blank node or literal are
/// skipped.
fn apply_eq_rep_p(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let same_pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut substitutes: HashMap<NamedNode, HashSet<NamedNode>> = HashMap::new();
    for (p_subj, p2_term) in same_pairs {
        let Subject::NamedNode(p) = p_subj else {
            continue;
        };
        let Some(p2) = term_to_named(&p2_term) else {
            continue;
        };
        if p == p2 {
            continue;
        }
        substitutes.entry(p).or_default().insert(p2);
    }
    let quads = all_quads(store, a, i)?;
    let mut derived = Vec::new();
    for (s, p, o) in quads {
        let Some(subs) = substitutes.get(&p) else {
            continue;
        };
        for p2 in subs {
            derived.push(Triple::new(s.clone(), p2.clone(), o.clone()));
        }
    }
    Ok(derived)
}

/// `eq-rep-o`: `?o owl:sameAs ?o2 . ?s ?p ?o` → `?s ?p ?o2`.
fn apply_eq_rep_o(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let same_pairs = pairs_for_predicate(store, OWL_SAME_AS, a, i)?;
    let mut substitutes: HashMap<Term, HashSet<Term>> = HashMap::new();
    for (o_subj, o2_term) in same_pairs {
        let o_term = subj_to_term(&o_subj);
        if o_term == o2_term {
            continue;
        }
        substitutes.entry(o_term).or_default().insert(o2_term);
    }
    let quads = all_quads(store, a, i)?;
    let mut derived = Vec::new();
    for (s, p, o) in quads {
        let Some(subs) = substitutes.get(&o) else {
            continue;
        };
        for o2 in subs {
            derived.push(Triple::new(s.clone(), p.clone(), o2.clone()));
        }
    }
    Ok(derived)
}

/// Cartesian product of a slice of value sets. Returns `[]` if any input
/// set is empty (no valid tuple can be formed). Used by `prp-key`.
fn cartesian_product(sets: &[Vec<Term>]) -> Vec<Vec<Term>> {
    let mut result: Vec<Vec<Term>> = vec![Vec::new()];
    for set in sets {
        if set.is_empty() {
            return Vec::new();
        }
        let mut next = Vec::with_capacity(result.len() * set.len());
        for partial in &result {
            for item in set {
                let mut extended = partial.clone();
                extended.push(item.clone());
                next.push(extended);
            }
        }
        result = next;
    }
    result
}

// ── 0.10.0 Phase E — datatype rules ──────────────────────────────────────────

/// `dt-type1`: axiomatic — every datatype IRI in the closed W3C list gets
/// `?dt rdf:type rdfs:Datatype`. The list is `DT_TYPE1_DATATYPES` above.
fn apply_dt_type1(_store: &Store, _a: &GraphName, _i: &GraphName) -> Result<Vec<Triple>> {
    let datatype_term = Term::NamedNode(RDFS_DATATYPE.into_owned());
    let mut derived = Vec::with_capacity(DT_TYPE1_DATATYPES.len());
    for iri in DT_TYPE1_DATATYPES {
        derived.push(Triple::new(
            Subject::NamedNode(NamedNode::new_unchecked(*iri)),
            RDF_TYPE.into_owned(),
            datatype_term.clone(),
        ));
    }
    Ok(derived)
}

/// `dt-type2`: `?lt is a literal` → `datatype(?lt) rdf:type rdfs:Datatype`.
/// Picks up consumer-defined datatypes that aren't in `DT_TYPE1_DATATYPES`.
fn apply_dt_type2(store: &Store, a: &GraphName, i: &GraphName) -> Result<Vec<Triple>> {
    let quads = all_quads(store, a, i)?;
    let mut datatypes: HashSet<NamedNode> = HashSet::new();
    for (_, _, o) in quads {
        if let Term::Literal(lit) = o {
            datatypes.insert(lit.datatype().into_owned());
        }
    }
    let datatype_term = Term::NamedNode(RDFS_DATATYPE.into_owned());
    let mut derived = Vec::with_capacity(datatypes.len());
    for dt in datatypes {
        derived.push(Triple::new(
            Subject::NamedNode(dt),
            RDF_TYPE.into_owned(),
            datatype_term.clone(),
        ));
    }
    Ok(derived)
}

/// `dt-eq`: `?lt1 is a literal . ?lt2 is a literal . value(?lt1) =
/// value(?lt2)` → `?lt1 owl:sameAs ?lt2`.
///
/// **Currently a no-op.** Oxigraph 0.4's `Subject` enum does not admit
/// `Literal` variants, so the derived triple is not representable. The
/// W3C OWL 2 RL/RDF spec explicitly extends Subject to permit literals
/// for this rule's emission, but Rust-side construction is type-blocked.
/// Revive when Oxigraph upgrades the model (≥ 0.5) or when a consumer
/// surfaces a workload that needs the literal-sameAs derivations.
fn apply_dt_eq(_store: &Store, _a: &GraphName, _i: &GraphName) -> Result<Vec<Triple>> {
    Ok(Vec::new())
}

/// `dt-diff`: `?lt1 is a literal . ?lt2 is a literal . value(?lt1) ≠
/// value(?lt2)` → `?lt1 owl:differentFrom ?lt2`.
///
/// Same Oxigraph-0.4 model limitation as `dt-eq`. Currently a no-op.
fn apply_dt_diff(_store: &Store, _a: &GraphName, _i: &GraphName) -> Result<Vec<Triple>> {
    Ok(Vec::new())
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

    // ── 0.10.0 Phase B — scm-* T-Box smoke tests ────────────────────────────

    fn assert_contains(derived: &[Triple], expected: &Triple) {
        assert!(
            derived.contains(expected),
            "expected {expected:?}; got {derived:?}"
        );
    }

    fn run(rule: fn(&Store, &GraphName, &GraphName) -> Result<Vec<Triple>>, store: &Store) -> Vec<Triple> {
        rule(store, &GraphName::DefaultGraph, &GraphName::DefaultGraph).unwrap()
    }

    #[test]
    fn scm_cls_emits_reflexive_and_thing_nothing_brackets() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", RDF_TYPE, "http://www.w3.org/2002/07/owl#Class");
        let derived = run(apply_scm_cls, &store);
        let a = iri("http://e/A");
        let thing = iri("http://www.w3.org/2002/07/owl#Thing");
        let nothing = iri("http://www.w3.org/2002/07/owl#Nothing");
        assert_contains(
            &derived,
            &Triple::new(a.clone(), RDFS_SUB_CLASS_OF.into_owned(), a.clone()),
        );
        assert_contains(
            &derived,
            &Triple::new(a.clone(), OWL_EQUIVALENT_CLASS.into_owned(), a.clone()),
        );
        assert_contains(
            &derived,
            &Triple::new(a.clone(), RDFS_SUB_CLASS_OF.into_owned(), thing),
        );
        assert_contains(
            &derived,
            &Triple::new(nothing, RDFS_SUB_CLASS_OF.into_owned(), a),
        );
    }

    #[test]
    fn scm_op_emits_reflexive_subproperty_and_equivalent_property() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/p", RDF_TYPE, "http://www.w3.org/2002/07/owl#ObjectProperty");
        let derived = run(apply_scm_op, &store);
        let p = iri("http://e/p");
        assert_contains(
            &derived,
            &Triple::new(p.clone(), RDFS_SUB_PROPERTY_OF.into_owned(), p.clone()),
        );
        assert_contains(
            &derived,
            &Triple::new(p.clone(), OWL_EQUIVALENT_PROPERTY.into_owned(), p),
        );
    }

    #[test]
    fn scm_dp_emits_reflexive_subproperty_and_equivalent_property() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/p", RDF_TYPE, "http://www.w3.org/2002/07/owl#DatatypeProperty");
        let derived = run(apply_scm_dp, &store);
        let p = iri("http://e/p");
        assert_contains(
            &derived,
            &Triple::new(p.clone(), RDFS_SUB_PROPERTY_OF.into_owned(), p.clone()),
        );
        assert_contains(
            &derived,
            &Triple::new(p.clone(), OWL_EQUIVALENT_PROPERTY.into_owned(), p),
        );
    }

    #[test]
    fn scm_eqc2_collapses_bidirectional_subsumption() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", RDFS_SUB_CLASS_OF, "http://e/B");
        insert(&store, "http://e/B", RDFS_SUB_CLASS_OF, "http://e/A");
        let derived = run(apply_scm_eqc2, &store);
        // Both directions should derive (the rule fires on both pairs).
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/A"), OWL_EQUIVALENT_CLASS.into_owned(), iri("http://e/B")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/B"), OWL_EQUIVALENT_CLASS.into_owned(), iri("http://e/A")),
        );
    }

    #[test]
    fn scm_eqp2_collapses_bidirectional_subproperty() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/p", RDFS_SUB_PROPERTY_OF, "http://e/q");
        insert(&store, "http://e/q", RDFS_SUB_PROPERTY_OF, "http://e/p");
        let derived = run(apply_scm_eqp2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), OWL_EQUIVALENT_PROPERTY.into_owned(), iri("http://e/q")),
        );
    }

    #[test]
    fn scm_dom1_propagates_domain_up_subclass() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/p", RDFS_DOMAIN, "http://e/A");
        insert(&store, "http://e/A", RDFS_SUB_CLASS_OF, "http://e/B");
        let derived = run(apply_scm_dom1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), RDFS_DOMAIN.into_owned(), iri("http://e/B")),
        );
    }

    #[test]
    fn scm_rng1_propagates_range_up_subclass() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/p", RDFS_RANGE, "http://e/A");
        insert(&store, "http://e/A", RDFS_SUB_CLASS_OF, "http://e/B");
        let derived = run(apply_scm_rng1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), RDFS_RANGE.into_owned(), iri("http://e/B")),
        );
    }

    #[test]
    fn scm_dom2_propagates_domain_down_subproperty() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/q", RDFS_DOMAIN, "http://e/C");
        insert(&store, "http://e/p", RDFS_SUB_PROPERTY_OF, "http://e/q");
        let derived = run(apply_scm_dom2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), RDFS_DOMAIN.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn scm_rng2_propagates_range_down_subproperty() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/q", RDFS_RANGE, "http://e/C");
        insert(&store, "http://e/p", RDFS_SUB_PROPERTY_OF, "http://e/q");
        let derived = run(apply_scm_rng2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), RDFS_RANGE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn scm_hv_subsumes_restrictions_with_compatible_property() {
        let store = Store::new().unwrap();
        // C1 = restriction (p1, hasValue v). C2 = restriction (p2, hasValue v). p1 ⊑ p2.
        insert(&store, "http://e/C1", OWL_ON_PROPERTY, "http://e/p1");
        insert(&store, "http://e/C1", OWL_HAS_VALUE, "http://e/v");
        insert(&store, "http://e/C2", OWL_ON_PROPERTY, "http://e/p2");
        insert(&store, "http://e/C2", OWL_HAS_VALUE, "http://e/v");
        insert(&store, "http://e/p1", RDFS_SUB_PROPERTY_OF, "http://e/p2");
        let derived = run(apply_scm_hv, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C1"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C2")),
        );
    }

    #[test]
    fn scm_svf1_subsumes_via_filler_subclass() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C1", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C1", OWL_SOME_VALUES_FROM, "http://e/Y1");
        insert(&store, "http://e/C2", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C2", OWL_SOME_VALUES_FROM, "http://e/Y2");
        insert(&store, "http://e/Y1", RDFS_SUB_CLASS_OF, "http://e/Y2");
        let derived = run(apply_scm_svf1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C1"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C2")),
        );
    }

    #[test]
    fn scm_svf2_subsumes_via_onproperty_subsumption() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C1", OWL_ON_PROPERTY, "http://e/p1");
        insert(&store, "http://e/C1", OWL_SOME_VALUES_FROM, "http://e/Y");
        insert(&store, "http://e/C2", OWL_ON_PROPERTY, "http://e/p2");
        insert(&store, "http://e/C2", OWL_SOME_VALUES_FROM, "http://e/Y");
        insert(&store, "http://e/p1", RDFS_SUB_PROPERTY_OF, "http://e/p2");
        let derived = run(apply_scm_svf2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C1"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C2")),
        );
    }

    #[test]
    fn scm_avf1_subsumes_via_filler_subclass() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C1", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C1", OWL_ALL_VALUES_FROM, "http://e/Y1");
        insert(&store, "http://e/C2", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C2", OWL_ALL_VALUES_FROM, "http://e/Y2");
        insert(&store, "http://e/Y1", RDFS_SUB_CLASS_OF, "http://e/Y2");
        let derived = run(apply_scm_avf1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C1"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C2")),
        );
    }

    #[test]
    fn scm_avf2_subsumes_in_reverse_direction() {
        let store = Store::new().unwrap();
        // For allValuesFrom, super-property restriction is sub-class of sub-property
        // restriction (direction flips relative to svf2).
        insert(&store, "http://e/C1", OWL_ON_PROPERTY, "http://e/p1");
        insert(&store, "http://e/C1", OWL_ALL_VALUES_FROM, "http://e/Y");
        insert(&store, "http://e/C2", OWL_ON_PROPERTY, "http://e/p2");
        insert(&store, "http://e/C2", OWL_ALL_VALUES_FROM, "http://e/Y");
        insert(&store, "http://e/p1", RDFS_SUB_PROPERTY_OF, "http://e/p2");
        let derived = run(apply_scm_avf2, &store);
        // C2 (with super-property p2) ⊑ C1 (with sub-property p1)
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C2"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C1")),
        );
    }

    fn insert_list(store: &Store, head: &str, members: &[&str]) {
        // Build (m1 m2 … mn) chained at IRI nodes named head, head_1, …
        let mut current = head.to_string();
        for (idx, m) in members.iter().enumerate() {
            let rest = if idx + 1 == members.len() {
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil".to_string()
            } else {
                format!("{head}_{}", idx + 1)
            };
            insert(
                store,
                &current,
                NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#first"),
                m,
            );
            insert(
                store,
                &current,
                NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#rest"),
                &rest,
            );
            current = rest;
        }
    }

    #[test]
    fn scm_int_emits_subclass_to_each_member() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_INTERSECTION_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/A", "http://e/B"]);
        let derived = run(apply_scm_int, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/A")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/C"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/B")),
        );
    }

    #[test]
    fn scm_uni_emits_subclass_from_each_member() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_UNION_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/A", "http://e/B"]);
        let derived = run(apply_scm_uni, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/A"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/B"), RDFS_SUB_CLASS_OF.into_owned(), iri("http://e/C")),
        );
    }

    // ── 0.10.0 Phase C — class-expression A-Box smoke tests ─────────────────

    fn insert_typed(store: &Store, s: &str, c: &str) {
        insert(store, s, RDF_TYPE, c);
    }

    fn insert_literal(store: &Store, s: &str, p: NamedNodeRef<'_>, lit: oxigraph::model::Literal) {
        store
            .insert(&Quad::new(
                iri(s),
                p.into_owned(),
                Term::Literal(lit),
                GraphName::DefaultGraph,
            ))
            .unwrap();
    }

    fn xsd_int_one() -> oxigraph::model::Literal {
        oxigraph::model::Literal::new_typed_literal(
            "1",
            NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#nonNegativeInteger"),
        )
    }

    #[test]
    fn cls_thing_emits_axiom() {
        let store = Store::new().unwrap();
        let derived = run(apply_cls_thing, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://www.w3.org/2002/07/owl#Thing"),
                RDF_TYPE.into_owned(),
                iri("http://www.w3.org/2002/07/owl#Class"),
            ),
        );
    }

    #[test]
    fn cls_nothing1_emits_axiom() {
        let store = Store::new().unwrap();
        let derived = run(apply_cls_nothing1, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://www.w3.org/2002/07/owl#Nothing"),
                RDF_TYPE.into_owned(),
                iri("http://www.w3.org/2002/07/owl#Class"),
            ),
        );
    }

    #[test]
    fn cls_int1_requires_all_member_types() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_INTERSECTION_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/A", "http://e/B"]);
        insert_typed(&store, "http://e/x", "http://e/A");
        insert_typed(&store, "http://e/x", "http://e/B");
        insert_typed(&store, "http://e/y", "http://e/A"); // y misses B
        let derived = run(apply_cls_int1, &store);
        let x_c = Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/C"));
        let y_c = Triple::new(iri("http://e/y"), RDF_TYPE.into_owned(), iri("http://e/C"));
        assert!(derived.contains(&x_c), "expected x:type C; got {derived:?}");
        assert!(!derived.contains(&y_c), "y should NOT be inferred as C; got {derived:?}");
    }

    #[test]
    fn cls_int2_decomposes_member_types() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_INTERSECTION_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/A", "http://e/B"]);
        insert_typed(&store, "http://e/x", "http://e/C");
        let derived = run(apply_cls_int2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/A")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/B")),
        );
    }

    #[test]
    fn cls_uni_propagates_from_any_member() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_UNION_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/A", "http://e/B"]);
        insert_typed(&store, "http://e/x", "http://e/A");
        let derived = run(apply_cls_uni, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn cls_svf1_typed_value() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_SOME_VALUES_FROM, "http://e/Y");
        insert(&store, "http://e/u", p, "http://e/v");
        insert_typed(&store, "http://e/v", "http://e/Y");
        let derived = run(apply_cls_svf1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/u"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn cls_svf2_propagates_via_thing_filler() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_SOME_VALUES_FROM, "http://www.w3.org/2002/07/owl#Thing");
        insert(&store, "http://e/u", p, "http://e/v");
        let derived = run(apply_cls_svf2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/u"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn cls_avf_constrains_property_values() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_ALL_VALUES_FROM, "http://e/Y");
        insert_typed(&store, "http://e/x", "http://e/C");
        insert(&store, "http://e/x", p, "http://e/u");
        let derived = run(apply_cls_avf, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/u"), RDF_TYPE.into_owned(), iri("http://e/Y")),
        );
    }

    #[test]
    fn cls_hv1_asserts_required_value() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_HAS_VALUE, "http://e/v");
        insert_typed(&store, "http://e/x", "http://e/C");
        let derived = run(apply_cls_hv1, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://e/x"),
                NamedNode::new_unchecked("http://e/p"),
                iri("http://e/v"),
            ),
        );
    }

    #[test]
    fn cls_hv2_detects_class_membership_from_value() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_HAS_VALUE, "http://e/v");
        insert(&store, "http://e/x", p, "http://e/v");
        let derived = run(apply_cls_hv2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn cls_maxc2_collapses_to_same_as() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert_literal(&store, "http://e/C", OWL_MAX_CARDINALITY, xsd_int_one());
        insert_typed(&store, "http://e/x", "http://e/C");
        insert(&store, "http://e/x", p, "http://e/y1");
        insert(&store, "http://e/x", p, "http://e/y2");
        let derived = run(apply_cls_maxc2, &store);
        let y1_y2 = Triple::new(iri("http://e/y1"), OWL_SAME_AS.into_owned(), iri("http://e/y2"));
        let y2_y1 = Triple::new(iri("http://e/y2"), OWL_SAME_AS.into_owned(), iri("http://e/y1"));
        assert!(derived.contains(&y1_y2) || derived.contains(&y2_y1));
    }

    #[test]
    fn cls_maxqc3_collapses_when_both_match_onclass() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_ON_CLASS, "http://e/Person");
        insert_literal(&store, "http://e/C", OWL_MAX_QUALIFIED_CARDINALITY, xsd_int_one());
        insert_typed(&store, "http://e/x", "http://e/C");
        insert(&store, "http://e/x", p, "http://e/y1");
        insert(&store, "http://e/x", p, "http://e/y2");
        insert_typed(&store, "http://e/y1", "http://e/Person");
        insert_typed(&store, "http://e/y2", "http://e/Person");
        let derived = run(apply_cls_maxqc3, &store);
        let y1_y2 = Triple::new(iri("http://e/y1"), OWL_SAME_AS.into_owned(), iri("http://e/y2"));
        let y2_y1 = Triple::new(iri("http://e/y2"), OWL_SAME_AS.into_owned(), iri("http://e/y1"));
        assert!(derived.contains(&y1_y2) || derived.contains(&y2_y1));
    }

    #[test]
    fn cls_maxqc3_skips_when_one_lacks_onclass_type() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_ON_CLASS, "http://e/Person");
        insert_literal(&store, "http://e/C", OWL_MAX_QUALIFIED_CARDINALITY, xsd_int_one());
        insert_typed(&store, "http://e/x", "http://e/C");
        insert(&store, "http://e/x", p, "http://e/y1");
        insert(&store, "http://e/x", p, "http://e/y2");
        insert_typed(&store, "http://e/y1", "http://e/Person");
        // y2 NOT typed as Person — must not collapse
        let derived = run(apply_cls_maxqc3, &store);
        assert!(derived.is_empty(), "expected no derivation; got {derived:?}");
    }

    #[test]
    fn cls_maxqc4_collapses_for_owl_thing_onclass() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/C", OWL_ON_PROPERTY, "http://e/p");
        insert(&store, "http://e/C", OWL_ON_CLASS, "http://www.w3.org/2002/07/owl#Thing");
        insert_literal(&store, "http://e/C", OWL_MAX_QUALIFIED_CARDINALITY, xsd_int_one());
        insert_typed(&store, "http://e/x", "http://e/C");
        insert(&store, "http://e/x", p, "http://e/y1");
        insert(&store, "http://e/x", p, "http://e/y2");
        // No explicit Thing typing on y1/y2 — that's the whole point of maxqc4
        let derived = run(apply_cls_maxqc4, &store);
        let y1_y2 = Triple::new(iri("http://e/y1"), OWL_SAME_AS.into_owned(), iri("http://e/y2"));
        let y2_y1 = Triple::new(iri("http://e/y2"), OWL_SAME_AS.into_owned(), iri("http://e/y1"));
        assert!(derived.contains(&y1_y2) || derived.contains(&y2_y1));
    }

    #[test]
    fn cls_oo_types_each_enumerated_member() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/C", OWL_ONE_OF, "http://e/L");
        insert_list(&store, "http://e/L", &["http://e/a", "http://e/b"]);
        let derived = run(apply_cls_oo, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/a"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/b"), RDF_TYPE.into_owned(), iri("http://e/C")),
        );
    }

    #[test]
    fn cax_eqc1_propagates_member_from_c1_to_c2() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", OWL_EQUIVALENT_CLASS, "http://e/B");
        insert_typed(&store, "http://e/x", "http://e/A");
        let derived = run(apply_cax_eqc1, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/B")),
        );
    }

    #[test]
    fn cax_eqc2_propagates_member_from_c2_to_c1() {
        let store = Store::new().unwrap();
        insert(&store, "http://e/A", OWL_EQUIVALENT_CLASS, "http://e/B");
        insert_typed(&store, "http://e/x", "http://e/B");
        let derived = run(apply_cax_eqc2, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/x"), RDF_TYPE.into_owned(), iri("http://e/A")),
        );
    }

    // ── 0.10.0 Phase D — property + equality smoke tests ────────────────────

    #[test]
    fn prp_ifp_collapses_subjects_sharing_value() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/email");
        insert(&store, "http://e/email", RDF_TYPE,
            "http://www.w3.org/2002/07/owl#InverseFunctionalProperty");
        insert(&store, "http://e/alice", p, "http://e/inbox42");
        insert(&store, "http://e/al", p, "http://e/inbox42");
        let derived = run(apply_prp_ifp, &store);
        let alice_al = Triple::new(iri("http://e/alice"), OWL_SAME_AS.into_owned(), iri("http://e/al"));
        let al_alice = Triple::new(iri("http://e/al"), OWL_SAME_AS.into_owned(), iri("http://e/alice"));
        assert!(derived.contains(&alice_al) || derived.contains(&al_alice));
    }

    #[test]
    fn prp_spo2_composes_two_step_chain() {
        let store = Store::new().unwrap();
        let parent = NamedNodeRef::new_unchecked("http://e/parent");
        let sibling = NamedNodeRef::new_unchecked("http://e/sibling");
        // uncle = parent ∘ sibling
        insert(&store, "http://e/uncle", OWL_PROPERTY_CHAIN_AXIOM, "http://e/chain");
        insert_list(&store, "http://e/chain", &["http://e/parent", "http://e/sibling"]);
        insert(&store, "http://e/alice", parent, "http://e/bob");
        insert(&store, "http://e/bob", sibling, "http://e/carol");
        let derived = run(apply_prp_spo2, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://e/alice"),
                NamedNode::new_unchecked("http://e/uncle"),
                iri("http://e/carol"),
            ),
        );
    }

    #[test]
    fn prp_eqp1_propagates_from_p1_to_p2() {
        let store = Store::new().unwrap();
        let p1 = NamedNodeRef::new_unchecked("http://e/p1");
        insert(&store, "http://e/p1", OWL_EQUIVALENT_PROPERTY, "http://e/p2");
        insert(&store, "http://e/a", p1, "http://e/b");
        let derived = run(apply_prp_eqp1, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://e/a"),
                NamedNode::new_unchecked("http://e/p2"),
                iri("http://e/b"),
            ),
        );
    }

    #[test]
    fn prp_eqp2_propagates_from_p2_to_p1() {
        let store = Store::new().unwrap();
        let p2 = NamedNodeRef::new_unchecked("http://e/p2");
        insert(&store, "http://e/p1", OWL_EQUIVALENT_PROPERTY, "http://e/p2");
        insert(&store, "http://e/a", p2, "http://e/b");
        let derived = run(apply_prp_eqp2, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://e/a"),
                NamedNode::new_unchecked("http://e/p1"),
                iri("http://e/b"),
            ),
        );
    }

    #[test]
    fn prp_key_collapses_instances_with_matching_key() {
        let store = Store::new().unwrap();
        let given = NamedNodeRef::new_unchecked("http://e/given");
        let family = NamedNodeRef::new_unchecked("http://e/family");
        insert(&store, "http://e/Person", OWL_HAS_KEY, "http://e/key");
        insert_list(&store, "http://e/key", &["http://e/given", "http://e/family"]);
        insert_typed(&store, "http://e/p1", "http://e/Person");
        insert_typed(&store, "http://e/p2", "http://e/Person");
        insert_literal(
            &store, "http://e/p1", given,
            oxigraph::model::Literal::new_simple_literal("Alice"),
        );
        insert_literal(
            &store, "http://e/p1", family,
            oxigraph::model::Literal::new_simple_literal("Smith"),
        );
        insert_literal(
            &store, "http://e/p2", given,
            oxigraph::model::Literal::new_simple_literal("Alice"),
        );
        insert_literal(
            &store, "http://e/p2", family,
            oxigraph::model::Literal::new_simple_literal("Smith"),
        );
        let derived = run(apply_prp_key, &store);
        let p1_p2 = Triple::new(iri("http://e/p1"), OWL_SAME_AS.into_owned(), iri("http://e/p2"));
        let p2_p1 = Triple::new(iri("http://e/p2"), OWL_SAME_AS.into_owned(), iri("http://e/p1"));
        assert!(
            derived.contains(&p1_p2) || derived.contains(&p2_p1),
            "expected p1 ≡ p2 from shared (given, family) key; got {derived:?}",
        );
    }

    #[test]
    fn prp_key_skips_instances_with_unequal_key() {
        let store = Store::new().unwrap();
        let given = NamedNodeRef::new_unchecked("http://e/given");
        insert(&store, "http://e/Person", OWL_HAS_KEY, "http://e/key");
        insert_list(&store, "http://e/key", &["http://e/given"]);
        insert_typed(&store, "http://e/p1", "http://e/Person");
        insert_typed(&store, "http://e/p2", "http://e/Person");
        insert_literal(
            &store, "http://e/p1", given,
            oxigraph::model::Literal::new_simple_literal("Alice"),
        );
        insert_literal(
            &store, "http://e/p2", given,
            oxigraph::model::Literal::new_simple_literal("Bob"),
        );
        let derived = run(apply_prp_key, &store);
        assert!(derived.is_empty(), "expected no derivation; got {derived:?}");
    }

    #[test]
    fn eq_ref_emits_reflexive_for_every_term_position() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/s", p, "http://e/o");
        let derived = run(apply_eq_ref, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/s"), OWL_SAME_AS.into_owned(), iri("http://e/s")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/p"), OWL_SAME_AS.into_owned(), iri("http://e/p")),
        );
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/o"), OWL_SAME_AS.into_owned(), iri("http://e/o")),
        );
    }

    #[test]
    fn eq_rep_s_substitutes_subject() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/a", OWL_SAME_AS, "http://e/b");
        insert(&store, "http://e/a", p, "http://e/o");
        let derived = run(apply_eq_rep_s, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/b"), p.into_owned(), iri("http://e/o")),
        );
    }

    #[test]
    fn eq_rep_p_substitutes_predicate() {
        let store = Store::new().unwrap();
        let p1 = NamedNodeRef::new_unchecked("http://e/p1");
        insert(&store, "http://e/p1", OWL_SAME_AS, "http://e/p2");
        insert(&store, "http://e/s", p1, "http://e/o");
        let derived = run(apply_eq_rep_p, &store);
        assert_contains(
            &derived,
            &Triple::new(
                iri("http://e/s"),
                NamedNode::new_unchecked("http://e/p2"),
                iri("http://e/o"),
            ),
        );
    }

    #[test]
    fn eq_rep_o_substitutes_object() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert(&store, "http://e/a", OWL_SAME_AS, "http://e/b");
        insert(&store, "http://e/s", p, "http://e/a");
        let derived = run(apply_eq_rep_o, &store);
        assert_contains(
            &derived,
            &Triple::new(iri("http://e/s"), p.into_owned(), iri("http://e/b")),
        );
    }

    // ── 0.10.0 Phase E — datatype smoke tests ───────────────────────────────

    #[test]
    fn dt_type1_emits_axiom_for_every_xsd_datatype() {
        let store = Store::new().unwrap();
        let derived = run(apply_dt_type1, &store);
        assert_eq!(
            derived.len(),
            DT_TYPE1_DATATYPES.len(),
            "expected one quad per W3C-listed datatype",
        );
        // Sample two key entries: xsd:integer and rdf:XMLLiteral.
        let xsd_integer = Triple::new(
            iri("http://www.w3.org/2001/XMLSchema#integer"),
            RDF_TYPE.into_owned(),
            iri("http://www.w3.org/2000/01/rdf-schema#Datatype"),
        );
        let rdf_xml_literal = Triple::new(
            iri("http://www.w3.org/1999/02/22-rdf-syntax-ns#XMLLiteral"),
            RDF_TYPE.into_owned(),
            iri("http://www.w3.org/2000/01/rdf-schema#Datatype"),
        );
        assert!(derived.contains(&xsd_integer));
        assert!(derived.contains(&rdf_xml_literal));
    }

    #[test]
    fn dt_type2_picks_up_consumer_defined_datatype() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert_literal(
            &store,
            "http://e/s",
            p,
            oxigraph::model::Literal::new_typed_literal(
                "abc",
                NamedNode::new_unchecked("http://e/my-custom-datatype"),
            ),
        );
        let derived = run(apply_dt_type2, &store);
        let expected = Triple::new(
            iri("http://e/my-custom-datatype"),
            RDF_TYPE.into_owned(),
            iri("http://www.w3.org/2000/01/rdf-schema#Datatype"),
        );
        assert!(
            derived.contains(&expected),
            "expected custom datatype axiom; got {derived:?}",
        );
    }

    #[test]
    fn dt_eq_is_currently_a_no_op() {
        // Literals can't be subjects in Oxigraph 0.4's model, so dt-eq
        // returns empty. When Oxigraph supports Subject::Literal this test
        // should fail and the rule body should be filled in.
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert_literal(
            &store,
            "http://e/s1",
            p,
            oxigraph::model::Literal::new_typed_literal(
                "1",
                NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#integer"),
            ),
        );
        insert_literal(
            &store,
            "http://e/s2",
            p,
            oxigraph::model::Literal::new_typed_literal(
                "1",
                NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#int"),
            ),
        );
        let derived = run(apply_dt_eq, &store);
        assert!(derived.is_empty(), "dt-eq must currently be a no-op; got {derived:?}");
    }

    #[test]
    fn dt_diff_is_currently_a_no_op() {
        let store = Store::new().unwrap();
        let p = NamedNodeRef::new_unchecked("http://e/p");
        insert_literal(
            &store,
            "http://e/s1",
            p,
            oxigraph::model::Literal::new_typed_literal(
                "1",
                NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#integer"),
            ),
        );
        insert_literal(
            &store,
            "http://e/s2",
            p,
            oxigraph::model::Literal::new_typed_literal(
                "2",
                NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#integer"),
            ),
        );
        let derived = run(apply_dt_diff, &store);
        assert!(derived.is_empty(), "dt-diff must currently be a no-op; got {derived:?}");
    }
}
