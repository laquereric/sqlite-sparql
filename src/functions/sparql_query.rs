/// SPARQL query scalar functions:
///
/// | SQL Function                       | Returns                                       |
/// |------------------------------------|-----------------------------------------------|
/// | `sparql_query(query)`              | JSON array of solution objects (SELECT)       |
/// | `sparql_ask(query)`                | `0` or `1` (ASK)                              |
/// | `sparql_construct(query)`          | N-Triples-(star) text (CONSTRUCT)             |
/// | `rdf_construct_many(queries_json)` | JSON array of N-Triples blobs (since 0.8.0)   |
/// | `sparql_update(query)`             | Signed net delta in store size (since 0.5.0)  |
///
/// `sparql_query` returns a JSON array of binding objects whose keys are
/// the projected variable names and whose values are the bound terms in
/// N-Triples encoding (RDF-star quoted triples emit as `<< s p o >>`
/// since 0.7.0).
///
/// # Example (SQL)
/// ```sql
/// SELECT sparql_query('SELECT ?s ?p ?o WHERE { ?s ?p ?o }');
/// -- Returns: [{"s":"http://...","p":"http://...","o":"\"hello\""}]
/// ```
use oxigraph::sparql::{EvaluationError, Query, QueryResults, QuerySolution};
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

/// Scalar function: `rdf_construct_many(queries_json) -> TEXT (JSON array)`.
///
/// One FFI crossing for N CONSTRUCTs. Returns a JSON array of N
/// N-Triples blobs, one per input query. Per-query attribution is
/// preserved; provenance shape stays on the consumer side. See
/// PLAN_0.8.0.md.
pub fn rdf_construct_many_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let queries_json = api::value_text(
        values
            .get(0)
            .expect("1st argument: JSON array of CONSTRUCT queries"),
    )?;
    let json_result = execute_sparql_construct_many(queries_json)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_text(context, &json_result)?;
    Ok(())
}

/// Scalar function: `sparql_update(query_string) -> INTEGER (signed net delta)`.
///
/// Runs any SPARQL 1.1 UPDATE form against the process-wide store. The
/// return value is the *signed* change in `rdf_count_all()` across the
/// call:
///
/// | UPDATE shape                              | Return |
/// |-------------------------------------------|--------|
/// | `INSERT DATA { … }`                       | `+N`   |
/// | `DELETE DATA { … }`                       | `-N`   |
/// | `INSERT { … } WHERE { … }`                | `+N`   |
/// | `DELETE { … } WHERE { … }`                | `-N`   |
/// | mixed `DELETE/INSERT { … } WHERE { … }`   | `inserts - deletes` (may be `0`) |
/// | `CLEAR DEFAULT` / `CLEAR ALL`             | `-N`   |
///
/// Oxigraph's `Store::update` doesn't expose a first-class affected-row
/// count, so we sandwich it between `len()` reads. Callers that know
/// their UPDATE is one-direction can `.abs()` the result; mixed-shape
/// callers should observe the store state via `rdf_count` /
/// `sparql_query` instead of relying on the delta.
pub fn sparql_update_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let query_str =
        api::value_text(values.get(0).expect("1st argument: SPARQL UPDATE query"))?;

    let delta = execute_sparql_update(query_str).map_err(sqlite_loadable::Error::from)?;

    api::result_int64(context, delta);
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

fn execute_sparql_update(query_str: &str) -> crate::error::Result<i64> {
    with_store(|store| {
        let before = store.len().unwrap_or(0) as i64;
        store.update(query_str).map_err(classify_evaluation_error)?;
        let after = store.len().unwrap_or(0) as i64;
        Ok(after - before)
    })
}

/// Map Oxigraph's `EvaluationError` onto our own enum. Splitting the
/// parse path out of the evaluation path matters for RS's refusal
/// envelopes — `ParseError` carries the "bad SPARQL syntax" signal.
fn classify_evaluation_error(e: EvaluationError) -> SparqlError {
    match e {
        EvaluationError::Parsing(_) => SparqlError::ParseError(e.to_string()),
        _ => SparqlError::EvalError(e.to_string()),
    }
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

/// Evaluates N CONSTRUCT queries in one FFI crossing and returns a
/// JSON array of N N-Triples blobs. Per-query attribution preserved.
/// See PLAN_0.8.0.md for the return-shape rationale.
///
/// Atomicity: all queries are parse-validated up front; if any parse
/// fails the batch errors with `(query index N)` before any
/// evaluation runs. CONSTRUCT is read-only, so partial-on-evaluation
/// has no rollback question.
fn execute_sparql_construct_many(queries_json: &str) -> crate::error::Result<String> {
    let queries: Vec<String> = serde_json::from_str(queries_json).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_construct_many: expected JSON array of query strings: {e}"
        ))
    })?;

    for (i, q) in queries.iter().enumerate() {
        if let Err(e) = Query::parse(q, None) {
            return Err(SparqlError::ParseError(format!(
                "SPARQL parse error (query index {i}): {e}"
            )));
        }
    }

    with_store(|store| {
        let mut results: Vec<String> = Vec::with_capacity(queries.len());
        for (i, q) in queries.iter().enumerate() {
            let qres = store.query(q).map_err(|e| {
                SparqlError::EvalError(format!(
                    "SPARQL evaluation error (query index {i}): {e}"
                ))
            })?;
            match qres {
                QueryResults::Graph(triples) => {
                    let mut blob = String::new();
                    for t in triples {
                        let t = t.map_err(|e| {
                            SparqlError::EvalError(format!(
                                "evaluation (query index {i}): {e}"
                            ))
                        })?;
                        blob.push_str(&format!(
                            "{} {} {} .\n",
                            term_to_ntriples_subject(&t.subject),
                            format!("<{}>", t.predicate.as_str()),
                            term_to_ntriples(&t.object),
                        ));
                    }
                    results.push(blob);
                }
                _ => {
                    return Err(SparqlError::InvalidArgument(format!(
                        "rdf_construct_many: query index {i} is not a CONSTRUCT"
                    )));
                }
            }
        }
        serde_json::to_string(&results).map_err(SparqlError::JsonError)
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
        // N-Triples-star encoding: << s p o >>
        Term::Triple(t) => format!(
            "<< {} <{}> {} >>",
            term_to_ntriples_subject(&t.subject),
            t.predicate.as_str(),
            term_to_ntriples(&t.object),
        ),
    }
}

pub fn term_to_ntriples_subject(subject: &Subject) -> String {
    match subject {
        Subject::NamedNode(n) => format!("<{}>", n.as_str()),
        Subject::BlankNode(b) => format!("_:{}", b.as_str()),
        Subject::Triple(t) => format!(
            "<< {} <{}> {} >>",
            term_to_ntriples_subject(&t.subject),
            t.predicate.as_str(),
            term_to_ntriples(&t.object),
        ),
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
    define_scalar_function(
        db,
        "rdf_construct_many",
        1,
        rdf_construct_many_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "sparql_update",
        1,
        sparql_update_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
