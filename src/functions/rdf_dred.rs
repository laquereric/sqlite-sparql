//! Native DRed over-deletion (since 0.12.0).
//!
//! | SQL Function                                                       | Returns                                          |
//! |--------------------------------------------------------------------|--------------------------------------------------|
//! | `rdf_dred_overdelete(inferred_iri, retracted_premises_json)`       | INTEGER — count of over-deleted inferred quads   |
//!
//! Reads the dependency index populated during
//! `rdf_owl_rl_materialise(... track_dependencies: true)` and removes
//! every inferred quad whose every derivation became invalid when any
//! of the supplied premises was retracted. Cascades transitively — an
//! over-deleted inferred quad is itself treated as a removed premise
//! for downstream derivations.
//!
//! The retracted premises themselves are **not** touched by this
//! function. The consumer is responsible for `rdf_delete` /
//! `sparql_update` on the asserted graph (DRed's "retract" phase);
//! this scalar handles only the "over-delete" phase. The natural flow
//! is retract → rdf_dred_overdelete → re-materialise.

use oxigraph::model::Quad;
use serde_json::Value;
use sqlite_loadable::api::ValueType;
use sqlite_loadable::{api, define_scalar_function, prelude::*, Error, FunctionFlags};
use std::collections::HashSet;

use crate::dependency_index::with_index;
use crate::store::{build_quad, with_store};

pub fn rdf_dred_overdelete_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let inferred_iri = arg_text_required(
        values.get(0).expect("inferred_iri"),
        "inferred_iri",
    )?;
    let retracted_json = arg_text_required(
        values.get(1).expect("retracted_premises_json"),
        "retracted_premises_json",
    )?;

    // Mirror rdf_owl_rl_materialise's stance: the inferred slot must be
    // a real named graph (no empty IRIs, no default-graph alias).
    if inferred_iri.is_empty() {
        return Err(Error::new_message(
            "rdf_dred_overdelete: inferred_iri must be a named graph \
             (empty string not allowed)",
        ));
    }

    let retracted = parse_retracted_json(retracted_json)?;
    if retracted.is_empty() {
        api::result_int64(context, 0);
        return Ok(());
    }

    // If the index is empty, no derivation chains exist — either the
    // consumer never ran materialise with `track_dependencies: true`,
    // or `rdf_clear()` wiped the index. Surface a fixed-prefix message
    // so consumers can recognise the wiring problem distinctly from
    // "nothing depended on these premises" (which silently returns 0
    // with a populated index).
    let index_empty = with_index(|idx| idx.is_empty());
    if index_empty {
        return Err(Error::new_message(
            "rdf_dred_overdelete: no dependency index — re-run \
             `rdf_owl_rl_materialise` with `track_dependencies: true`",
        ));
    }

    let cascade = with_index(|idx| idx.cascade(&retracted));
    let count = cascade.len() as i64;

    with_store(|store| {
        for q in &cascade {
            // Ignore errors: the quad may already be absent (e.g. cleared
            // by a concurrent operation). The index forget below cleans
            // up the bookkeeping regardless.
            let _ = store.remove(q);
        }
    });
    with_index(|idx| {
        for q in &cascade {
            idx.forget(q);
        }
    });

    api::result_int64(context, count);
    Ok(())
}

fn parse_retracted_json(json_text: &str) -> sqlite_loadable::Result<HashSet<Quad>> {
    let parsed: Value = serde_json::from_str(json_text).map_err(|e| {
        Error::new_message(&format!(
            "rdf_dred_overdelete: retracted_premises_json parse error: {e}"
        ))
    })?;
    let rows = parsed.as_array().ok_or_else(|| {
        Error::new_message(
            "rdf_dred_overdelete: retracted_premises_json must be a JSON array \
             of [s, p, o] or [s, p, o, graph] rows",
        )
    })?;

    let mut out: HashSet<Quad> = HashSet::new();
    for (idx, row) in rows.iter().enumerate() {
        let arr = row.as_array().ok_or_else(|| {
            Error::new_message(&format!(
                "rdf_dred_overdelete: row {idx} is not a JSON array"
            ))
        })?;
        if arr.len() < 3 || arr.len() > 4 {
            return Err(Error::new_message(&format!(
                "rdf_dred_overdelete: row {idx} must have 3 or 4 elements \
                 (subject, predicate, object[, graph])"
            )));
        }
        let s = string_field(&arr[0], idx, "subject")?;
        let p = string_field(&arr[1], idx, "predicate")?;
        let o = string_field(&arr[2], idx, "object")?;
        let g: Option<&str> = if arr.len() == 4 && !arr[3].is_null() {
            Some(string_field(&arr[3], idx, "graph")?)
        } else {
            None
        };
        let quad = build_quad(s, p, o, g).map_err(sqlite_loadable::Error::from)?;
        out.insert(quad);
    }
    Ok(out)
}

fn string_field<'a>(v: &'a Value, idx: usize, name: &str) -> sqlite_loadable::Result<&'a str> {
    v.as_str().ok_or_else(|| {
        Error::new_message(&format!(
            "rdf_dred_overdelete: row {idx}: {name} must be a string"
        ))
    })
}

fn arg_text_required<'a>(
    v: &'a *mut sqlite3_value,
    name: &str,
) -> sqlite_loadable::Result<&'a str> {
    if api::value_type(v) == ValueType::Null {
        return Err(Error::new_message(&format!(
            "rdf_dred_overdelete: {name} is required (NULL not allowed)"
        )));
    }
    api::value_text(v).map_err(|e| Error::new_message(&e.to_string()))
}

pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(
        db,
        "rdf_dred_overdelete",
        2,
        rdf_dred_overdelete_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
