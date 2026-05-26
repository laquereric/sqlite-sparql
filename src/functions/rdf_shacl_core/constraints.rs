//! SHACL Core constraint library — 12-component subset matching
//! `vv-graph`'s `Vv::Graph::Shacl::ConstraintLibrary`.

#![allow(dead_code)]

use crate::error::{Result, SparqlError};
use oxigraph::model::{
    GraphName, GraphNameRef, NamedNode, NamedNodeRef, Subject, SubjectRef, Term,
};
use oxigraph::store::Store;
use std::collections::HashSet;

// ── Well-known IRIs ──────────────────────────────────────────────────────────

const RDF_TYPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
const RDFS_SUB_CLASS_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subClassOf");

const SH_IRI: &str = "http://www.w3.org/ns/shacl#IRI";
const SH_BLANK_NODE: &str = "http://www.w3.org/ns/shacl#BlankNode";
const SH_LITERAL: &str = "http://www.w3.org/ns/shacl#Literal";
const SH_BLANK_NODE_OR_IRI: &str = "http://www.w3.org/ns/shacl#BlankNodeOrIRI";
const SH_BLANK_NODE_OR_LITERAL: &str = "http://www.w3.org/ns/shacl#BlankNodeOrLiteral";
const SH_IRI_OR_LITERAL: &str = "http://www.w3.org/ns/shacl#IRIOrLiteral";

// ── Public types ─────────────────────────────────────────────────────────────

/// One W3C `sh:ValidationResult` worth of fields.
#[derive(Debug, Clone)]
pub(crate) struct Violation {
    pub focus_node: Subject,
    pub result_path: Option<Term>,
    pub value: Option<Term>,
    pub source_shape: Subject,
    pub source_constraint_component: &'static str,
    pub result_severity: &'static str,
    pub result_message: String,
}

/// Context handed to each constraint evaluator.
pub(crate) struct EvalCtx<'a> {
    pub store: &'a Store,
    pub data_graph: &'a GraphName,
    pub focus: Subject,
    pub property_path: Option<Term>,
    pub values: Vec<Term>,
    pub source_shape: Subject,
}

pub(crate) struct Constraint {
    pub iri: &'static str,
    pub evaluate: fn(&EvalCtx, &PropertyShapeParams) -> Result<Vec<Violation>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PropertyShapeParams {
    pub min_count: Option<u64>,
    pub max_count: Option<u64>,
    pub datatype: Option<String>,
    pub node_kind: Option<String>,
    pub class: Option<String>,
    pub pattern: Option<String>,
    pub flags: Option<String>,
    pub min_length: Option<u64>,
    pub max_length: Option<u64>,
    pub r#in: Option<Vec<Term>>,
    pub has_value: Option<Term>,
    pub min_inclusive: Option<Term>,
    pub max_inclusive: Option<Term>,
}

// ── Constraint component IRIs ────────────────────────────────────────────────

pub(crate) const SH_MIN_COUNT_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MinCountConstraintComponent";
pub(crate) const SH_MAX_COUNT_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MaxCountConstraintComponent";
pub(crate) const SH_DATATYPE_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#DatatypeConstraintComponent";
pub(crate) const SH_NODE_KIND_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#NodeKindConstraintComponent";
pub(crate) const SH_CLASS_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#ClassConstraintComponent";
pub(crate) const SH_PATTERN_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#PatternConstraintComponent";
pub(crate) const SH_MIN_LENGTH_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MinLengthConstraintComponent";
pub(crate) const SH_MAX_LENGTH_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MaxLengthConstraintComponent";
pub(crate) const SH_IN_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#InConstraintComponent";
pub(crate) const SH_HAS_VALUE_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#HasValueConstraintComponent";
pub(crate) const SH_MIN_INCLUSIVE_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MinInclusiveConstraintComponent";
pub(crate) const SH_MAX_INCLUSIVE_COMPONENT: &str =
    "http://www.w3.org/ns/shacl#MaxInclusiveConstraintComponent";

pub(crate) const SH_VIOLATION: &str = "http://www.w3.org/ns/shacl#Violation";

// ── Helpers ──────────────────────────────────────────────────────────────────

fn graph_ref(g: &GraphName) -> GraphNameRef<'_> {
    match g {
        GraphName::DefaultGraph => GraphNameRef::DefaultGraph,
        GraphName::NamedNode(n) => GraphNameRef::NamedNode(n.as_ref()),
        GraphName::BlankNode(b) => GraphNameRef::BlankNode(b.as_ref()),
    }
}

fn build_violation(
    ctx: &EvalCtx,
    component: &'static str,
    value: Option<Term>,
    message: String,
) -> Violation {
    Violation {
        focus_node: ctx.focus.clone(),
        result_path: ctx.property_path.clone(),
        value,
        source_shape: ctx.source_shape.clone(),
        source_constraint_component: component,
        result_severity: "http://www.w3.org/ns/shacl#Violation",
        result_message: message,
    }
}

fn literal_lexical(term: &Term) -> Option<&str> {
    if let Term::Literal(l) = term {
        Some(l.value())
    } else {
        None
    }
}

fn literal_as_f64(term: &Term) -> Option<f64> {
    if let Term::Literal(l) = term {
        l.value().parse::<f64>().ok()
    } else {
        None
    }
}

// ── Evaluators ───────────────────────────────────────────────────────────────

pub(crate) fn eval_min_count(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(min) = params.min_count else {
        return Ok(Vec::new());
    };
    if (ctx.values.len() as u64) < min {
        return Ok(vec![build_violation(
            ctx,
            SH_MIN_COUNT_COMPONENT,
            None,
            format!(
                "Less than {min} values on path (found {})",
                ctx.values.len()
            ),
        )]);
    }
    Ok(Vec::new())
}

pub(crate) fn eval_max_count(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(max) = params.max_count else {
        return Ok(Vec::new());
    };
    if (ctx.values.len() as u64) > max {
        return Ok(vec![build_violation(
            ctx,
            SH_MAX_COUNT_COMPONENT,
            None,
            format!(
                "More than {max} values on path (found {})",
                ctx.values.len()
            ),
        )]);
    }
    Ok(Vec::new())
}

pub(crate) fn eval_datatype(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(dt_iri) = &params.datatype else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let conforms = matches!(v, Term::Literal(l) if l.datatype().as_str() == dt_iri.as_str());
        if !conforms {
            out.push(build_violation(
                ctx,
                SH_DATATYPE_COMPONENT,
                Some(v.clone()),
                format!("Value does not have datatype <{dt_iri}>"),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_node_kind(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(kind) = &params.node_kind else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let ok = match kind.as_str() {
            SH_IRI => matches!(v, Term::NamedNode(_)),
            SH_BLANK_NODE => matches!(v, Term::BlankNode(_)),
            SH_LITERAL => matches!(v, Term::Literal(_)),
            SH_BLANK_NODE_OR_IRI => matches!(v, Term::NamedNode(_) | Term::BlankNode(_)),
            SH_BLANK_NODE_OR_LITERAL => matches!(v, Term::BlankNode(_) | Term::Literal(_)),
            SH_IRI_OR_LITERAL => matches!(v, Term::NamedNode(_) | Term::Literal(_)),
            other => {
                return Err(SparqlError::InvalidArgument(format!(
                    "rdf_shacl_core_validate: unknown sh:nodeKind <{other}>"
                )))
            }
        };
        if !ok {
            out.push(build_violation(
                ctx,
                SH_NODE_KIND_COMPONENT,
                Some(v.clone()),
                format!("Value does not have nodeKind <{kind}>"),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_class(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(class_iri) = &params.class else {
        return Ok(Vec::new());
    };
    let class_node = NamedNode::new(class_iri).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: sh:class IRI {class_iri}: {e}"
        ))
    })?;

    let mut out = Vec::new();
    for v in &ctx.values {
        let value_subject: Subject = match v {
            Term::NamedNode(n) => Subject::NamedNode(n.clone()),
            Term::BlankNode(b) => Subject::BlankNode(b.clone()),
            Term::Literal(_) | Term::Triple(_) => {
                out.push(build_violation(
                    ctx,
                    SH_CLASS_COMPONENT,
                    Some(v.clone()),
                    format!("Value is not an instance of class <{class_iri}>"),
                ));
                continue;
            }
        };
        if !instance_of(
            ctx.store,
            ctx.data_graph,
            &value_subject,
            &class_node,
        )? {
            out.push(build_violation(
                ctx,
                SH_CLASS_COMPONENT,
                Some(v.clone()),
                format!("Value is not an instance of class <{class_iri}>"),
            ));
        }
    }
    Ok(out)
}

/// SHACL `sh:class` instance check: there exists a class `T` such that
/// `value rdf:type T` and `T rdfs:subClassOf* class`. The subclass walk
/// stays within the data graph (shapes-graph `rdfs:subClassOf` axioms
/// don't count — SHACL Core sees the data as-is).
fn instance_of(
    store: &Store,
    data: &GraphName,
    value: &Subject,
    class: &NamedNode,
) -> Result<bool> {
    let value_ref: SubjectRef<'_> = match value {
        Subject::NamedNode(n) => SubjectRef::NamedNode(n.as_ref()),
        Subject::BlankNode(b) => SubjectRef::BlankNode(b.as_ref()),
        Subject::Triple(t) => SubjectRef::Triple(t),
    };
    let g = graph_ref(data);

    let mut types: Vec<NamedNode> = Vec::new();
    for q in store.quads_for_pattern(Some(value_ref), Some(RDF_TYPE), None, Some(g)) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if let Term::NamedNode(n) = q.object {
            types.push(n);
        }
    }

    let mut seen: HashSet<NamedNode> = HashSet::new();
    let mut queue: Vec<NamedNode> = types;
    while let Some(cls) = queue.pop() {
        if !seen.insert(cls.clone()) {
            continue;
        }
        if &cls == class {
            return Ok(true);
        }
        for q in store.quads_for_pattern(
            Some(SubjectRef::NamedNode(cls.as_ref())),
            Some(RDFS_SUB_CLASS_OF),
            None,
            Some(g),
        ) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            if let Term::NamedNode(parent) = q.object {
                queue.push(parent);
            }
        }
    }
    Ok(false)
}

pub(crate) fn eval_pattern(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(pattern) = &params.pattern else {
        return Ok(Vec::new());
    };
    let mut builder = regex::RegexBuilder::new(pattern);
    if let Some(flags) = &params.flags {
        for f in flags.chars() {
            match f {
                'i' => {
                    builder.case_insensitive(true);
                }
                's' => {
                    builder.dot_matches_new_line(true);
                }
                'm' => {
                    builder.multi_line(true);
                }
                'x' => {
                    builder.ignore_whitespace(true);
                }
                _ => {} // SHACL flags 'q' (literal) and others — silently ignore
            }
        }
    }
    let re = builder.build().map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: sh:pattern: {e}"
        ))
    })?;

    let mut out = Vec::new();
    for v in &ctx.values {
        let lex = match v {
            Term::Literal(l) => l.value().to_string(),
            Term::NamedNode(n) => n.as_str().to_string(),
            _ => {
                out.push(build_violation(
                    ctx,
                    SH_PATTERN_COMPONENT,
                    Some(v.clone()),
                    "Value cannot be tested against sh:pattern (not a literal or IRI)"
                        .to_string(),
                ));
                continue;
            }
        };
        if !re.is_match(&lex) {
            out.push(build_violation(
                ctx,
                SH_PATTERN_COMPONENT,
                Some(v.clone()),
                format!("Value does not match pattern \"{pattern}\""),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_min_length(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(min) = params.min_length else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let lex = match v {
            Term::Literal(l) => l.value().to_string(),
            Term::NamedNode(n) => n.as_str().to_string(),
            _ => {
                out.push(build_violation(
                    ctx,
                    SH_MIN_LENGTH_COMPONENT,
                    Some(v.clone()),
                    "Value cannot be tested against sh:minLength (not a literal or IRI)"
                        .to_string(),
                ));
                continue;
            }
        };
        if (lex.chars().count() as u64) < min {
            out.push(build_violation(
                ctx,
                SH_MIN_LENGTH_COMPONENT,
                Some(v.clone()),
                format!("Value has length less than {min}"),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_max_length(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(max) = params.max_length else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let lex = match v {
            Term::Literal(l) => l.value().to_string(),
            Term::NamedNode(n) => n.as_str().to_string(),
            _ => {
                out.push(build_violation(
                    ctx,
                    SH_MAX_LENGTH_COMPONENT,
                    Some(v.clone()),
                    "Value cannot be tested against sh:maxLength (not a literal or IRI)"
                        .to_string(),
                ));
                continue;
            }
        };
        if (lex.chars().count() as u64) > max {
            out.push(build_violation(
                ctx,
                SH_MAX_LENGTH_COMPONENT,
                Some(v.clone()),
                format!("Value has length greater than {max}"),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_in(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(list) = &params.r#in else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        if !list.iter().any(|allowed| allowed == v) {
            out.push(build_violation(
                ctx,
                SH_IN_COMPONENT,
                Some(v.clone()),
                "Value is not in the allowed sh:in list".to_string(),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_has_value(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(required) = &params.has_value else {
        return Ok(Vec::new());
    };
    if ctx.values.iter().any(|v| v == required) {
        return Ok(Vec::new());
    }
    Ok(vec![build_violation(
        ctx,
        SH_HAS_VALUE_COMPONENT,
        None,
        "None of the values match the required sh:hasValue".to_string(),
    )])
}

pub(crate) fn eval_min_inclusive(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(bound) = &params.min_inclusive else {
        return Ok(Vec::new());
    };
    let Some(bound_f) = literal_as_f64(bound) else {
        return Err(SparqlError::InvalidArgument(
            "rdf_shacl_core_validate: sh:minInclusive bound is not a numeric literal"
                .to_string(),
        ));
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let lit = literal_as_f64(v);
        let conforms = match lit {
            Some(f) => f >= bound_f,
            None => false,
        };
        if !conforms {
            out.push(build_violation(
                ctx,
                SH_MIN_INCLUSIVE_COMPONENT,
                Some(v.clone()),
                format!(
                    "Value is less than {} or not comparable",
                    literal_lexical(bound).unwrap_or("")
                ),
            ));
        }
    }
    Ok(out)
}

pub(crate) fn eval_max_inclusive(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let Some(bound) = &params.max_inclusive else {
        return Ok(Vec::new());
    };
    let Some(bound_f) = literal_as_f64(bound) else {
        return Err(SparqlError::InvalidArgument(
            "rdf_shacl_core_validate: sh:maxInclusive bound is not a numeric literal"
                .to_string(),
        ));
    };
    let mut out = Vec::new();
    for v in &ctx.values {
        let lit = literal_as_f64(v);
        let conforms = match lit {
            Some(f) => f <= bound_f,
            None => false,
        };
        if !conforms {
            out.push(build_violation(
                ctx,
                SH_MAX_INCLUSIVE_COMPONENT,
                Some(v.clone()),
                format!(
                    "Value is greater than {} or not comparable",
                    literal_lexical(bound).unwrap_or("")
                ),
            ));
        }
    }
    Ok(out)
}

pub(crate) static CONSTRAINTS: &[Constraint] = &[
    Constraint { iri: SH_MIN_COUNT_COMPONENT,     evaluate: eval_min_count },
    Constraint { iri: SH_MAX_COUNT_COMPONENT,     evaluate: eval_max_count },
    Constraint { iri: SH_DATATYPE_COMPONENT,      evaluate: eval_datatype },
    Constraint { iri: SH_NODE_KIND_COMPONENT,     evaluate: eval_node_kind },
    Constraint { iri: SH_CLASS_COMPONENT,         evaluate: eval_class },
    Constraint { iri: SH_PATTERN_COMPONENT,       evaluate: eval_pattern },
    Constraint { iri: SH_MIN_LENGTH_COMPONENT,    evaluate: eval_min_length },
    Constraint { iri: SH_MAX_LENGTH_COMPONENT,    evaluate: eval_max_length },
    Constraint { iri: SH_IN_COMPONENT,            evaluate: eval_in },
    Constraint { iri: SH_HAS_VALUE_COMPONENT,     evaluate: eval_has_value },
    Constraint { iri: SH_MIN_INCLUSIVE_COMPONENT, evaluate: eval_min_inclusive },
    Constraint { iri: SH_MAX_INCLUSIVE_COMPONENT, evaluate: eval_max_inclusive },
];

// ── Per-constraint unit tests ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{Literal, NamedNode, Quad};

    fn iri(s: &str) -> NamedNode {
        NamedNode::new(s).unwrap()
    }

    fn lit(s: &str) -> Term {
        Term::Literal(Literal::new_simple_literal(s))
    }

    fn typed_lit(value: &str, dt: &str) -> Term {
        Term::Literal(Literal::new_typed_literal(value, iri(dt)))
    }

    fn fresh_store() -> Store {
        Store::new().unwrap()
    }

    fn ctx<'a>(
        store: &'a Store,
        graph: &'a GraphName,
        focus_iri: &str,
        path: Option<&str>,
        values: Vec<Term>,
    ) -> EvalCtx<'a> {
        EvalCtx {
            store,
            data_graph: graph,
            focus: Subject::NamedNode(iri(focus_iri)),
            property_path: path.map(|p| Term::NamedNode(iri(p))),
            values,
            source_shape: Subject::NamedNode(iri("http://e/s/Shape")),
        }
    }

    #[test]
    fn min_count_violates_when_too_few() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(&store, &g, "http://e/f", Some("http://e/p"), vec![]);
        let p = PropertyShapeParams {
            min_count: Some(1),
            ..Default::default()
        };
        let v = eval_min_count(&c, &p).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].source_constraint_component, SH_MIN_COUNT_COMPONENT);
    }

    #[test]
    fn min_count_conforms() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("x")],
        );
        let p = PropertyShapeParams {
            min_count: Some(1),
            ..Default::default()
        };
        assert!(eval_min_count(&c, &p).unwrap().is_empty());
    }

    #[test]
    fn max_count_violates_when_too_many() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("a"), lit("b"), lit("c")],
        );
        let p = PropertyShapeParams {
            max_count: Some(2),
            ..Default::default()
        };
        assert_eq!(eval_max_count(&c, &p).unwrap().len(), 1);
    }

    #[test]
    fn datatype_violates_on_mismatch() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![
                typed_lit("42", "http://www.w3.org/2001/XMLSchema#integer"),
                typed_lit("hello", "http://www.w3.org/2001/XMLSchema#string"),
            ],
        );
        let p = PropertyShapeParams {
            datatype: Some("http://www.w3.org/2001/XMLSchema#integer".to_string()),
            ..Default::default()
        };
        let vs = eval_datatype(&c, &p).unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].source_constraint_component, SH_DATATYPE_COMPONENT);
    }

    #[test]
    fn node_kind_iri_violates_on_literal() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("hello"), Term::NamedNode(iri("http://e/x"))],
        );
        let p = PropertyShapeParams {
            node_kind: Some(SH_IRI.to_string()),
            ..Default::default()
        };
        let vs = eval_node_kind(&c, &p).unwrap();
        assert_eq!(vs.len(), 1);
    }

    #[test]
    fn class_walks_subclass_of() {
        let store = fresh_store();
        store
            .insert(&Quad::new(
                Subject::NamedNode(iri("http://e/a")),
                RDF_TYPE.into_owned(),
                Term::NamedNode(iri("http://e/Sub")),
                GraphName::DefaultGraph,
            ))
            .unwrap();
        store
            .insert(&Quad::new(
                Subject::NamedNode(iri("http://e/Sub")),
                RDFS_SUB_CLASS_OF.into_owned(),
                Term::NamedNode(iri("http://e/Super")),
                GraphName::DefaultGraph,
            ))
            .unwrap();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![Term::NamedNode(iri("http://e/a"))],
        );
        let p = PropertyShapeParams {
            class: Some("http://e/Super".to_string()),
            ..Default::default()
        };
        assert!(eval_class(&c, &p).unwrap().is_empty(),
            "subClassOf chain should be walked");
    }

    #[test]
    fn class_violates_when_no_match() {
        let store = fresh_store();
        store
            .insert(&Quad::new(
                Subject::NamedNode(iri("http://e/a")),
                RDF_TYPE.into_owned(),
                Term::NamedNode(iri("http://e/Other")),
                GraphName::DefaultGraph,
            ))
            .unwrap();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![Term::NamedNode(iri("http://e/a"))],
        );
        let p = PropertyShapeParams {
            class: Some("http://e/Super".to_string()),
            ..Default::default()
        };
        assert_eq!(eval_class(&c, &p).unwrap().len(), 1);
    }

    #[test]
    fn pattern_violates_on_mismatch() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("abc"), lit("xyz")],
        );
        let p = PropertyShapeParams {
            pattern: Some("^a".to_string()),
            ..Default::default()
        };
        assert_eq!(eval_pattern(&c, &p).unwrap().len(), 1);
    }

    #[test]
    fn pattern_case_insensitive_flag() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("ABC")],
        );
        let p = PropertyShapeParams {
            pattern: Some("^a".to_string()),
            flags: Some("i".to_string()),
            ..Default::default()
        };
        assert!(eval_pattern(&c, &p).unwrap().is_empty());
    }

    #[test]
    fn min_max_length_bounds() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("a"), lit("toolong")],
        );
        let p_min = PropertyShapeParams {
            min_length: Some(3),
            ..Default::default()
        };
        let p_max = PropertyShapeParams {
            max_length: Some(3),
            ..Default::default()
        };
        assert_eq!(eval_min_length(&c, &p_min).unwrap().len(), 1);
        assert_eq!(eval_max_length(&c, &p_max).unwrap().len(), 1);
    }

    #[test]
    fn in_violates_outside_list() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("a"), lit("z")],
        );
        let p = PropertyShapeParams {
            r#in: Some(vec![lit("a"), lit("b")]),
            ..Default::default()
        };
        let vs = eval_in(&c, &p).unwrap();
        assert_eq!(vs.len(), 1);
        assert_eq!(vs[0].value, Some(lit("z")));
    }

    #[test]
    fn has_value_violates_when_absent() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c_with = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("a"), lit("b")],
        );
        let c_without = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![lit("x"), lit("y")],
        );
        let p = PropertyShapeParams {
            has_value: Some(lit("a")),
            ..Default::default()
        };
        assert!(eval_has_value(&c_with, &p).unwrap().is_empty());
        assert_eq!(eval_has_value(&c_without, &p).unwrap().len(), 1);
    }

    #[test]
    fn min_max_inclusive_bounds() {
        let store = fresh_store();
        let g = GraphName::DefaultGraph;
        let c = ctx(
            &store,
            &g,
            "http://e/f",
            Some("http://e/p"),
            vec![
                typed_lit("5", "http://www.w3.org/2001/XMLSchema#integer"),
                typed_lit("15", "http://www.w3.org/2001/XMLSchema#integer"),
            ],
        );
        let p_min = PropertyShapeParams {
            min_inclusive: Some(typed_lit("10", "http://www.w3.org/2001/XMLSchema#integer")),
            ..Default::default()
        };
        let p_max = PropertyShapeParams {
            max_inclusive: Some(typed_lit("10", "http://www.w3.org/2001/XMLSchema#integer")),
            ..Default::default()
        };
        assert_eq!(eval_min_inclusive(&c, &p_min).unwrap().len(), 1);
        assert_eq!(eval_max_inclusive(&c, &p_max).unwrap().len(), 1);
    }

    #[test]
    fn constraint_table_has_12_entries() {
        assert_eq!(CONSTRAINTS.len(), 12);
    }

}
