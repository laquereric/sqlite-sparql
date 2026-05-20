//! Process-wide Oxigraph in-memory store.
//!
//! One [`Store`] for the lifetime of the process, lazily initialised on
//! first use and shared by every SQLite connection on every thread.
//!
//! Oxigraph 0.4's in-memory [`Store`] is internally concurrent — its
//! `insert`, `remove`, `query`, and `clear` methods all take `&self` and
//! it composes from `Arc`-shared indexes plus `DashMap` / `RwLock`
//! interior synchronisation. Wrapping it in a `Mutex` or `RwLock` here
//! would only add contention without buying any correctness.
//!
//! This is a deliberate departure from the per-thread store the
//! extension shipped with in 0.1.0; see
//! `docs/reviews/REVIEW_0.1.0.md` and `docs/plans/PLAN_0.2.0.md` for
//! the reasoning.

use oxigraph::model::*;
use oxigraph::store::Store;
use std::sync::OnceLock;

static STORE: OnceLock<Store> = OnceLock::new();

fn store() -> &'static Store {
    STORE.get_or_init(|| {
        Store::new().expect("failed to create Oxigraph in-memory store")
    })
}

/// Execute a closure with a shared reference to the process-wide store.
pub fn with_store<F, T>(f: F) -> T
where
    F: FnOnce(&Store) -> T,
{
    f(store())
}

/// Remove every quad from the store. The store instance itself stays
/// alive; future inserts go to the same `Store` object.
pub fn clear_store() -> crate::error::Result<()> {
    store()
        .clear()
        .map_err(|e| crate::error::SparqlError::StoreError(e.to_string()))
}

/// Insert a single triple into the default graph (3-arg form, 0.1.0 surface).
pub fn insert_triple(s: &str, p: &str, o: &str) -> crate::error::Result<()> {
    insert_triple_in_graph(s, p, o, None)
}

/// Delete a single triple from the default graph (3-arg form, 0.1.0 surface).
pub fn delete_triple(s: &str, p: &str, o: &str) -> crate::error::Result<()> {
    delete_triple_in_graph(s, p, o, None)
}

/// Insert into a specific graph. `graph = None` → default graph; `Some(iri)`
/// → the named graph with that IRI. Blank-node graphs are rejected (Oxigraph
/// supports them but we keep the boundary narrow — see PLAN_0.3.0.md).
pub fn insert_triple_in_graph(
    s: &str,
    p: &str,
    o: &str,
    graph: Option<&str>,
) -> crate::error::Result<()> {
    let quad = build_quad(s, p, o, graph)?;
    store()
        .insert(&quad)
        .map(|_| ())
        .map_err(|e| crate::error::SparqlError::StoreError(e.to_string()))
}

/// Delete from a specific graph. Same `graph` semantics as `insert_triple_in_graph`.
pub fn delete_triple_in_graph(
    s: &str,
    p: &str,
    o: &str,
    graph: Option<&str>,
) -> crate::error::Result<()> {
    let quad = build_quad(s, p, o, graph)?;
    store()
        .remove(&quad)
        .map(|_| ())
        .map_err(|e| crate::error::SparqlError::StoreError(e.to_string()))
}

/// Count the number of quads in the default graph (0.1.0 surface).
pub fn triple_count() -> usize {
    triple_count_in_graph(None)
}

/// Count the number of quads in a specific graph. `graph = None` counts the
/// default graph only.
pub fn triple_count_in_graph(graph: Option<&str>) -> usize {
    use oxigraph::model::GraphNameRef;

    // Hold the NamedNode owned so the GraphNameRef can borrow from it.
    let named: Option<NamedNode> = match graph {
        None => None,
        Some(iri) => match NamedNode::new(iri) {
            Ok(n) => Some(n),
            Err(_) => return 0,
        },
    };
    let graph_ref = match &named {
        None => GraphNameRef::DefaultGraph,
        Some(n) => GraphNameRef::NamedNode(n.as_ref()),
    };

    let mut count = 0usize;
    for quad in store().quads_for_pattern(None, None, None, Some(graph_ref)) {
        if quad.is_ok() {
            count += 1;
        }
    }
    count
}

/// Count quads across every graph, including the default graph.
pub fn triple_count_all() -> usize {
    store().len().unwrap_or(0)
}

// ── Internal helpers ─────────────────────────────────────────────────────────

pub(crate) fn build_quad(
    s: &str,
    p: &str,
    o: &str,
    graph: Option<&str>,
) -> crate::error::Result<Quad> {
    let subject = parse_named_or_blank(s)?;
    let predicate = NamedNode::new(p)
        .map_err(|e| crate::error::SparqlError::InvalidArgument(format!("predicate IRI: {e}")))?;
    let object = parse_term(o)?;
    let graph_name = parse_graph_name(graph)?;
    Ok(Quad::new(subject, predicate, object, graph_name))
}

pub(crate) fn parse_graph_name(graph: Option<&str>) -> crate::error::Result<GraphName> {
    match graph {
        None => Ok(GraphName::DefaultGraph),
        Some(iri) if iri.is_empty() => Err(crate::error::SparqlError::InvalidArgument(
            "graph IRI may not be the empty string; pass NULL for the default graph"
                .to_string(),
        )),
        Some(iri) if iri.starts_with("_:") => Err(crate::error::SparqlError::InvalidArgument(
            "blank-node graphs are not supported in sqlite-sparql 0.3.x".to_string(),
        )),
        Some(iri) => Ok(GraphName::NamedNode(NamedNode::new(iri).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("graph IRI: {e}"))
        })?)),
    }
}

// ── Parsing helpers ──────────────────────────────────────────────────────────

/// Parse a string as a NamedNode (IRI) or BlankNode.
///
/// Blank nodes must be prefixed with `_:` (e.g. `_:b0`).
pub(crate) fn parse_named_or_blank(s: &str) -> crate::error::Result<Subject> {
    if let Some(id) = s.strip_prefix("_:") {
        Ok(Subject::BlankNode(BlankNode::new(id).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("blank node id: {e}"))
        })?))
    } else {
        Ok(Subject::NamedNode(NamedNode::new(s).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("subject IRI: {e}"))
        })?))
    }
}

/// Parse an RDF term (object position).
///
/// Rules:
/// - `_:xxx`  → BlankNode
/// - `"text"` or `"text"^^<iri>` or `"text"@lang` → Literal
/// - anything else → NamedNode (IRI)
pub(crate) fn parse_term(s: &str) -> crate::error::Result<Term> {
    if let Some(id) = s.strip_prefix("_:") {
        return Ok(Term::BlankNode(BlankNode::new(id).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("blank node id: {e}"))
        })?));
    }

    if s.starts_with('"') {
        return parse_literal(s);
    }

    Ok(Term::NamedNode(NamedNode::new(s).map_err(|e| {
        crate::error::SparqlError::InvalidArgument(format!("object IRI: {e}"))
    })?))
}

/// Parse a quoted literal string in N-Triples / Turtle syntax.
///
/// Supported forms:
/// - `"plain text"`
/// - `"text"@en`
/// - `"42"^^<http://www.w3.org/2001/XMLSchema#integer>`
fn parse_literal(s: &str) -> crate::error::Result<Term> {
    let rest = s.strip_prefix('"').ok_or_else(|| {
        crate::error::SparqlError::InvalidArgument(format!("expected opening quote in: {s}"))
    })?;

    let close = rest.rfind('"').ok_or_else(|| {
        crate::error::SparqlError::InvalidArgument(format!("no closing quote in: {s}"))
    })?;

    let value = &rest[..close];
    let suffix = &rest[close + 1..];

    let literal = if let Some(lang) = suffix.strip_prefix('@') {
        Literal::new_language_tagged_literal(value, lang).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("language tag: {e}"))
        })?
    } else if let Some(dt) = suffix.strip_prefix("^^") {
        let dt_iri = dt.trim_start_matches('<').trim_end_matches('>');
        let dt_node = NamedNode::new(dt_iri).map_err(|e| {
            crate::error::SparqlError::InvalidArgument(format!("datatype IRI: {e}"))
        })?;
        Literal::new_typed_literal(value, dt_node)
    } else {
        Literal::new_simple_literal(value)
    };

    Ok(Term::Literal(literal))
}
