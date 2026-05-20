/// Scalar functions for managing RDF triples in the thread-local Oxigraph store.
///
/// | SQL Function                        | Description                                      |
/// |-------------------------------------|--------------------------------------------------|
/// | `rdf_insert(s, p, o)`               | Insert a triple into the default graph           |
/// | `rdf_delete(s, p, o)`               | Delete a triple from the default graph           |
/// | `rdf_clear()`                       | Remove all triples and reset the store           |
/// | `rdf_count()`                       | Return the number of triples in the store        |
/// | `rdf_load_turtle(turtle_text)`      | Bulk-load triples from a Turtle-format string    |
/// | `rdf_load_turtle_to_graph(b, g)`    | …into the named graph `g` (`NULL` → default)     |
/// | `rdf_load_ntriples(ntriples_text)`  | Bulk-load triples from an N-Triples string       |
/// | `rdf_load_ntriples_to_graph(b, g)`  | …into the named graph `g` (`NULL` → default)     |
/// | `rdf_load_rdfxml(xml_text)`         | Bulk-load triples from an RDF/XML string         |
/// | `rdf_load_rdfxml_to_graph(b, g)`    | …into the named graph `g` (`NULL` → default)     |
/// | `rdf_dump_ntriples()`               | Dump all triples as an N-Triples string          |
/// | `rdf_term_type(term)`               | Return "iri", "blank", or "literal"              |
/// | `rdf_term_value(term)`              | Extract the string value from an N-Triples term  |
use oxigraph::io::{RdfFormat, RdfParser};
use sqlite_loadable::{api, define_scalar_function, prelude::*, FunctionFlags};

use crate::error::SparqlError;
use crate::store::{
    clear_store, delete_triple, delete_triple_in_graph, insert_triple,
    insert_triple_in_graph, parse_graph_name, triple_count, triple_count_all,
    triple_count_in_graph, with_store,
};
use sqlite_loadable::api::ValueType;

// ── rdf_insert ────────────────────────────────────────────────────────────────

pub fn rdf_insert_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let s = api::value_text(values.get(0).expect("subject"))?;
    let p = api::value_text(values.get(1).expect("predicate"))?;
    let o = api::value_text(values.get(2).expect("object"))?;

    insert_triple(s, p, o).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, 1);
    Ok(())
}

/// 4-arg form: `rdf_insert(s, p, o, graph)`. `graph = NULL` → default graph.
pub fn rdf_insert_g_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let s = api::value_text(values.get(0).expect("subject"))?;
    let p = api::value_text(values.get(1).expect("predicate"))?;
    let o = api::value_text(values.get(2).expect("object"))?;
    let g = graph_arg(values.get(3).expect("graph"))?;

    insert_triple_in_graph(s, p, o, g).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, 1);
    Ok(())
}

// ── rdf_delete ────────────────────────────────────────────────────────────────

pub fn rdf_delete_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let s = api::value_text(values.get(0).expect("subject"))?;
    let p = api::value_text(values.get(1).expect("predicate"))?;
    let o = api::value_text(values.get(2).expect("object"))?;

    delete_triple(s, p, o).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, 1);
    Ok(())
}

/// 4-arg form: `rdf_delete(s, p, o, graph)`. `graph = NULL` → default graph.
pub fn rdf_delete_g_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let s = api::value_text(values.get(0).expect("subject"))?;
    let p = api::value_text(values.get(1).expect("predicate"))?;
    let o = api::value_text(values.get(2).expect("object"))?;
    let g = graph_arg(values.get(3).expect("graph"))?;

    delete_triple_in_graph(s, p, o, g).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, 1);
    Ok(())
}

fn graph_arg<'a>(v: &'a *mut sqlite3_value) -> sqlite_loadable::Result<Option<&'a str>> {
    if api::value_type(v) == ValueType::Null {
        Ok(None)
    } else {
        let s = api::value_text(v)?;
        Ok(Some(s))
    }
}

// ── rdf_clear ─────────────────────────────────────────────────────────────────

pub fn rdf_clear_fn(
    context: *mut sqlite3_context,
    _values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    clear_store().map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, 1);
    Ok(())
}

// ── rdf_count ─────────────────────────────────────────────────────────────────

/// `rdf_count()` — count in the default graph (0.1.0 surface, unchanged).
pub fn rdf_count_fn(
    context: *mut sqlite3_context,
    _values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let count = triple_count();
    api::result_int(context, count as i32);
    Ok(())
}

/// `rdf_count(graph)` — count in a specific graph. `NULL` means the default
/// graph (same as zero-arg `rdf_count()`).
pub fn rdf_count_g_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let g = graph_arg(values.get(0).expect("graph"))?;
    let count = triple_count_in_graph(g);
    api::result_int(context, count as i32);
    Ok(())
}

/// `rdf_count_all()` — count across every graph including the default.
pub fn rdf_count_all_fn(
    context: *mut sqlite3_context,
    _values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let count = triple_count_all();
    api::result_int(context, count as i32);
    Ok(())
}

// ── rdf_load_turtle ───────────────────────────────────────────────────────────

pub fn rdf_load_turtle_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let turtle = api::value_text(values.get(0).expect("Turtle text"))?;
    let count = load_rdf(turtle, RdfFormat::Turtle, None)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

/// 2-arg form: `rdf_load_turtle_to_graph(body, graph)`. `graph = NULL` →
/// default graph (identical to the 1-arg form).
pub fn rdf_load_turtle_to_graph_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let turtle = api::value_text(values.get(0).expect("Turtle text"))?;
    let g = graph_arg(values.get(1).expect("graph"))?;
    let count = load_rdf(turtle, RdfFormat::Turtle, g)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

// ── rdf_load_ntriples ─────────────────────────────────────────────────────────

pub fn rdf_load_ntriples_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let nt = api::value_text(values.get(0).expect("N-Triples text"))?;
    let count = load_rdf(nt, RdfFormat::NTriples, None)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

/// 2-arg form: `rdf_load_ntriples_to_graph(body, graph)`. `graph = NULL` →
/// default graph (identical to the 1-arg form).
pub fn rdf_load_ntriples_to_graph_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let nt = api::value_text(values.get(0).expect("N-Triples text"))?;
    let g = graph_arg(values.get(1).expect("graph"))?;
    let count = load_rdf(nt, RdfFormat::NTriples, g)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

// ── rdf_load_rdfxml ───────────────────────────────────────────────────────────

pub fn rdf_load_rdfxml_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let xml = api::value_text(values.get(0).expect("RDF/XML text"))?;
    let count = load_rdf(xml, RdfFormat::RdfXml, None)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

/// 2-arg form: `rdf_load_rdfxml_to_graph(body, graph)`. `graph = NULL` →
/// default graph (identical to the 1-arg form).
pub fn rdf_load_rdfxml_to_graph_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let xml = api::value_text(values.get(0).expect("RDF/XML text"))?;
    let g = graph_arg(values.get(1).expect("graph"))?;
    let count = load_rdf(xml, RdfFormat::RdfXml, g)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

// ── rdf_dump_ntriples ─────────────────────────────────────────────────────────

pub fn rdf_dump_ntriples_fn(
    context: *mut sqlite3_context,
    _values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let nt = dump_ntriples().map_err(sqlite_loadable::Error::from)?;
    api::result_text(context, &nt)?;
    Ok(())
}

// ── rdf_term_type ─────────────────────────────────────────────────────────────

/// Returns "iri", "blank", or "literal" for an N-Triples encoded term string.
pub fn rdf_term_type_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let term = api::value_text(values.get(0).expect("term"))?;
    let kind = if term.starts_with('<') {
        "iri"
    } else if term.starts_with("_:") {
        "blank"
    } else if term.starts_with('"') {
        "literal"
    } else {
        "unknown"
    };
    api::result_text(context, kind)?;
    Ok(())
}

// ── rdf_term_value ────────────────────────────────────────────────────────────

/// Extracts the plain string value from an N-Triples encoded term.
///
/// - `<http://example.org/foo>` → `http://example.org/foo`
/// - `_:b0` → `b0`
/// - `"hello"@en` → `hello`
/// - `"42"^^<xsd:integer>` → `42`
pub fn rdf_term_value_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let term = api::value_text(values.get(0).expect("term"))?;
    let value = extract_term_value(term)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_text(context, &value)?;
    Ok(())
}

fn extract_term_value(term: &str) -> crate::error::Result<String> {
    if let Some(iri) = term.strip_prefix('<') {
        Ok(iri.trim_end_matches('>').to_string())
    } else if let Some(id) = term.strip_prefix("_:") {
        Ok(id.to_string())
    } else if term.starts_with('"') {
        // Strip leading quote, then find closing quote
        let rest = &term[1..];
        let close = rest.rfind('"').ok_or_else(|| {
            SparqlError::InvalidArgument(format!("malformed literal: {term}"))
        })?;
        Ok(rest[..close].to_string())
    } else {
        Err(SparqlError::InvalidArgument(format!(
            "unrecognised term format: {term}"
        )))
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn load_rdf(
    text: &str,
    format: RdfFormat,
    graph: Option<&str>,
) -> crate::error::Result<usize> {
    // Resolve the graph IRI once. `None` → default graph, preserving the 1-arg
    // loader contract; `Some(iri)` → named graph, with the same blank-node /
    // empty-string rejection as the 4-arg `rdf_insert` path.
    let graph_name = parse_graph_name(graph)?;

    with_store(|store| {
        let mut count = 0usize;
        let parser = RdfParser::from_format(format);
        for quad_result in parser.for_reader(text.as_bytes()) {
            let quad = quad_result
                .map_err(|e| SparqlError::RdfParseError(e.to_string()))?;
            let routed = oxigraph::model::Quad::new(
                quad.subject,
                quad.predicate,
                quad.object,
                graph_name.clone(),
            );
            store
                .insert(&routed)
                .map_err(|e| SparqlError::StoreError(e.to_string()))?;
            count += 1;
        }
        Ok(count)
    })
}

fn dump_ntriples() -> crate::error::Result<String> {
    use crate::functions::sparql_query::{term_to_ntriples, term_to_ntriples_subject};
    use oxigraph::model::Term;

    with_store(|store| {
        let mut out = String::new();
        for quad in store.iter() {
            let quad = quad.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            out.push_str(&format!(
                "{} {} {} .\n",
                term_to_ntriples_subject(&quad.subject),
                format!("<{}>", quad.predicate.as_str()),
                term_to_ntriples(&Term::from(quad.object)),
            ));
        }
        Ok(out)
    })
}

/// Register all RDF triple management functions on the given database connection.
pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(db, "rdf_insert", 3, rdf_insert_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_insert", 4, rdf_insert_g_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_delete", 3, rdf_delete_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_delete", 4, rdf_delete_g_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_clear", 0, rdf_clear_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_count", 0, rdf_count_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_count", 1, rdf_count_g_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "rdf_count_all", 0, rdf_count_all_fn, FunctionFlags::UTF8)?;
    define_scalar_function(
        db,
        "rdf_load_turtle",
        1,
        rdf_load_turtle_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_load_turtle_to_graph",
        2,
        rdf_load_turtle_to_graph_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_load_ntriples",
        1,
        rdf_load_ntriples_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_load_ntriples_to_graph",
        2,
        rdf_load_ntriples_to_graph_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_load_rdfxml",
        1,
        rdf_load_rdfxml_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_load_rdfxml_to_graph",
        2,
        rdf_load_rdfxml_to_graph_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_dump_ntriples",
        0,
        rdf_dump_ntriples_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_term_type",
        1,
        rdf_term_type_fn,
        FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC,
    )?;
    define_scalar_function(
        db,
        "rdf_term_value",
        1,
        rdf_term_value_fn,
        FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC,
    )?;
    Ok(())
}
