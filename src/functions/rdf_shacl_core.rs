//! Native SHACL Core validator pass.
//!
//! | SQL Function                                                              | Returns                                |
//! |---------------------------------------------------------------------------|----------------------------------------|
//! | `rdf_shacl_core_validate(data, shapes, report, options_json)`             | Violation count (i64; 0 = conforming)  |
//!
//! 0.11.0 ships the 12-constraint subset that mirrors `vv-graph`'s
//! `Vv::Graph::Shacl::ConstraintLibrary` (minCount / maxCount / datatype /
//! nodeKind / class / pattern / minLength / maxLength / in / hasValue /
//! minInclusive / maxInclusive) plus a path evaluator covering predicate,
//! inverse, sequence, alternative, zero-or-more, one-or-more, and
//! zero-or-one paths. The pass walks the data graph once per shape and
//! emits a W3C-conformant `sh:ValidationReport` into the report graph,
//! collapsing what was previously N constraint × M focus-node
//! `sparql_query` round-trips into one FFI crossing.
//!
//! See `docs/plans/PLAN_0.11.0.md` for the full design.

use serde::Deserialize;
use sqlite_loadable::api::ValueType;
use sqlite_loadable::{api, define_scalar_function, prelude::*, Error, FunctionFlags};

use crate::error::SparqlError;
use crate::store::parse_graph_name;

pub(crate) mod constraints;
pub(crate) mod path;
pub(crate) mod validator;

/// Validation options. All fields optional; defaults match
/// `vv-graph`'s `Vv::Graph::Shacl.validate!` convention so the
/// equivalence test pins parity.
#[derive(Deserialize, Debug)]
pub(crate) struct ValidateOptions {
    /// Hard upper bound on emitted violations; the pass aborts with a
    /// fixed-prefix error once exceeded. Prevents pathological shapes
    /// (e.g. `sh:minCount` over a huge focus-node set with no values)
    /// from filling the store.
    #[serde(default = "default_max_violations")]
    pub max_violations: usize,
    /// When `true`, every `sh:ValidationResult` carries an RDF-star
    /// annotation `<< _:v sh:resultMessage "…" >> :reportedBy :Shape_X`
    /// (predicate operator-overridable). Default `false` — VG doesn't
    /// emit provenance by default either.
    #[serde(default)]
    pub provenance: bool,
    /// Predicate for the "reported by" provenance triple. Default
    /// `urn:semantica:shacl:reportedBy`.
    #[serde(default = "default_reported_by_iri")]
    pub reported_by_iri: String,
    /// Predicate for the "reported at" provenance timestamp. Default
    /// `http://www.w3.org/ns/prov#generatedAtTime`.
    #[serde(default = "default_reported_at_iri")]
    pub reported_at_iri: String,
    /// Prefix synthesised for blank-node shape IRIs in the
    /// `sh:sourceShape` slot. Named-node shapes are emitted verbatim.
    #[serde(default = "default_shape_iri_prefix")]
    pub shape_iri_prefix: String,
}

fn default_max_violations() -> usize {
    10_000
}
fn default_reported_by_iri() -> String {
    "urn:semantica:shacl:reportedBy".to_string()
}
fn default_reported_at_iri() -> String {
    "http://www.w3.org/ns/prov#generatedAtTime".to_string()
}
fn default_shape_iri_prefix() -> String {
    "urn:semantica:shape:".to_string()
}

/// `rdf_shacl_core_validate(data_iri TEXT, shapes_iri TEXT, report_iri TEXT, options_json TEXT) → INTEGER`.
///
/// `data_iri = NULL` means the default graph (consistent with
/// `rdf_owl_rl_materialise`). `shapes_iri = NULL` and `report_iri = NULL`
/// are both rejected — shapes have to live somewhere and the report
/// graph can't be the default graph (would mix validation output into
/// asserted data).
pub fn rdf_shacl_core_validate_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let data = arg_text_or_null(values.get(0).expect("data_iri"))?;
    let shapes = arg_text_or_null(values.get(1).expect("shapes_iri"))?;
    let report = arg_text_or_null(values.get(2).expect("report_iri"))?;
    let options_json = arg_text_or_null(values.get(3).expect("options_json"))?
        .unwrap_or("{}");

    let shapes = shapes.ok_or_else(|| {
        Error::new_message(
            "rdf_shacl_core_validate: shapes_iri must be a named graph \
             (NULL is not allowed)",
        )
    })?;
    let report = report.ok_or_else(|| {
        Error::new_message(
            "rdf_shacl_core_validate: report_iri must be a named graph \
             (NULL is not allowed for the report slot)",
        )
    })?;

    let count = execute_validate(data, shapes, report, options_json)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int64(context, count);
    Ok(())
}

fn arg_text_or_null<'a>(v: &'a *mut sqlite3_value) -> sqlite_loadable::Result<Option<&'a str>> {
    if api::value_type(v) == ValueType::Null {
        Ok(None)
    } else {
        Ok(Some(api::value_text(v)?))
    }
}

fn execute_validate(
    data: Option<&str>,
    shapes: &str,
    report: &str,
    options_json: &str,
) -> crate::error::Result<i64> {
    let opts: ValidateOptions = if options_json.trim().is_empty() {
        serde_json::from_str("{}").expect("empty options object is always valid")
    } else {
        serde_json::from_str(options_json).map_err(|e| {
            SparqlError::InvalidArgument(format!(
                "rdf_shacl_core_validate: options_json: {e}"
            ))
        })?
    };

    let data_g = parse_graph_name(data)?;
    let shapes_g = parse_graph_name(Some(shapes))?;
    let report_g = parse_graph_name(Some(report))?;

    validator::run(&data_g, &shapes_g, &report_g, &opts)
}

/// Register the SHACL Core validator on the given connection.
pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(
        db,
        "rdf_shacl_core_validate",
        4,
        rdf_shacl_core_validate_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}
