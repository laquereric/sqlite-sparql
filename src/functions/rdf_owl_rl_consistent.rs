//! OWL 2 RL inconsistency detection (since 0.13.0).
//!
//! | SQL Function                                                          | Returns                                              |
//! |-----------------------------------------------------------------------|------------------------------------------------------|
//! | `rdf_owl_rl_consistent(asserted_iri, inferred_iri, options_json)`     | TEXT — JSON array of `{rule, s, p, o}` records       |
//!
//! Sister to `rdf_owl_rl_materialise`. Read-only: never inserts into
//! the store, never touches the dependency index. Walks the asserted
//! + inferred graphs and emits one `ViolationRecord` per witness for
//! each of the 17 W3C OWL 2 RL/RDF inconsistency rules.
//!
//! Returns `"[]"` (literal two-char string) when the graphs are
//! consistent — distinct from any error. `max_violations` (default
//! `10_000`) bounds the result; exceeding it raises a fixed-prefix
//! error so consumers can pattern-match.

use serde::Deserialize;
use sqlite_loadable::api::ValueType;
use sqlite_loadable::{api, define_scalar_function, prelude::*, Error, FunctionFlags};

use super::rdf_owl_rl::inconsistency::{
    ViolationRecord, INCONSISTENCY_RULES,
};
use crate::error::{Result as SparqlResult, SparqlError};
use crate::store::{parse_graph_name, with_store};

#[derive(Deserialize, Debug)]
struct ConsistentOptions {
    #[serde(default = "default_max_violations")]
    max_violations: usize,
}

fn default_max_violations() -> usize {
    10_000
}

pub fn rdf_owl_rl_consistent_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let asserted = arg_text_or_null(values.get(0).expect("asserted_iri"))?;
    let inferred = arg_text_or_null(values.get(1).expect("inferred_iri"))?;
    let options_json = arg_text_or_null(values.get(2).expect("options_json"))?
        .unwrap_or("{}");

    let inferred = inferred.ok_or_else(|| {
        Error::new_message(
            "rdf_owl_rl_consistent: inferred_iri must be a named graph \
             (NULL is not allowed for the inferred slot)",
        )
    })?;

    let opts: ConsistentOptions = if options_json.trim().is_empty() {
        serde_json::from_str("{}").expect("empty options object is always valid")
    } else {
        serde_json::from_str(options_json).map_err(|e| {
            Error::new_message(&format!(
                "rdf_owl_rl_consistent: options_json: {e}"
            ))
        })?
    };

    let json = execute_consistent(asserted, inferred, &opts)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_text(context, &json)?;
    Ok(())
}

fn execute_consistent(
    asserted: Option<&str>,
    inferred: &str,
    opts: &ConsistentOptions,
) -> SparqlResult<String> {
    let asserted_g = parse_graph_name(asserted)?;
    let inferred_g = parse_graph_name(Some(inferred))?;

    with_store(|store| {
        let mut all: Vec<ViolationRecord> = Vec::new();
        for rule in INCONSISTENCY_RULES {
            if all.len() >= opts.max_violations {
                return Err(SparqlError::EvalError(format!(
                    "rdf_owl_rl_consistent: violation count exceeded \
                     max_violations ({})",
                    opts.max_violations
                )));
            }
            let mut found = (rule.detect)(store, &asserted_g, &inferred_g)
                .map_err(|e| {
                    SparqlError::EvalError(format!(
                        "rdf_owl_rl_consistent: rule {} error: {e}",
                        rule.iri
                    ))
                })?;
            let remaining = opts.max_violations.saturating_sub(all.len());
            if found.len() > remaining {
                // Hard truncation — and *also* a guard error if we'd lose
                // information. Match SHACL's posture: exceeding the cap
                // is an error, not a silent truncate, so the consumer
                // knows to raise the cap or scope the input narrower.
                return Err(SparqlError::EvalError(format!(
                    "rdf_owl_rl_consistent: violation count exceeded \
                     max_violations ({})",
                    opts.max_violations
                )));
            }
            found.sort();
            all.append(&mut found);
        }
        // Global sort for stable output across runs (per-rule output is
        // already sorted; this pins inter-rule order too).
        all.sort();
        Ok(serde_json::to_string(&all).expect("violation vec → JSON cannot fail"))
    })
}

fn arg_text_or_null<'a>(v: &'a *mut sqlite3_value) -> sqlite_loadable::Result<Option<&'a str>> {
    if api::value_type(v) == ValueType::Null {
        Ok(None)
    } else {
        api::value_text(v)
            .map(Some)
            .map_err(|e| Error::new_message(&e.to_string()))
    }
}

pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(
        db,
        "rdf_owl_rl_consistent",
        3,
        rdf_owl_rl_consistent_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
