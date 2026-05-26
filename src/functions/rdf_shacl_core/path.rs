//! SHACL property-path AST + evaluator.
//!
//! Per W3C SHACL §2.3 Property Paths, the AST covers:
//!
//! - **Predicate path** — a plain IRI in the `sh:path` slot.
//! - **Inverse** — `[ sh:inversePath :p ]`.
//! - **Sequence** — `( :p1 :p2 … )` (RDF list of paths).
//! - **Alternative** — `[ sh:alternativePath ( :p1 :p2 … ) ]`.
//! - **Zero-or-more** — `[ sh:zeroOrMorePath :p ]`.
//! - **One-or-more** — `[ sh:oneOrMorePath :p ]`.
//! - **Zero-or-one** — `[ sh:zeroOrOnePath :p ]`.
//!
//! `parse` reads the shapes graph (path metadata lives there, not in
//! the data graph). `evaluate` walks the data graph and returns the
//! set of terms reachable from the focus node along the path.

#![allow(dead_code)]

use crate::error::{Result, SparqlError};
use oxigraph::model::{
    GraphName, GraphNameRef, NamedNode, NamedNodeRef, Subject, SubjectRef, Term, TermRef,
};
use oxigraph::store::Store;
use std::collections::HashSet;

const RDF_FIRST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#first");
const RDF_REST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#rest");
const RDF_NIL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#nil");

const SH_INVERSE_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#inversePath");
const SH_ALTERNATIVE_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#alternativePath");
const SH_ZERO_OR_MORE_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#zeroOrMorePath");
const SH_ONE_OR_MORE_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#oneOrMorePath");
const SH_ZERO_OR_ONE_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#zeroOrOnePath");

#[derive(Debug, Clone)]
pub(crate) enum Path {
    Predicate(NamedNode),
    Inverse(Box<Path>),
    Sequence(Vec<Path>),
    Alternative(Vec<Path>),
    ZeroOrMore(Box<Path>),
    OneOrMore(Box<Path>),
    ZeroOrOne(Box<Path>),
}

impl Path {
    /// Parse the RDF representation of an `sh:path` value into the AST.
    ///
    /// - `Term::NamedNode(p)` → `Predicate(p)`.
    /// - Blank-node-headed structure → recurse on `sh:inversePath` /
    ///   `sh:alternativePath` / `sh:zeroOrMorePath` / etc., or on the
    ///   RDF-list shape for sequence paths.
    pub(crate) fn parse(
        store: &Store,
        path_node: &Term,
        graph: &GraphName,
    ) -> Result<Path> {
        // Predicate path: bare IRI.
        if let Term::NamedNode(n) = path_node {
            return Ok(Path::Predicate(n.clone()));
        }

        // Otherwise must be a blank node. Look up known SHACL predicates,
        // or recognise the RDF-list shape (rdf:first/rdf:rest at the node).
        let head: Subject = match path_node {
            Term::BlankNode(b) => Subject::BlankNode(b.clone()),
            Term::NamedNode(_) => unreachable!("handled above"),
            Term::Literal(_) | Term::Triple(_) => {
                return Err(SparqlError::InvalidArgument(format!(
                    "rdf_shacl_core_validate: sh:path must be an IRI or blank-node structure, got {path_node}"
                )));
            }
        };

        // Inverse path.
        if let Some(inner) = lookup_object(store, graph, &head, SH_INVERSE_PATH)? {
            return Ok(Path::Inverse(Box::new(Path::parse(store, &inner, graph)?)));
        }
        // Alternative path: object is an RDF list of paths.
        if let Some(list_head) = lookup_object(store, graph, &head, SH_ALTERNATIVE_PATH)? {
            let items = walk_list(store, graph, &list_head)?;
            let mut parts = Vec::with_capacity(items.len());
            for item in items {
                parts.push(Path::parse(store, &item, graph)?);
            }
            return Ok(Path::Alternative(parts));
        }
        // Cardinality modifiers.
        if let Some(inner) = lookup_object(store, graph, &head, SH_ZERO_OR_MORE_PATH)? {
            return Ok(Path::ZeroOrMore(Box::new(Path::parse(store, &inner, graph)?)));
        }
        if let Some(inner) = lookup_object(store, graph, &head, SH_ONE_OR_MORE_PATH)? {
            return Ok(Path::OneOrMore(Box::new(Path::parse(store, &inner, graph)?)));
        }
        if let Some(inner) = lookup_object(store, graph, &head, SH_ZERO_OR_ONE_PATH)? {
            return Ok(Path::ZeroOrOne(Box::new(Path::parse(store, &inner, graph)?)));
        }

        // No SHACL marker — assume RDF-list sequence path.
        let items = walk_list(store, graph, path_node)?;
        if items.is_empty() {
            return Err(SparqlError::InvalidArgument(format!(
                "rdf_shacl_core_validate: sh:path blank node {path_node} has no recognised SHACL marker or list structure"
            )));
        }
        let mut parts = Vec::with_capacity(items.len());
        for item in items {
            parts.push(Path::parse(store, &item, graph)?);
        }
        Ok(Path::Sequence(parts))
    }

    /// Walk the data graph from `focus` and return reachable values.
    /// Result is order-preserving; duplicates removed (SHACL values are
    /// a set, not a multiset).
    pub(crate) fn evaluate(
        &self,
        store: &Store,
        focus: &Subject,
        graph: &GraphName,
    ) -> Vec<Term> {
        let mut out = Vec::new();
        let mut seen: HashSet<Term> = HashSet::new();
        for t in self.eval_impl(store, focus, graph) {
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
        out
    }

    fn eval_impl(&self, store: &Store, focus: &Subject, graph: &GraphName) -> Vec<Term> {
        match self {
            Path::Predicate(p) => predicate_step(store, focus, p, graph),
            Path::Inverse(inner) => {
                // Inverse only inverts one predicate step. For inverse of
                // a single predicate, walk backwards. For nested inverse
                // of complex paths, the spec defines inverse as the
                // relational inverse; we support inverse(Predicate) and
                // inverse(Inverse(p)) (double-inverse cancels).
                match inner.as_ref() {
                    Path::Predicate(p) => inverse_predicate_step(store, focus, p, graph),
                    Path::Inverse(inner2) => inner2.eval_impl(store, focus, graph),
                    // For any other inner path, fall back to evaluating
                    // the inner path and using a generic inverse via
                    // exhaustive search would be expensive. The 0.11.0
                    // surface guarantees inverse(Predicate); document
                    // and skip otherwise.
                    _ => Vec::new(),
                }
            }
            Path::Sequence(parts) => {
                let mut frontier: Vec<Subject> = vec![focus.clone()];
                for (i, part) in parts.iter().enumerate() {
                    let mut next: Vec<Subject> = Vec::new();
                    let last = i == parts.len() - 1;
                    for f in &frontier {
                        let values = part.eval_impl(store, f, graph);
                        if last {
                            // Last hop — return the terms directly.
                            // We'll re-collect below.
                            for v in values {
                                next.extend(term_to_subject(&v));
                            }
                            // For the last hop, defer to outer logic:
                            // re-evaluate just for terms.
                        } else {
                            for v in values {
                                if let Some(s) = term_to_subject(&v) {
                                    next.push(s);
                                }
                            }
                        }
                    }
                    frontier = next;
                }
                // Re-run the last hop to collect terms (not subjects),
                // since the body above only kept subject-form. Simpler:
                // re-evaluate the last part from the second-to-last frontier.
                if parts.is_empty() {
                    return vec![term_from_subject(focus)];
                }
                let last = parts.last().unwrap();
                let mut prefix_frontier: Vec<Subject> = vec![focus.clone()];
                for part in &parts[..parts.len() - 1] {
                    let mut next: Vec<Subject> = Vec::new();
                    for f in &prefix_frontier {
                        for v in part.eval_impl(store, f, graph) {
                            if let Some(s) = term_to_subject(&v) {
                                next.push(s);
                            }
                        }
                    }
                    prefix_frontier = next;
                }
                let mut out = Vec::new();
                for f in &prefix_frontier {
                    out.extend(last.eval_impl(store, f, graph));
                }
                out
            }
            Path::Alternative(parts) => {
                let mut out = Vec::new();
                for p in parts {
                    out.extend(p.eval_impl(store, focus, graph));
                }
                out
            }
            Path::ZeroOrMore(inner) => {
                // Reflexive transitive closure.
                let mut out: Vec<Term> = vec![term_from_subject(focus)];
                let mut frontier: Vec<Subject> = vec![focus.clone()];
                let mut visited: HashSet<Subject> = HashSet::new();
                visited.insert(focus.clone());
                loop {
                    let mut next: Vec<Subject> = Vec::new();
                    for f in &frontier {
                        for v in inner.eval_impl(store, f, graph) {
                            if let Some(s) = term_to_subject(&v) {
                                if visited.insert(s.clone()) {
                                    next.push(s);
                                    out.push(v);
                                }
                            }
                        }
                    }
                    if next.is_empty() {
                        break;
                    }
                    frontier = next;
                }
                out
            }
            Path::OneOrMore(inner) => {
                // Transitive closure, no reflexive seed.
                let mut out: Vec<Term> = Vec::new();
                let mut visited: HashSet<Subject> = HashSet::new();
                let initial = inner.eval_impl(store, focus, graph);
                let mut frontier: Vec<Subject> = Vec::new();
                for v in initial {
                    if let Some(s) = term_to_subject(&v) {
                        if visited.insert(s.clone()) {
                            frontier.push(s);
                            out.push(v);
                        }
                    } else {
                        out.push(v);
                    }
                }
                loop {
                    let mut next: Vec<Subject> = Vec::new();
                    for f in &frontier {
                        for v in inner.eval_impl(store, f, graph) {
                            if let Some(s) = term_to_subject(&v) {
                                if visited.insert(s.clone()) {
                                    next.push(s);
                                    out.push(v);
                                }
                            }
                        }
                    }
                    if next.is_empty() {
                        break;
                    }
                    frontier = next;
                }
                out
            }
            Path::ZeroOrOne(inner) => {
                let mut out: Vec<Term> = vec![term_from_subject(focus)];
                out.extend(inner.eval_impl(store, focus, graph));
                out
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn graph_ref(g: &GraphName) -> GraphNameRef<'_> {
    match g {
        GraphName::DefaultGraph => GraphNameRef::DefaultGraph,
        GraphName::NamedNode(n) => GraphNameRef::NamedNode(n.as_ref()),
        GraphName::BlankNode(b) => GraphNameRef::BlankNode(b.as_ref()),
    }
}

fn subject_ref(s: &Subject) -> SubjectRef<'_> {
    match s {
        Subject::NamedNode(n) => SubjectRef::NamedNode(n.as_ref()),
        Subject::BlankNode(b) => SubjectRef::BlankNode(b.as_ref()),
        Subject::Triple(t) => SubjectRef::Triple(t),
    }
}

fn term_to_subject(t: &Term) -> Option<Subject> {
    match t {
        Term::NamedNode(n) => Some(Subject::NamedNode(n.clone())),
        Term::BlankNode(b) => Some(Subject::BlankNode(b.clone())),
        Term::Triple(t) => Some(Subject::Triple(t.clone())),
        Term::Literal(_) => None,
    }
}

fn term_from_subject(s: &Subject) -> Term {
    match s {
        Subject::NamedNode(n) => Term::NamedNode(n.clone()),
        Subject::BlankNode(b) => Term::BlankNode(b.clone()),
        Subject::Triple(t) => Term::Triple(t.clone()),
    }
}

fn predicate_step(
    store: &Store,
    focus: &Subject,
    predicate: &NamedNode,
    graph: &GraphName,
) -> Vec<Term> {
    let mut out = Vec::new();
    for q in store.quads_for_pattern(
        Some(subject_ref(focus)),
        Some(predicate.as_ref()),
        None,
        Some(graph_ref(graph)),
    ) {
        if let Ok(q) = q {
            out.push(q.object);
        }
    }
    out
}

fn inverse_predicate_step(
    store: &Store,
    focus: &Subject,
    predicate: &NamedNode,
    graph: &GraphName,
) -> Vec<Term> {
    // The focus is in the object slot; collect subjects. We need a
    // TermRef for the object slot.
    let focus_term: Term = term_from_subject(focus);
    let focus_ref: TermRef<'_> = match &focus_term {
        Term::NamedNode(n) => TermRef::NamedNode(n.as_ref()),
        Term::BlankNode(b) => TermRef::BlankNode(b.as_ref()),
        Term::Literal(l) => TermRef::Literal(l.as_ref()),
        Term::Triple(t) => TermRef::Triple(t),
    };
    let mut out = Vec::new();
    for q in store.quads_for_pattern(
        None,
        Some(predicate.as_ref()),
        Some(focus_ref),
        Some(graph_ref(graph)),
    ) {
        if let Ok(q) = q {
            out.push(term_from_subject(&q.subject));
        }
    }
    out
}

fn lookup_object(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<Term>> {
    for q in store.quads_for_pattern(
        Some(subject_ref(subject)),
        Some(predicate),
        None,
        Some(graph_ref(graph)),
    ) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        return Ok(Some(q.object));
    }
    Ok(None)
}

/// Walk an RDF list starting at `head` (a Term that is either rdf:nil
/// or a blank/named node with rdf:first / rdf:rest links). Returns the
/// list elements in order. Used for sequence paths and alternative
/// paths. Errors on cycles or malformed lists.
fn walk_list(store: &Store, graph: &GraphName, head: &Term) -> Result<Vec<Term>> {
    if let Term::NamedNode(n) = head {
        if n.as_ref() == RDF_NIL {
            return Ok(Vec::new());
        }
    }
    let mut out = Vec::new();
    let mut seen: HashSet<Subject> = HashSet::new();
    let mut cursor_term = head.clone();
    loop {
        let cursor: Subject = match cursor_term {
            Term::NamedNode(n) if n.as_ref() == RDF_NIL => return Ok(out),
            Term::NamedNode(n) => Subject::NamedNode(n),
            Term::BlankNode(b) => Subject::BlankNode(b),
            Term::Triple(t) => Subject::Triple(t),
            Term::Literal(_) => {
                return Err(SparqlError::InvalidArgument(
                    "rdf_shacl_core_validate: sh:path list contains a literal node".to_string(),
                ));
            }
        };
        if !seen.insert(cursor.clone()) {
            return Err(SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:path list has a cycle".to_string(),
            ));
        }
        let first = lookup_object(store, graph, &cursor, RDF_FIRST)?.ok_or_else(|| {
            SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:path list node missing rdf:first".to_string(),
            )
        })?;
        out.push(first);
        let rest = lookup_object(store, graph, &cursor, RDF_REST)?.ok_or_else(|| {
            SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:path list node missing rdf:rest".to_string(),
            )
        })?;
        cursor_term = rest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{BlankNode, Literal, Quad};

    fn iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    fn insert(store: &Store, s: Subject, p: NamedNode, o: Term, g: &GraphName) {
        store
            .insert(&Quad::new(s, p, o, g.clone()))
            .unwrap();
    }

    #[test]
    fn predicate_path_walks_one_hop() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        let path = Path::Predicate(iri("http://e/p"));
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert_eq!(result, vec![Term::NamedNode(iri("http://e/b"))]);
    }

    #[test]
    fn inverse_predicate_walks_backward() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        let path = Path::Inverse(Box::new(Path::Predicate(iri("http://e/p"))));
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/b")), &g);
        assert_eq!(result, vec![Term::NamedNode(iri("http://e/a"))]);
    }

    #[test]
    fn sequence_path_chains_two_hops() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p1"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        insert(
            &store,
            Subject::NamedNode(iri("http://e/b")),
            iri("http://e/p2"),
            Term::NamedNode(iri("http://e/c")),
            &g,
        );
        let path = Path::Sequence(vec![
            Path::Predicate(iri("http://e/p1")),
            Path::Predicate(iri("http://e/p2")),
        ]);
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert_eq!(result, vec![Term::NamedNode(iri("http://e/c"))]);
    }

    #[test]
    fn alternative_path_unions_branches() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p1"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p2"),
            Term::NamedNode(iri("http://e/c")),
            &g,
        );
        let path = Path::Alternative(vec![
            Path::Predicate(iri("http://e/p1")),
            Path::Predicate(iri("http://e/p2")),
        ]);
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert_eq!(result.len(), 2);
        assert!(result.contains(&Term::NamedNode(iri("http://e/b"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/c"))));
    }

    #[test]
    fn zero_or_more_includes_self_and_descendants() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        insert(
            &store,
            Subject::NamedNode(iri("http://e/b")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/c")),
            &g,
        );
        let path = Path::ZeroOrMore(Box::new(Path::Predicate(iri("http://e/p"))));
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert!(result.contains(&Term::NamedNode(iri("http://e/a"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/b"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/c"))));
    }

    #[test]
    fn one_or_more_excludes_self() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        insert(
            &store,
            Subject::NamedNode(iri("http://e/b")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/c")),
            &g,
        );
        let path = Path::OneOrMore(Box::new(Path::Predicate(iri("http://e/p"))));
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert!(!result.contains(&Term::NamedNode(iri("http://e/a"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/b"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/c"))));
    }

    #[test]
    fn zero_or_one_returns_self_and_immediate() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        insert(
            &store,
            Subject::NamedNode(iri("http://e/a")),
            iri("http://e/p"),
            Term::NamedNode(iri("http://e/b")),
            &g,
        );
        let path = Path::ZeroOrOne(Box::new(Path::Predicate(iri("http://e/p"))));
        let result = path.evaluate(&store, &Subject::NamedNode(iri("http://e/a")), &g);
        assert!(result.contains(&Term::NamedNode(iri("http://e/a"))));
        assert!(result.contains(&Term::NamedNode(iri("http://e/b"))));
    }

    #[test]
    fn parse_predicate_iri() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        let parsed = Path::parse(
            &store,
            &Term::NamedNode(iri("http://e/p")),
            &g,
        )
        .unwrap();
        assert!(matches!(parsed, Path::Predicate(_)));
    }

    #[test]
    fn parse_inverse_path() {
        let store = Store::new().unwrap();
        let g_shapes = GraphName::NamedNode(iri("urn:g:shapes"));
        let head = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(head.clone()),
            SH_INVERSE_PATH.into_owned(),
            Term::NamedNode(iri("http://e/p")),
            &g_shapes,
        );
        let parsed = Path::parse(&store, &Term::BlankNode(head), &g_shapes).unwrap();
        match parsed {
            Path::Inverse(inner) => match inner.as_ref() {
                Path::Predicate(p) => assert_eq!(p.as_str(), "http://e/p"),
                _ => panic!("expected Predicate inside Inverse"),
            },
            _ => panic!("expected Inverse"),
        }
    }

    #[test]
    fn parse_sequence_path() {
        let store = Store::new().unwrap();
        let g_shapes = GraphName::NamedNode(iri("urn:g:shapes-seq"));
        // RDF list: ( :p1 :p2 )
        let n1 = BlankNode::default();
        let n2 = BlankNode::default();
        insert(
            &store,
            Subject::BlankNode(n1.clone()),
            RDF_FIRST.into_owned(),
            Term::NamedNode(iri("http://e/p1")),
            &g_shapes,
        );
        insert(
            &store,
            Subject::BlankNode(n1.clone()),
            RDF_REST.into_owned(),
            Term::BlankNode(n2.clone()),
            &g_shapes,
        );
        insert(
            &store,
            Subject::BlankNode(n2.clone()),
            RDF_FIRST.into_owned(),
            Term::NamedNode(iri("http://e/p2")),
            &g_shapes,
        );
        insert(
            &store,
            Subject::BlankNode(n2.clone()),
            RDF_REST.into_owned(),
            Term::NamedNode(RDF_NIL.into_owned()),
            &g_shapes,
        );
        let parsed = Path::parse(&store, &Term::BlankNode(n1), &g_shapes).unwrap();
        match parsed {
            Path::Sequence(parts) => assert_eq!(parts.len(), 2),
            _ => panic!("expected Sequence"),
        }
    }

    #[test]
    fn literal_in_path_rejects() {
        let store = Store::new().unwrap();
        let g = GraphName::DefaultGraph;
        let err = Path::parse(
            &store,
            &Term::Literal(Literal::new_simple_literal("nope")),
            &g,
        );
        assert!(err.is_err());
    }
}
