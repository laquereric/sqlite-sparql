//! SHACL Core validator driver.
//!
//! Enumerates `sh:NodeShape` (and any subject with `sh:target*`
//! predicates) in the shapes graph, resolves focus nodes against the
//! data graph, walks each property shape's `sh:path`, evaluates each
//! constraint, and emits a W3C `sh:ValidationReport` into the report
//! graph (cleared before write).

use crate::error::{Result, SparqlError};
use crate::store::with_store;
use oxigraph::model::{
    BlankNode, GraphName, GraphNameRef, Literal, NamedNode, NamedNodeRef, Quad, Subject,
    SubjectRef, Term,
};
use oxigraph::store::Store;
use std::collections::HashSet;

use super::constraints::{
    eval_class, eval_datatype, eval_has_value, eval_in, eval_max_count, eval_max_inclusive,
    eval_max_length, eval_min_count, eval_min_inclusive, eval_min_length, eval_node_kind,
    eval_pattern, EvalCtx, PropertyShapeParams, Violation,
};
use super::path::Path;
use super::ValidateOptions;

// ── Well-known IRIs ──────────────────────────────────────────────────────────

const RDF_TYPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
const RDF_FIRST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#first");
const RDF_REST: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#rest");
const RDF_NIL: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/1999/02/22-rdf-syntax-ns#nil");
const XSD_BOOLEAN: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2001/XMLSchema#boolean");
const XSD_DATETIME: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/2001/XMLSchema#dateTime");

const SH_NODE_SHAPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#NodeShape");
const SH_TARGET_CLASS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#targetClass");
const SH_TARGET_NODE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#targetNode");
const SH_TARGET_SUBJECTS_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#targetSubjectsOf");
const SH_TARGET_OBJECTS_OF: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#targetObjectsOf");
const SH_PROPERTY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#property");
const SH_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#path");
const SH_MIN_COUNT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#minCount");
const SH_MAX_COUNT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#maxCount");
const SH_DATATYPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#datatype");
const SH_NODE_KIND: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#nodeKind");
const SH_CLASS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#class");
const SH_PATTERN: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#pattern");
const SH_FLAGS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#flags");
const SH_MIN_LENGTH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#minLength");
const SH_MAX_LENGTH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#maxLength");
const SH_IN: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#in");
const SH_HAS_VALUE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#hasValue");
const SH_MIN_INCLUSIVE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#minInclusive");
const SH_MAX_INCLUSIVE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#maxInclusive");

const SH_VALIDATION_REPORT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#ValidationReport");
const SH_VALIDATION_RESULT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#ValidationResult");
const SH_CONFORMS: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#conforms");
const SH_RESULT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#result");
const SH_FOCUS_NODE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#focusNode");
const SH_RESULT_PATH: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#resultPath");
const SH_VALUE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#value");
const SH_SOURCE_SHAPE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#sourceShape");
const SH_SOURCE_CONSTRAINT_COMPONENT: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#sourceConstraintComponent");
const SH_RESULT_SEVERITY: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#resultSeverity");
const SH_RESULT_MESSAGE: NamedNodeRef<'_> =
    NamedNodeRef::new_unchecked("http://www.w3.org/ns/shacl#resultMessage");

// ── Entry point ──────────────────────────────────────────────────────────────

pub(crate) fn run(
    data_graph: &GraphName,
    shapes_graph: &GraphName,
    report_graph: &GraphName,
    opts: &ValidateOptions,
) -> Result<i64> {
    with_store(|store| {
        let shapes = enumerate_shapes(store, shapes_graph)?;
        let mut violations: Vec<Violation> = Vec::new();

        for shape in &shapes {
            let targets = resolve_targets(store, shapes_graph, data_graph, &shape.node)?;
            let property_shapes = read_property_shapes(store, shapes_graph, &shape.node)?;
            for focus in &targets {
                for ps in &property_shapes {
                    let path = Path::parse(store, &ps.path, shapes_graph)?;
                    let values = path.evaluate(store, focus, data_graph);
                    let ctx = EvalCtx {
                        store,
                        data_graph,
                        focus: focus.clone(),
                        property_path: Some(ps.path.clone()),
                        values,
                        source_shape: ps.node.clone(),
                    };
                    let new_violations = evaluate_all_constraints(&ctx, &ps.params)?;
                    for v in new_violations {
                        violations.push(v);
                        if violations.len() > opts.max_violations {
                            return Err(SparqlError::EvalError(format!(
                                "rdf_shacl_core_validate: violation count exceeded \
                                 max_violations ({})",
                                opts.max_violations
                            )));
                        }
                    }
                }
            }
        }

        clear_graph(store, report_graph)?;
        emit_report(store, report_graph, &violations, opts)?;
        Ok(violations.len() as i64)
    })
}

// ── Shape enumeration ────────────────────────────────────────────────────────

struct ShapeRef {
    /// The shape's identifier — IRI or blank node.
    node: Subject,
}

fn enumerate_shapes(store: &Store, shapes_graph: &GraphName) -> Result<Vec<ShapeRef>> {
    let mut seen: HashSet<Subject> = HashSet::new();
    let mut out: Vec<ShapeRef> = Vec::new();
    let g = graph_ref(shapes_graph);

    // Pick up: (1) explicit rdf:type sh:NodeShape, (2) subjects of any
    // sh:target* predicate. Implicit class targets (a class also being
    // a NodeShape via rdf:type sh:NodeShape — there are SHACL profiles
    // that infer this; 0.11.0 stays explicit).
    let triggers: [NamedNodeRef<'_>; 4] = [
        SH_TARGET_CLASS,
        SH_TARGET_NODE,
        SH_TARGET_SUBJECTS_OF,
        SH_TARGET_OBJECTS_OF,
    ];
    for predicate in triggers {
        for q in store.quads_for_pattern(None, Some(predicate), None, Some(g)) {
            let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
            if seen.insert(q.subject.clone()) {
                out.push(ShapeRef { node: q.subject });
            }
        }
    }
    // Also pick up rdf:type sh:NodeShape so a shape that only declares
    // its kind (no targets) is at least enumerated; targets list will
    // be empty and the loop body skips.
    for q in store.quads_for_pattern(None, Some(RDF_TYPE), Some(SH_NODE_SHAPE.into()), Some(g)) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if seen.insert(q.subject.clone()) {
            out.push(ShapeRef { node: q.subject });
        }
    }
    Ok(out)
}

// ── Target resolution ────────────────────────────────────────────────────────

fn resolve_targets(
    store: &Store,
    shapes_graph: &GraphName,
    data_graph: &GraphName,
    shape: &Subject,
) -> Result<Vec<Subject>> {
    let g_shapes = graph_ref(shapes_graph);
    let g_data = graph_ref(data_graph);
    let s_ref = subject_ref(shape);
    let mut focus: HashSet<Subject> = HashSet::new();

    // sh:targetNode <iri> — focus is the node verbatim.
    for q in store.quads_for_pattern(Some(s_ref), Some(SH_TARGET_NODE), None, Some(g_shapes)) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if let Some(s) = term_to_subject(&q.object) {
            focus.insert(s);
        }
    }
    // sh:targetClass <C> — focus is every (?f rdf:type C) in data graph.
    for q in store.quads_for_pattern(Some(s_ref), Some(SH_TARGET_CLASS), None, Some(g_shapes)) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if let Term::NamedNode(class_iri) = q.object {
            for f in store.quads_for_pattern(
                None,
                Some(RDF_TYPE),
                Some(Term::NamedNode(class_iri.clone()).as_ref()),
                Some(g_data),
            ) {
                let f = f.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                focus.insert(f.subject);
            }
        }
    }
    // sh:targetSubjectsOf <P> — focus is every subject of (?f P ?_).
    for q in store.quads_for_pattern(
        Some(s_ref),
        Some(SH_TARGET_SUBJECTS_OF),
        None,
        Some(g_shapes),
    ) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if let Term::NamedNode(p) = q.object {
            for f in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_data)) {
                let f = f.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                focus.insert(f.subject);
            }
        }
    }
    // sh:targetObjectsOf <P> — focus is every object of (?_ P ?f).
    for q in store.quads_for_pattern(
        Some(s_ref),
        Some(SH_TARGET_OBJECTS_OF),
        None,
        Some(g_shapes),
    ) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        if let Term::NamedNode(p) = q.object {
            for f in store.quads_for_pattern(None, Some(p.as_ref()), None, Some(g_data)) {
                let f = f.map_err(|e| SparqlError::StoreError(e.to_string()))?;
                if let Some(s) = term_to_subject(&f.object) {
                    focus.insert(s);
                }
            }
        }
    }

    Ok(focus.into_iter().collect())
}

// ── Property shapes ──────────────────────────────────────────────────────────

struct PropertyShape {
    /// The blank/named node identifying this property shape.
    node: Subject,
    /// The raw `sh:path` value (parsed lazily by Path::parse).
    path: Term,
    params: PropertyShapeParams,
}

fn read_property_shapes(
    store: &Store,
    shapes_graph: &GraphName,
    shape: &Subject,
) -> Result<Vec<PropertyShape>> {
    let g = graph_ref(shapes_graph);
    let s_ref = subject_ref(shape);
    let mut out = Vec::new();
    for q in store.quads_for_pattern(Some(s_ref), Some(SH_PROPERTY), None, Some(g)) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        let prop_subj = match term_to_subject(&q.object) {
            Some(s) => s,
            None => continue, // a property shape can't be a literal
        };
        let path = match lookup_object(store, shapes_graph, &prop_subj, SH_PATH)? {
            Some(t) => t,
            None => {
                return Err(SparqlError::InvalidArgument(format!(
                    "rdf_shacl_core_validate: property shape {prop_subj} has no sh:path"
                )))
            }
        };
        let params = read_params(store, shapes_graph, &prop_subj)?;
        out.push(PropertyShape {
            node: prop_subj,
            path,
            params,
        });
    }
    Ok(out)
}

fn read_params(
    store: &Store,
    shapes_graph: &GraphName,
    shape: &Subject,
) -> Result<PropertyShapeParams> {
    let mut p = PropertyShapeParams::default();
    p.min_count = read_uint(store, shapes_graph, shape, SH_MIN_COUNT)?;
    p.max_count = read_uint(store, shapes_graph, shape, SH_MAX_COUNT)?;
    p.datatype = read_iri_str(store, shapes_graph, shape, SH_DATATYPE)?;
    p.node_kind = read_iri_str(store, shapes_graph, shape, SH_NODE_KIND)?;
    p.class = read_iri_str(store, shapes_graph, shape, SH_CLASS)?;
    p.pattern = read_string_lit(store, shapes_graph, shape, SH_PATTERN)?;
    p.flags = read_string_lit(store, shapes_graph, shape, SH_FLAGS)?;
    p.min_length = read_uint(store, shapes_graph, shape, SH_MIN_LENGTH)?;
    p.max_length = read_uint(store, shapes_graph, shape, SH_MAX_LENGTH)?;
    p.r#in = read_term_list(store, shapes_graph, shape, SH_IN)?;
    p.has_value = lookup_object(store, shapes_graph, shape, SH_HAS_VALUE)?;
    p.min_inclusive = lookup_object(store, shapes_graph, shape, SH_MIN_INCLUSIVE)?;
    p.max_inclusive = lookup_object(store, shapes_graph, shape, SH_MAX_INCLUSIVE)?;
    Ok(p)
}

fn read_uint(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<u64>> {
    let term = match lookup_object(store, graph, subject, predicate)? {
        Some(t) => t,
        None => return Ok(None),
    };
    let lex = match &term {
        Term::Literal(l) => l.value(),
        _ => {
            return Err(SparqlError::InvalidArgument(format!(
                "rdf_shacl_core_validate: expected integer literal for {predicate}, got {term}"
            )))
        }
    };
    lex.parse::<u64>()
        .map(Some)
        .map_err(|e| SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: {predicate}: {e}"
        )))
}

fn read_iri_str(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<String>> {
    match lookup_object(store, graph, subject, predicate)? {
        Some(Term::NamedNode(n)) => Ok(Some(n.into_string())),
        Some(other) => Err(SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: expected IRI for {predicate}, got {other}"
        ))),
        None => Ok(None),
    }
}

fn read_string_lit(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<String>> {
    match lookup_object(store, graph, subject, predicate)? {
        Some(Term::Literal(l)) => Ok(Some(l.value().to_string())),
        Some(other) => Err(SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: expected string literal for {predicate}, got {other}"
        ))),
        None => Ok(None),
    }
}

fn read_term_list(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<Vec<Term>>> {
    let head = match lookup_object(store, graph, subject, predicate)? {
        Some(t) => t,
        None => return Ok(None),
    };
    let mut out = Vec::new();
    let mut seen: HashSet<Subject> = HashSet::new();
    let mut cursor_term = head;
    loop {
        let cursor: Subject = match cursor_term {
            Term::NamedNode(n) if n.as_ref() == RDF_NIL => return Ok(Some(out)),
            Term::NamedNode(n) => Subject::NamedNode(n),
            Term::BlankNode(b) => Subject::BlankNode(b),
            Term::Triple(t) => Subject::Triple(t),
            Term::Literal(_) => {
                return Err(SparqlError::InvalidArgument(
                    "rdf_shacl_core_validate: sh:in list contains a literal head".to_string(),
                ))
            }
        };
        if !seen.insert(cursor.clone()) {
            return Err(SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:in list has a cycle".to_string(),
            ));
        }
        let first = lookup_object(store, graph, &cursor, RDF_FIRST)?.ok_or_else(|| {
            SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:in list node missing rdf:first".to_string(),
            )
        })?;
        out.push(first);
        let rest = lookup_object(store, graph, &cursor, RDF_REST)?.ok_or_else(|| {
            SparqlError::InvalidArgument(
                "rdf_shacl_core_validate: sh:in list node missing rdf:rest".to_string(),
            )
        })?;
        cursor_term = rest;
    }
}

// ── Constraint dispatch ──────────────────────────────────────────────────────

fn evaluate_all_constraints(
    ctx: &EvalCtx,
    params: &PropertyShapeParams,
) -> Result<Vec<Violation>> {
    let mut out = Vec::new();
    out.extend(eval_min_count(ctx, params)?);
    out.extend(eval_max_count(ctx, params)?);
    out.extend(eval_datatype(ctx, params)?);
    out.extend(eval_node_kind(ctx, params)?);
    out.extend(eval_class(ctx, params)?);
    out.extend(eval_pattern(ctx, params)?);
    out.extend(eval_min_length(ctx, params)?);
    out.extend(eval_max_length(ctx, params)?);
    out.extend(eval_in(ctx, params)?);
    out.extend(eval_has_value(ctx, params)?);
    out.extend(eval_min_inclusive(ctx, params)?);
    out.extend(eval_max_inclusive(ctx, params)?);
    Ok(out)
}

// ── Report emission ──────────────────────────────────────────────────────────

fn emit_report(
    store: &Store,
    report_graph: &GraphName,
    violations: &[Violation],
    opts: &ValidateOptions,
) -> Result<()> {
    let report_node = BlankNode::default();
    let conforms = violations.is_empty();
    let now = now_rfc3339();

    insert(
        store,
        report_graph,
        Subject::BlankNode(report_node.clone()),
        NamedNode::from(RDF_TYPE),
        Term::NamedNode(NamedNode::from(SH_VALIDATION_REPORT)),
    )?;
    insert(
        store,
        report_graph,
        Subject::BlankNode(report_node.clone()),
        NamedNode::from(SH_CONFORMS),
        Term::Literal(Literal::new_typed_literal(
            if conforms { "true" } else { "false" },
            NamedNode::from(XSD_BOOLEAN),
        )),
    )?;

    let reported_by_node = NamedNode::new(&opts.reported_by_iri).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: reported_by_iri: {e}"
        ))
    })?;
    let reported_at_node = NamedNode::new(&opts.reported_at_iri).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_shacl_core_validate: reported_at_iri: {e}"
        ))
    })?;

    for v in violations {
        let result_node = BlankNode::default();
        let result_subj = Subject::BlankNode(result_node.clone());
        insert(
            store,
            report_graph,
            Subject::BlankNode(report_node.clone()),
            NamedNode::from(SH_RESULT),
            Term::BlankNode(result_node.clone()),
        )?;
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(RDF_TYPE),
            Term::NamedNode(NamedNode::from(SH_VALIDATION_RESULT)),
        )?;
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(SH_FOCUS_NODE),
            term_from_subject(&v.focus_node),
        )?;
        if let Some(path) = &v.result_path {
            insert(
                store,
                report_graph,
                result_subj.clone(),
                NamedNode::from(SH_RESULT_PATH),
                path.clone(),
            )?;
        }
        if let Some(value) = &v.value {
            insert(
                store,
                report_graph,
                result_subj.clone(),
                NamedNode::from(SH_VALUE),
                value.clone(),
            )?;
        }
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(SH_SOURCE_SHAPE),
            shape_iri_term(&v.source_shape, &opts.shape_iri_prefix),
        )?;
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(SH_SOURCE_CONSTRAINT_COMPONENT),
            Term::NamedNode(NamedNode::new(v.source_constraint_component).map_err(|e| {
                SparqlError::InvalidArgument(format!("constraint component IRI: {e}"))
            })?),
        )?;
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(SH_RESULT_SEVERITY),
            Term::NamedNode(NamedNode::new(v.result_severity).map_err(|e| {
                SparqlError::InvalidArgument(format!("severity IRI: {e}"))
            })?),
        )?;
        insert(
            store,
            report_graph,
            result_subj.clone(),
            NamedNode::from(SH_RESULT_MESSAGE),
            Term::Literal(Literal::new_simple_literal(&v.result_message)),
        )?;

        if opts.provenance {
            insert(
                store,
                report_graph,
                result_subj.clone(),
                reported_by_node.clone(),
                shape_iri_term(&v.source_shape, &opts.shape_iri_prefix),
            )?;
            insert(
                store,
                report_graph,
                result_subj,
                reported_at_node.clone(),
                Term::Literal(Literal::new_typed_literal(
                    &now,
                    NamedNode::from(XSD_DATETIME),
                )),
            )?;
        }
    }
    Ok(())
}

/// For named-node shapes the IRI is emitted verbatim; blank-node
/// shapes are stamped with `shape_iri_prefix + <bnode-id>` so the
/// `sh:sourceShape` value remains a stable IRI per the W3C report
/// convention (consumers don't need to grok blank-node identity to
/// pin a shape).
fn shape_iri_term(shape: &Subject, prefix: &str) -> Term {
    match shape {
        Subject::NamedNode(n) => Term::NamedNode(n.clone()),
        Subject::BlankNode(b) => Term::NamedNode(
            NamedNode::new(format!("{}{}", prefix, b.as_str()))
                .unwrap_or_else(|_| NamedNode::new_unchecked(format!("urn:engine:shape:{}", b.as_str()))),
        ),
        Subject::Triple(_) => Term::NamedNode(NamedNode::new_unchecked(
            "urn:engine:shape:unsupported-triple-shape",
        )),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn graph_ref(g: &GraphName) -> GraphNameRef<'_> {
    match g {
        GraphName::DefaultGraph => GraphNameRef::DefaultGraph,
        GraphName::NamedNode(n) => GraphNameRef::NamedNode(n.as_ref()),
        GraphName::BlankNode(b) => GraphNameRef::BlankNode(b.as_ref()),
    }
}

fn subject_ref(s: &Subject) -> SubjectRef<'_> {
    match s {
        Subject::NamedNode(n) => SubjectRef::NamedNode(n.as_ref()),
        Subject::BlankNode(b) => SubjectRef::BlankNode(b.as_ref()),
        Subject::Triple(t) => SubjectRef::Triple(t),
    }
}

fn term_to_subject(t: &Term) -> Option<Subject> {
    match t {
        Term::NamedNode(n) => Some(Subject::NamedNode(n.clone())),
        Term::BlankNode(b) => Some(Subject::BlankNode(b.clone())),
        Term::Triple(t) => Some(Subject::Triple(t.clone())),
        Term::Literal(_) => None,
    }
}

fn term_from_subject(s: &Subject) -> Term {
    match s {
        Subject::NamedNode(n) => Term::NamedNode(n.clone()),
        Subject::BlankNode(b) => Term::BlankNode(b.clone()),
        Subject::Triple(t) => Term::Triple(t.clone()),
    }
}

fn lookup_object(
    store: &Store,
    graph: &GraphName,
    subject: &Subject,
    predicate: NamedNodeRef<'_>,
) -> Result<Option<Term>> {
    for q in store.quads_for_pattern(
        Some(subject_ref(subject)),
        Some(predicate),
        None,
        Some(graph_ref(graph)),
    ) {
        let q = q.map_err(|e| SparqlError::StoreError(e.to_string()))?;
        return Ok(Some(q.object));
    }
    Ok(None)
}

fn insert(
    store: &Store,
    graph: &GraphName,
    s: Subject,
    p: NamedNode,
    o: Term,
) -> Result<()> {
    store
        .insert(&Quad::new(s, p, o, graph.clone()))
        .map(|_| ())
        .map_err(|e| SparqlError::StoreError(e.to_string()))
}

fn clear_graph(store: &Store, graph: &GraphName) -> Result<()> {
    let g = graph_ref(graph);
    let to_remove: Vec<Quad> = store
        .quads_for_pattern(None, None, None, Some(g))
        .filter_map(|q| q.ok())
        .collect();
    for q in to_remove {
        store
            .remove(&q)
            .map(|_| ())
            .map_err(|e| SparqlError::StoreError(e.to_string()))?;
    }
    Ok(())
}

/// RFC3339 timestamp for `xsd:dateTime` provenance literals. Copied
/// (intentionally — single call site) from `rdf_owl_rl.rs` to keep
/// the validator module self-contained.
fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (year, month, day, h, m, s) = epoch_secs_to_components(secs);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

fn epoch_secs_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86400);
    let time_in_day = secs.rem_euclid(86400);
    let h = (time_in_day / 3600) as u32;
    let m = ((time_in_day % 3600) / 60) as u32;
    let s = (time_in_day % 60) as u32;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y_in_era = yoe as i64;
    let y_civil = y_in_era + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m_civil = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (if m_civil <= 2 { y_civil + 1 } else { y_civil }) as i32;
    (year, m_civil, d, h, m, s)
}
