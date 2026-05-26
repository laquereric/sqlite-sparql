//! RDF list traversal helper for OWL 2 RL rules whose premises reference
//! lists (`(c1 c2 … cn)`-style class-expression / hasKey / property-chain
//! constructs).
//!
//! An RDF list `(t1 t2 … tn)` is encoded as a chain of blank nodes (or
//! IRIs) linked by `rdf:first` / `rdf:rest`, terminating at `rdf:nil`:
//!
//! ```text
//! head ── rdf:first → t1
//!      ── rdf:rest  → mid1 ── rdf:first → t2
//!                          ── rdf:rest  → mid2 ── … → rdf:nil
//! ```
//!
//! Used by `cls-int1`, `cls-int2`, `cls-uni`, `cls-oo`, `cls-svf*`,
//! `cls-avf`, `cls-hv*`, `cls-maxqc*`, `prp-spo2` (property chain),
//! `prp-key`, `scm-int`, `scm-uni` in PLAN_0.10.0.

// Phase A lands the helper; Phase B–D wire it into the rule functions.
#![allow(dead_code)]

use crate::error::{Result, SparqlError};
use oxigraph::model::{GraphName, GraphNameRef, NamedNodeRef, Subject, Term, TermRef};
use oxigraph::store::Store;
use std::collections::HashSet;

pub(crate) const RDF_FIRST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#first");
pub(crate) const RDF_REST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#rest");
pub(crate) const RDF_NIL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#nil");

/// Walk an RDF list starting at `head`, returning the sequence of
/// `rdf:first` values in list order.
///
/// - `Some(vec![])` — `head` is `rdf:nil`.
/// - `Some(vec![t1, …, tn])` — well-formed list of length n.
/// - `None` — malformed (missing `rdf:first`/`rdf:rest`, multiple firsts
///   at one node, non-node rest, or a cycle).
///
/// `graphs` is the list of graphs to query for premises (typically
/// `[asserted, inferred]` from `graphs_to_query`). The list nodes
/// may be split across both graphs and walking still works.
pub(crate) fn walk_list(
    store: &Store,
    head: &Subject,
    graphs: &[&GraphName],
) -> Result<Option<Vec<Term>>> {
    let nil_term = TermRef::NamedNode(RDF_NIL);

    if subject_is_nil(head) {
        return Ok(Some(Vec::new()));
    }

    let mut out = Vec::new();
    let mut seen: HashSet<Subject> = HashSet::new();
    let mut cursor: Subject = head.clone();

    loop {
        if !seen.insert(cursor.clone()) {
            // Cycle.
            return Ok(None);
        }

        // Exactly one rdf:first per node; collect across graphs and reject
        // ambiguity. (A list with two different rdf:first values at the
        // same node is malformed.)
        let mut first_value: Option<Term> = None;
        for graph in graphs {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(
                Some(subject_to_ref(&cursor)),
                Some(RDF_FIRST),
                None,
                Some(g_ref),
            ) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                match &first_value {
                    None => first_value = Some(q.object),
                    Some(prev) if prev == &q.object => {}
                    Some(_) => return Ok(None), // ambiguous
                }
            }
        }
        let Some(first) = first_value else {
            return Ok(None); // missing rdf:first
        };
        out.push(first);

        // Same shape for rdf:rest.
        let mut rest_value: Option<Term> = None;
        for graph in graphs {
            let g_ref = graph_to_ref(graph);
            for q in store.quads_for_pattern(
                Some(subject_to_ref(&cursor)),
                Some(RDF_REST),
                None,
                Some(g_ref),
            ) {
                let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                match &rest_value {
                    None => rest_value = Some(q.object),
                    Some(prev) if prev == &q.object => {}
                    Some(_) => return Ok(None),
                }
            }
        }
        let Some(rest) = rest_value else {
            return Ok(None); // missing rdf:rest
        };

        // rdf:nil terminator?
        if rest.as_ref() == nil_term {
            return Ok(Some(out));
        }

        // Otherwise rest must itself be a node usable as a subject.
        cursor = match rest {
            Term::NamedNode(n) => Subject::NamedNode(n),
            Term::BlankNode(b) => Subject::BlankNode(b),
            // A literal rest is malformed.
            Term::Literal(_) => return Ok(None),
            // A quoted-triple rest is malformed for RDF lists.
            Term::Triple(_) => return Ok(None),
        };
    }
}

fn subject_is_nil(s: &Subject) -> bool {
    matches!(s, Subject::NamedNode(n) if n.as_ref() == RDF_NIL)
}

fn graph_to_ref(g: &GraphName) -> GraphNameRef<'_> {
    match g {
        GraphName::DefaultGraph => GraphNameRef::DefaultGraph,
        GraphName::NamedNode(n) => GraphNameRef::NamedNode(n.as_ref()),
        GraphName::BlankNode(b) => GraphNameRef::BlankNode(b.as_ref()),
    }
}

fn subject_to_ref(s: &Subject) -> oxigraph::model::SubjectRef<'_> {
    match s {
        Subject::NamedNode(n) => oxigraph::model::SubjectRef::NamedNode(n.as_ref()),
        Subject::BlankNode(b) => oxigraph::model::SubjectRef::BlankNode(b.as_ref()),
        Subject::Triple(t) => oxigraph::model::SubjectRef::Triple(t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{BlankNode, NamedNode, Quad};

    fn iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    fn insert(store: &Store, s: Subject, p: NamedNodeRef<'_>, o: Term) {
        store
            .insert(&Quad::new(s, p.into_owned(), o, GraphName::DefaultGraph))
            .unwrap();
    }

    fn walk(store: &Store, head: Subject) -> Option<Vec<Term>> {
        walk_list(store, &head, &[&GraphName::DefaultGraph]).unwrap()
    }

    #[test]
    fn nil_is_empty_list() {
        let store = Store::new().unwrap();
        let result = walk(&store, Subject::NamedNode(RDF_NIL.into_owned()));
        assert_eq!(result, Some(Vec::new()));
    }

    #[test]
    fn single_element_list() {
        let store = Store::new().unwrap();
        let head = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_FIRST,
            Term::NamedNode(iri("http://e/a")),
        );
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_REST,
            Term::NamedNode(RDF_NIL.into_owned()),
        );
        let result = walk(&store, Subject::BlankNode(head));
        assert_eq!(result, Some(vec![Term::NamedNode(iri("http://e/a"))]));
    }

    #[test]
    fn three_element_list() {
        let store = Store::new().unwrap();
        let n1 = BlankNode::default();
        let n2 = BlankNode::default();
        let n3 = BlankNode::default();
        insert(&store, Subject::BlankNode(n1.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/a")));
        insert(&store, Subject::BlankNode(n1.clone()), RDF_REST, Term::BlankNode(n2.clone()));
        insert(&store, Subject::BlankNode(n2.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/b")));
        insert(&store, Subject::BlankNode(n2.clone()), RDF_REST, Term::BlankNode(n3.clone()));
        insert(&store, Subject::BlankNode(n3.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/c")));
        insert(&store, Subject::BlankNode(n3.clone()), RDF_REST, Term::NamedNode(RDF_NIL.into_owned()));
        let result = walk(&store, Subject::BlankNode(n1));
        assert_eq!(
            result,
            Some(vec![
                Term::NamedNode(iri("http://e/a")),
                Term::NamedNode(iri("http://e/b")),
                Term::NamedNode(iri("http://e/c")),
            ])
        );
    }

    #[test]
    fn cyclic_list_returns_none() {
        let store = Store::new().unwrap();
        let n1 = BlankNode::default();
        let n2 = BlankNode::default();
        insert(&store, Subject::BlankNode(n1.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/a")));
        insert(&store, Subject::BlankNode(n1.clone()), RDF_REST, Term::BlankNode(n2.clone()));
        insert(&store, Subject::BlankNode(n2.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/b")));
        insert(&store, Subject::BlankNode(n2.clone()), RDF_REST, Term::BlankNode(n1.clone())); // back to n1
        let result = walk(&store, Subject::BlankNode(n1));
        assert_eq!(result, None);
    }

    #[test]
    fn missing_first_returns_none() {
        let store = Store::new().unwrap();
        let head = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_REST,
            Term::NamedNode(RDF_NIL.into_owned()),
        );
        let result = walk(&store, Subject::BlankNode(head));
        assert_eq!(result, None);
    }

    #[test]
    fn missing_rest_returns_none() {
        let store = Store::new().unwrap();
        let head = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_FIRST,
            Term::NamedNode(iri("http://e/a")),
        );
        let result = walk(&store, Subject::BlankNode(head));
        assert_eq!(result, None);
    }

    #[test]
    fn literal_rest_is_malformed() {
        let store = Store::new().unwrap();
        let head = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_FIRST,
            Term::NamedNode(iri("http://e/a")),
        );
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            RDF_REST,
            Term::Literal(oxigraph::model::Literal::new_simple_literal("not-a-node")),
        );
        let result = walk(&store, Subject::BlankNode(head));
        assert_eq!(result, None);
    }

    #[test]
    fn ambiguous_first_is_malformed() {
        let store = Store::new().unwrap();
        let head = BlankNode::default();
        insert(&store, Subject::BlankNode(head.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/a")));
        insert(&store, Subject::BlankNode(head.clone()), RDF_FIRST, Term::NamedNode(iri("http://e/b")));
        insert(&store, Subject::BlankNode(head.clone()), RDF_REST, Term::NamedNode(RDF_NIL.into_owned()));
        let result = walk(&store, Subject::BlankNode(head));
        assert_eq!(result, None);
    }
}
