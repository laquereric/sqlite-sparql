//! Batched insert / delete via JSON-array input.
//!
//! See `docs/plans/PLAN_0.4.0.md` for the shape contract and rationale.
//! Single TEXT argument; each row is `[s, p, o]` or `[s, p, o, graph]`.
//! Term syntax matches the existing 3-arg `rdf_insert` scalar — bare IRI
//! strings on s/p/o, `"…"` for literals, `_:b0` for blanks. Malformed
//! rows abort the whole batch before touching the store.

use oxigraph::model::Quad;
use serde_json::Value;
use sqlite_loadable::{api, define_scalar_function, prelude::*, FunctionFlags};

use crate::error::SparqlError;
use crate::store::{build_quad, with_store};

// ── rdf_insert_many ──────────────────────────────────────────────────────────

pub fn rdf_insert_many_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let json = api::value_text(values.get(0).expect("JSON array argument"))?;
    let count = insert_many(json).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

// ── rdf_delete_many ──────────────────────────────────────────────────────────

pub fn rdf_delete_many_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let json = api::value_text(values.get(0).expect("JSON array argument"))?;
    let count = delete_many(json).map_err(sqlite_loadable::Error::from)?;
    api::result_int(context, count as i32);
    Ok(())
}

// ── Internal ─────────────────────────────────────────────────────────────────

fn insert_many(json: &str) -> crate::error::Result<usize> {
    let quads = parse_rows(json)?;
    if quads.is_empty() {
        return Ok(0);
    }
    with_store(|store| {
        let before = store.len().unwrap_or(0);
        store
            .bulk_loader()
            .load_quads(quads)
            .map_err(|e| SparqlError::StoreError(e.to_string()))?;
        let after = store.len().unwrap_or(0);
        Ok(after.saturating_sub(before))
    })
}

fn delete_many(json: &str) -> crate::error::Result<usize> {
    let quads = parse_rows(json)?;
    if quads.is_empty() {
        return Ok(0);
    }
    with_store(|store| {
        let mut removed = 0usize;
        for quad in quads {
            // store.remove returns Ok(true) for a real removal, Ok(false)
            // when the quad wasn't present — that's our no-op case.
            match store.remove(&quad) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(e) => return Err(SparqlError::StoreError(e.to_string())),
            }
        }
        Ok(removed)
    })
}

fn parse_rows(json: &str) -> crate::error::Result<Vec<Quad>> {
    let outer: Vec<Value> = serde_json::from_str(json).map_err(SparqlError::JsonError)?;
    if outer.is_empty() {
        return Ok(Vec::new());
    }
    let mut quads = Vec::with_capacity(outer.len());
    for (i, row) in outer.into_iter().enumerate() {
        let row_arr = row.as_array().ok_or_else(|| {
            SparqlError::InvalidArgument(format!("row {i}: expected JSON array"))
        })?;
        let (s, p, o, g) = match row_arr.len() {
            3 => (&row_arr[0], &row_arr[1], &row_arr[2], None),
            4 => (
                &row_arr[0],
                &row_arr[1],
                &row_arr[2],
                Some(&row_arr[3]),
            ),
            n => {
                return Err(SparqlError::InvalidArgument(format!(
                    "row {i}: expected 3 or 4 elements, got {n}"
                )))
            }
        };
        let s = row_string(s, i, "subject")?;
        let p = row_string(p, i, "predicate")?;
        let o = row_string(o, i, "object")?;
        let g_opt = match g {
            None => None,
            Some(Value::Null) => None,
            Some(v) => Some(row_string(v, i, "graph")?),
        };
        quads.push(build_row_quad(&s, &p, &o, g_opt.as_deref(), i)?);
    }
    Ok(quads)
}

fn row_string(v: &Value, row_idx: usize, field: &str) -> crate::error::Result<String> {
    v.as_str().map(str::to_owned).ok_or_else(|| {
        SparqlError::InvalidArgument(format!(
            "row {row_idx}: {field} must be a string"
        ))
    })
}

// row-level error wrapper around store::build_quad — keeps "same parser
// for single + batch" honest (locking in PLAN_0.4.0.md risk #2) and just
// decorates the error message with the row index for diagnostics.
fn build_row_quad(
    s: &str,
    p: &str,
    o: &str,
    graph: Option<&str>,
    row_idx: usize,
) -> crate::error::Result<Quad> {
    build_quad(s, p, o, graph).map_err(|e| {
        SparqlError::InvalidArgument(format!("row {row_idx}: {e}"))
    })
}

/// Register the bulk scalar functions.
pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(
        db,
        "rdf_insert_many",
        1,
        rdf_insert_many_fn,
        FunctionFlags::UTF8,
    )?;
    define_scalar_function(
        db,
        "rdf_delete_many",
        1,
        rdf_delete_many_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
