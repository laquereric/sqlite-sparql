/// Thread-local Oxigraph in-memory store.
///
/// SQLite extensions run in the same process as the host application.  We keep
/// one Oxigraph `MemoryStore` per thread (matching SQLite's connection-per-thread
/// model) so that concurrent reads from different connections do not race.
///
/// The store is lazily initialised on first use and lives for the lifetime of
/// the thread.  A `CALL rdf_clear()` SQL function resets it.
use oxigraph::model::*;
use oxigraph::store::Store;
use std::cell::RefCell;

thread_local! {
    /// The per-thread Oxigraph store.
    static STORE: RefCell<Store> = RefCell::new(
        Store::new().expect("failed to create Oxigraph in-memory store")
    );
}

/// Execute a closure with a shared reference to the thread-local store.
pub fn with_store<F, T>(f: F) -> T
where
    F: FnOnce(&Store) -> T,
{
    STORE.with(|s| f(&s.borrow()))
}

/// Execute a closure with a mutable reference to the thread-local store.
pub fn with_store_mut<F, T>(f: F) -> T
where
    F: FnOnce(&Store) -> T,
{
    STORE.with(|s| f(&s.borrow()))
}

/// Reset (clear) the thread-local store.
pub fn clear_store() {
    STORE.with(|s| {
        *s.borrow_mut() = Store::new().expect("failed to create new Oxigraph store");
    });
}

/// Insert a single triple (subject, predicate, object) into the default graph.
pub fn insert_triple(s: &str, p: &str, o: &str) -> crate::error::Result<()> {
    let subject = parse_named_or_blank(s)?;
    let predicate = NamedNode::new(p)
        .map_err(|e| crate::error::SparqlError::InvalidArgument(format!("predicate IRI: {e}")))?;
    let object = parse_term(o)?;

    let quad = Quad::new(subject, predicate, object, GraphName::DefaultGraph);
    STORE.with(|store| {
        store
            .borrow()
            .insert(&quad)
            .map(|_| ())
            .map_err(|e| crate::error::SparqlError::StoreError(e.to_string()))
    })
}

/// Delete a single triple from the default graph.
pub fn delete_triple(s: &str, p: &str, o: &str) -> crate::error::Result<()> {
    let subject = parse_named_or_blank(s)?;
    let predicate = NamedNode::new(p)
        .map_err(|e| crate::error::SparqlError::InvalidArgument(format!("predicate IRI: {e}")))?;
    let object = parse_term(o)?;

    let quad = Quad::new(subject, predicate, object, GraphName::DefaultGraph);
    STORE.with(|store| {
        store
            .borrow()
            .remove(&quad)
            .map(|_| ())
            .map_err(|e| crate::error::SparqlError::StoreError(e.to_string()))
    })
}

/// Count the number of quads in the default graph.
pub fn triple_count() -> usize {
    STORE.with(|store| store.borrow().len().unwrap_or(0))
}

// ── Parsing helpers ──────────────────────────────────────────────────────────

/// Parse a string as a NamedNode (IRI) or BlankNode.
///
/// Blank nodes must be prefixed with `_:` (e.g. `_:b0`).
fn parse_named_or_blank(s: &str) -> crate::error::Result<Subject> {
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
fn parse_term(s: &str) -> crate::error::Result<Term> {
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
    // Strip leading quote
    let rest = s.strip_prefix('"').ok_or_else(|| {
        crate::error::SparqlError::InvalidArgument(format!("expected opening quote in: {s}"))
    })?;

    // Find the closing quote (last `"` before optional suffix)
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
