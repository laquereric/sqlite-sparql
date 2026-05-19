/// `sparql_query(query_string)` → JSON string
///
/// Executes a SPARQL SELECT query against the thread-local Oxigraph store and
/// returns the result set as a JSON array of objects.
///
/// Each row in the result set becomes a JSON object whose keys are the
/// projected variable names and whose values are the RDF term representations.
///
/// # Example (SQL)
/// ```sql
/// SELECT sparql_query('SELECT ?s ?p ?o WHERE { ?s ?p ?o }');
/// -- Returns: [{"s":"http://...","p":"http://...","o":"\"hello\""}]
/// ```
use oxigraph::sparql::{QueryResults, QuerySolution};
use sqlite_loadable::{api, define_scalar_function, prelude::*, FunctionFlags};

use crate::error::SparqlError;
use crate::store::with_store;

/// Scalar function: `sparql_query(query_string) -> TEXT (JSON)`
pub fn sparql_query_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let query_str = api::value_text(values.get(0).expect("1st argument: SPARQL query string"))?;

    let json_result = execute_sparql_select(query_str).map_err(sqlite_loadable::Error::from)?;

    api::result_text(context, &json_result)?;
    Ok(())
}

/// Scalar function: `sparql_ask(query_string) -> INTEGER (0 or 1)`
pub fn sparql_ask_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let query_str = api::value_text(values.get(0).expect("1st argument: SPARQL ASK query"))?;

    let result = execute_sparql_ask(query_str).map_err(sqlite_loadable::Error::from)?;

    api::result_int(context, if result { 1 } else { 0 });
    Ok(())
}

/// Scalar function: `sparql_construct(query_string) -> TEXT (Turtle)`
pub fn sparql_construct_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let query_str =
        api::value_text(values.get(0).expect("1st argument: SPARQL CONSTRUCT query"))?;

    let turtle = execute_sparql_construct(query_str).map_err(sqlite_loadable::Error::from)?;

    api::result_text(context, &turtle)?;
    Ok(())
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn execute_sparql_select(query_str: &str) -> crate::error::Result<String> {
    with_store(|store| {
        let results = store
            .query(query_str)
            .map_err(|e| SparqlError::ParseError(e.to_string()))?;

        match results {
            QueryResults::Solutions(solutions) => {
                let mut rows: Vec<serde_json::Value> = Vec::new();

                for solution in solutions {
                    let solution: QuerySolution =
                        solution.map_err(|e| SparqlError::EvalError(e.to_string()))?;

                    let mut obj = serde_json::Map::new();
                    for (var, term) in solution.iter() {
                        obj.insert(var.as_str().to_string(), term_to_json(term));
                    }
                    rows.push(serde_json::Value::Object(obj));
                }

                serde_json::to_string(&rows).map_err(SparqlError::JsonError)
            }
            _ => Err(SparqlError::InvalidArgument(
                "sparql_query() requires a SELECT query; use sparql_ask() or sparql_construct() for other forms".to_string(),
            )),
        }
    })
}

fn execute_sparql_ask(query_str: &str) -> crate::error::Result<bool> {
    with_store(|store| {
        let results = store
            .query(query_str)
            .map_err(|e| SparqlError::ParseError(e.to_string()))?;

        match results {
            QueryResults::Boolean(b) => Ok(b),
            _ => Err(SparqlError::InvalidArgument(
                "sparql_ask() requires an ASK query".to_string(),
            )),
        }
    })
}

fn execute_sparql_construct(query_str: &str) -> crate::error::Result<String> {
    with_store(|store| {
        let results = store
            .query(query_str)
            .map_err(|e| SparqlError::ParseError(e.to_string()))?;

        match results {
            QueryResults::Graph(triples) => {
                let mut out = String::new();
                for triple in triples {
                    let t = triple.map_err(|e| SparqlError::EvalError(e.to_string()))?;
                    out.push_str(&format!(
                        "{} {} {} .\n",
                        term_to_ntriples_subject(&t.subject),
                        format!("<{}>", t.predicate.as_str()),
                        term_to_ntriples(&t.object),
                    ));
                }
                Ok(out)
            }
            _ => Err(SparqlError::InvalidArgument(
                "sparql_construct() requires a CONSTRUCT query".to_string(),
            )),
        }
    })
}

// ── Term serialisation ────────────────────────────────────────────────────────

use oxigraph::model::{Subject, Term};

fn term_to_json(term: &Term) -> serde_json::Value {
    serde_json::Value::String(term_to_ntriples(term))
}

pub fn term_to_ntriples(term: &Term) -> String {
    match term {
        Term::NamedNode(n) => format!("<{}>", n.as_str()),
        Term::BlankNode(b) => format!("_:{}", b.as_str()),
        Term::Literal(l) => {
            if let Some(lang) = l.language() {
                format!("\"{}\"@{}", l.value(), lang)
            } else if l.datatype().as_str()
                == "http://www.w3.org/2001/XMLSchema#string"
            {
                format!("\"{}\"", l.value())
            } else {
                format!("\"{}\"^^<{}>", l.value(), l.datatype().as_str())
            }
        }
        // RDF-star quoted triples are out of scope for 0.1.x.
        Term::Triple(_) => "\"<<rdf-star unsupported>>\"".to_string(),
    }
}

pub fn term_to_ntriples_subject(subject: &Subject) -> String {
    match subject {
        Subject::NamedNode(n) => format!("<{}>", n.as_str()),
        Subject::BlankNode(b) => format!("_:{}", b.as_str()),
        Subject::Triple(_) => "\"<<rdf-star unsupported>>\"".to_string(),
    }
}

/// Register all SPARQL query functions on the given database connection.
pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(db, "sparql_query", 1, sparql_query_fn, FunctionFlags::UTF8)?;
    define_scalar_function(db, "sparql_ask", 1, sparql_ask_fn, FunctionFlags::UTF8)?;
    define_scalar_function(
        db,
        "sparql_construct",
        1,
        sparql_construct_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
