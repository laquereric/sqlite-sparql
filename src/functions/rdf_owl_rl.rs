/// Native OWL 2 RL reasoning pass.
///
/// | SQL Function                                                      | Returns                            |
/// |-------------------------------------------------------------------|------------------------------------|
/// | `rdf_owl_rl_materialise(asserted, inferred, options_json)`        | Signed net delta in store size     |
///
/// 0.9.0 shipped the 15-rule subset matching `vv-graph`'s
/// `Vv::Graph::Reasoner::Rules::OwlRl`. **0.10.0 expands to the full
/// W3C OWL 2 RL/RDF derivation rule set — 60 rules across
/// Scm / Cls / Cax / Prp / Eq / Dt.** Two new options:
/// `equality_saturation` (default `true`) gates `eq-rep-s/p/o`;
/// `eq_reflexive` (default `false` — opt-in) gates `eq-ref` which
/// doesn't converge under `provenance: true`. `dt-eq` / `dt-diff`
/// are no-ops in Oxigraph 0.4 (literal-subject triples not
/// representable). The ~15 W3C inconsistency rules defer to a
/// future `rdf_owl_rl_consistent` surface.
///
/// See `docs/plans/PLAN_0.9.0.md` and `docs/plans/PLAN_0.10.0.md`
/// for the full design (return-shape, provenance, atomicity,
/// the realised `eq-ref` non-convergence, the deferred-inconsistency
/// follow-on plan).
use oxigraph::model::{GraphName, Literal, NamedNode, Quad, Subject, Term, Triple};
use serde::Deserialize;
use sqlite_loadable::api::ValueType;
use sqlite_loadable::{api, define_scalar_function, prelude::*, Error, FunctionFlags};

use crate::error::SparqlError;
use crate::store::{parse_graph_name, with_store};

pub(crate) mod rdf_lists;
pub(crate) mod rules;

/// Materialisation options. All fields optional; defaults match
/// `vv-graph`'s `Vv::Graph::Reasoner.materialise!` convention so the
/// equivalence test pins parity.
#[derive(Deserialize, Debug)]
pub(crate) struct MaterialiseOptions {
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    #[serde(default)]
    pub provenance: bool,
    #[serde(default = "default_derived_by")]
    pub derived_by_iri: String,
    #[serde(default = "default_derived_at")]
    pub derived_at_iri: String,
    #[serde(default = "default_rule_prefix")]
    pub rule_iri_prefix: String,
    /// Phase D `eq-rep-s` / `eq-rep-p` / `eq-rep-o` toggle. Default `true`
    /// (W3C OWL 2 RL semantics). Set to `false` to short-circuit the three
    /// term-substitution rules when a graph with heavy `owl:sameAs` linkage
    /// would otherwise blow up the closure (O(N · K) in the worst case).
    /// `eq-sym`, `eq-trans` continue to fire regardless.
    #[serde(default = "default_equality_saturation")]
    pub equality_saturation: bool,
    /// Phase D `eq-ref` toggle. Default `false`. The W3C rule says every
    /// term appearing in any quad position derives a reflexive `owl:sameAs`;
    /// combined with `provenance: true`, the annotations on those reflexive
    /// derivations themselves contain new quoted-triple terms, which `eq-ref`
    /// then derives further reflexives for — the closure does not converge
    /// in practice. Leaving `eq-ref` opt-in keeps the inferred graph
    /// bounded; enable explicitly when round-tripping with a W3C-strict
    /// reasoner that expects the reflexive saturation.
    #[serde(default)]
    pub eq_reflexive: bool,
    /// 0.12.0 — populate the native dependency index as the fixpoint
    /// runs, so a subsequent `rdf_dred_overdelete` can identify inferred
    /// quads whose support is invalidated when a premise is retracted.
    /// Default `false` because tracking has a real allocation cost; turn
    /// on only when the consumer plans a DRed cycle. Tracked rules in
    /// 0.12.0: `scm-sco`, `scm-spo`, `eq-trans`, `cax-sco`, `prp-spo1`.
    /// Untracked rules still fire; their derivations just don't write
    /// through to the index.
    #[serde(default)]
    pub track_dependencies: bool,
}

fn default_max_iterations() -> usize {
    50
}
fn default_derived_by() -> String {
    "http://www.w3.org/ns/prov#wasDerivedFrom".to_string()
}
fn default_derived_at() -> String {
    "http://www.w3.org/ns/prov#generatedAtTime".to_string()
}
fn default_rule_prefix() -> String {
    "urn:semantica:rule:".to_string()
}
fn default_equality_saturation() -> bool {
    true
}

/// `rdf_owl_rl_materialise(asserted_iri TEXT, inferred_iri TEXT, options_json TEXT) → INTEGER`.
///
/// `asserted_iri = NULL` means the default graph. `inferred_iri = NULL` is
/// rejected — mixing derived triples into the default graph would erase
/// the asserted-vs-derived distinction OWL reasoning depends on.
pub fn rdf_owl_rl_materialise_fn(
    context: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> sqlite_loadable::Result<()> {
    let asserted = arg_text_or_null(values.get(0).expect("asserted_iri"))?;
    let inferred = arg_text_or_null(values.get(1).expect("inferred_iri"))?;
    let options_json = arg_text_or_null(values.get(2).expect("options_json"))?
        .unwrap_or("{}");

    let inferred = inferred.ok_or_else(|| {
        Error::new_message(
            "rdf_owl_rl_materialise: inferred_iri must be a named graph \
             (NULL is not allowed for the inferred slot)",
        )
    })?;

    let delta = execute_materialise(asserted, inferred, options_json)
        .map_err(sqlite_loadable::Error::from)?;
    api::result_int64(context, delta);
    Ok(())
}

fn arg_text_or_null<'a>(v: &'a *mut sqlite3_value) -> sqlite_loadable::Result<Option<&'a str>> {
    if api::value_type(v) == ValueType::Null {
        Ok(None)
    } else {
        Ok(Some(api::value_text(v)?))
    }
}

fn execute_materialise(
    asserted: Option<&str>,
    inferred: &str,
    options_json: &str,
) -> crate::error::Result<i64> {
    let opts: MaterialiseOptions = if options_json.trim().is_empty() {
        // Defensive: SQLite may hand back NULL/empty; treat as defaults.
        serde_json::from_str("{}").expect("empty options object is always valid")
    } else {
        serde_json::from_str(options_json).map_err(|e| {
            SparqlError::InvalidArgument(format!(
                "rdf_owl_rl_materialise: options_json: {e}"
            ))
        })?
    };

    let asserted_g = parse_graph_name(asserted)?;
    let inferred_g = parse_graph_name(Some(inferred))?;

    // Resolve provenance IRIs once; reuse across iterations.
    let derived_by_node = NamedNode::new(&opts.derived_by_iri).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_owl_rl_materialise: derived_by_iri: {e}"
        ))
    })?;
    let derived_at_node = NamedNode::new(&opts.derived_at_iri).map_err(|e| {
        SparqlError::InvalidArgument(format!(
            "rdf_owl_rl_materialise: derived_at_iri: {e}"
        ))
    })?;

    with_store(|store| {
        let before = store.len().unwrap_or(0) as i64;
        let mut iteration: usize = 0;

        loop {
            iteration += 1;
            if iteration > opts.max_iterations {
                return Err(SparqlError::EvalError(format!(
                    "rdf_owl_rl_materialise: fixpoint not reached after {} iterations",
                    opts.max_iterations
                )));
            }

            // One timestamp per iteration is honest enough — within a single
            // iteration, all derived triples share the same provenance time.
            let now = now_rfc3339();
            let mut new_quads: Vec<Quad> = Vec::new();
            // Records to push into the dependency index after we know which
            // derived quads were actually fresh (i.e., not already present).
            let mut to_record: Vec<(Quad, Vec<Quad>)> = Vec::new();
            for rule in rules::RULES {
                if !opts.equality_saturation
                    && rules::EQ_REP_RULE_IRIS.contains(&rule.iri)
                {
                    continue;
                }
                if !opts.eq_reflexive && rule.iri == rules::EQ_REF_RULE_IRI {
                    continue;
                }

                // When tracking is on AND the rule has a tracked variant,
                // use it so we capture premise quads. Otherwise fall back to
                // the un-tracked path — the rule still fires, the index just
                // doesn't see it (documented limitation for non-core rules
                // in 0.12.0).
                if opts.track_dependencies {
                    if let Some(tracked) = rule.apply_tracked {
                        let derived = tracked(store, &asserted_g, &inferred_g).map_err(|e| {
                            SparqlError::EvalError(format!(
                                "rdf_owl_rl_materialise: rule {} error at iteration {iteration}: {e}",
                                rule.iri
                            ))
                        })?;
                        for dt in derived {
                            let q = Quad::new(
                                dt.triple.subject.clone(),
                                dt.triple.predicate.clone(),
                                dt.triple.object.clone(),
                                inferred_g.clone(),
                            );
                            if store.contains(&q).unwrap_or(false) {
                                // Even if the quad is already in the store,
                                // a NEW derivation found in this iteration
                                // is still worth recording — multi-derivation
                                // matters for cascade correctness.
                                to_record.push((q.clone(), dt.premises.clone()));
                                continue;
                            }
                            new_quads.push(q.clone());
                            to_record.push((q, dt.premises));
                            if opts.provenance {
                                new_quads.extend(provenance_annotations(
                                    &dt.triple,
                                    rule.iri,
                                    &opts,
                                    &inferred_g,
                                    &derived_by_node,
                                    &derived_at_node,
                                    &now,
                                ));
                            }
                        }
                        continue;
                    }
                }

                let derived = (rule.apply)(store, &asserted_g, &inferred_g).map_err(|e| {
                    SparqlError::EvalError(format!(
                        "rdf_owl_rl_materialise: rule {} error at iteration {iteration}: {e}",
                        rule.iri
                    ))
                })?;
                for t in derived {
                    let q = Quad::new(
                        t.subject.clone(),
                        t.predicate.clone(),
                        t.object.clone(),
                        inferred_g.clone(),
                    );
                    if store.contains(&q).unwrap_or(false) {
                        continue; // dedup: already in inferred graph
                    }
                    new_quads.push(q);
                    if opts.provenance {
                        new_quads.extend(provenance_annotations(
                            &t,
                            rule.iri,
                            &opts,
                            &inferred_g,
                            &derived_by_node,
                            &derived_at_node,
                            &now,
                        ));
                    }
                }
            }

            // Flush index records before deciding fixpoint convergence —
            // a rule may derive an already-present quad but with a NEW
            // derivation, which is itself progress for the index (though
            // not for the store; fixpoint correctly ignores it).
            if opts.track_dependencies && !to_record.is_empty() {
                crate::dependency_index::with_index(|idx| {
                    for (q, premises) in to_record.drain(..) {
                        idx.record(q, premises.into_iter().collect());
                    }
                });
            }

            if new_quads.is_empty() {
                break; // fixpoint
            }
            for q in &new_quads {
                store
                    .insert(q)
                    .map_err(|e| SparqlError::StoreError(e.to_string()))?;
            }
        }

        let after = store.len().unwrap_or(0) as i64;
        Ok(after - before)
    })
}

fn provenance_annotations(
    derived: &Triple,
    rule_short_iri: &str,
    opts: &MaterialiseOptions,
    inferred_g: &GraphName,
    derived_by: &NamedNode,
    derived_at: &NamedNode,
    now_iso: &str,
) -> Vec<Quad> {
    let quoted = Subject::Triple(Box::new(derived.clone()));
    let rule_node = match NamedNode::new(format!("{}{}", opts.rule_iri_prefix, rule_short_iri)) {
        Ok(n) => n,
        // Malformed rule IRI (e.g., operator passed a prefix that produces an
        // invalid IRI when concatenated with the short name) — skip provenance
        // for this triple rather than abort the whole materialise. The asserted
        // derivation still lands; only the annotation is dropped.
        Err(_) => return Vec::new(),
    };
    let xsd_datetime =
        NamedNode::new_unchecked("http://www.w3.org/2001/XMLSchema#dateTime");
    let timestamp = Literal::new_typed_literal(now_iso, xsd_datetime);
    vec![
        Quad::new(
            quoted.clone(),
            derived_by.clone(),
            Term::NamedNode(rule_node),
            inferred_g.clone(),
        ),
        Quad::new(
            quoted,
            derived_at.clone(),
            Term::Literal(timestamp),
            inferred_g.clone(),
        ),
    ]
}

/// RFC3339 timestamp for `xsd:dateTime` literals (UTC, `Z` suffix).
/// Hand-rolled to avoid a `chrono` dependency for the single
/// formatter call site. Hinnant's civil-from-days algorithm —
/// standard reference, correct for the entire valid `xsd:dateTime`
/// range we care about (epoch ± centuries).
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

/// Register the OWL 2 RL materialise function on the given connection.
pub fn register(db: *mut sqlite3) -> sqlite_loadable::Result<()> {
    define_scalar_function(
        db,
        "rdf_owl_rl_materialise",
        3,
        rdf_owl_rl_materialise_fn,
        FunctionFlags::UTF8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_epoch_zero() {
        assert_eq!(epoch_secs_to_components(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn rfc3339_new_year_2024() {
        // 2024-01-01T00:00:00Z is the canonical reference; 2024 is a leap
        // year so this also exercises the leap-day arithmetic.
        assert_eq!(
            epoch_secs_to_components(1_704_067_200),
            (2024, 1, 1, 0, 0, 0)
        );
    }

    #[test]
    fn rfc3339_pads_zeros() {
        // One second past 2024-01-01: should be 2024-01-01T00:00:01Z.
        let (y, mo, d, h, m, s) = epoch_secs_to_components(1_704_067_201);
        let formatted = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}Z");
        assert_eq!(formatted, "2024-01-01T00:00:01Z");
    }
}
