use super::analysis_test_observer::{AnalysisPhaseEvent, observe_analysis_phase};
use super::*;
use projectatlas_core::graph::{
    CoverageRecord, CoverageScope, CoverageState, GraphIdentityText, LogicalRelation,
    RelationResolution, RepositoryFilePath, RepositoryNodePath, SymbolSelector,
};
use projectatlas_core::symbols::{ParserKind, SymbolGraph, SymbolKind};
use projectatlas_core::{IndexGeneration, Node, NodeKind, PurposeSource};
use std::cell::Cell;
use std::error::Error;
use std::fs;
use std::io;
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;

#[cfg(unix)]
#[test]
fn analysis_cursor_identity_preserves_non_utf8_root_collisions() -> Result<(), Box<dyn Error>> {
    use projectatlas_core::CanonicalProjectRoot;
    use std::os::unix::ffi::OsStringExt;

    let temp = tempfile::tempdir()?;
    let native = temp
        .path()
        .join(std::ffi::OsString::from_vec(vec![b'r', b'o', b'o', 0x80]));
    let replacement = temp.path().join("roo�");
    fs::create_dir(&native)?;
    fs::create_dir(&replacement)?;
    let native_root = CanonicalProjectRoot::from_path(&native)?;
    let replacement_root = CanonicalProjectRoot::from_path(&replacement)?;
    let query = analysis_query(RelationAnalysisMode::Architecture)?;
    let native_binding = analysis_cursor_binding(&query, &native_root)?;
    let replacement_binding = analysis_cursor_binding(&query, &replacement_root)?;
    require(
        native_binding.root_digest != replacement_binding.root_digest,
        "analysis cursor identity collapsed non-UTF-8 and replacement roots",
    )?;
    Ok(())
}

fn cancel_at_analysis_phase<T>(
    phase: AnalysisPhaseEvent,
    cancellation: IndexCancellation,
    operation: impl FnOnce() -> T,
) -> Result<T, Box<dyn Error>> {
    let seen = Rc::new(Cell::new(false));
    let observer_seen = Rc::clone(&seen);
    let result = observe_analysis_phase(
        move |event| {
            if event == phase {
                observer_seen.set(true);
                cancellation.cancel();
            }
        },
        operation,
    );
    require(
        seen.get(),
        "the deterministic analysis phase hook was not reached",
    )?;
    Ok(result)
}

#[test]
fn analysis_uses_real_graph_rows_dependency_sccs_and_resumable_output() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;
    let query = analysis_query(RelationAnalysisMode::Architecture)?;
    let draft = load_relation_analysis(&store, &query, None)?;
    let original_report_bytes = serialized_bytes_controlled(&draft.report, None)?;
    let (report, encoded) = draft.fit_output::<_, ServiceError, _>(|report, _control| {
        serde_json::to_vec(report).map_err(ServiceError::from)
    })?;
    let fitted_report_bytes = serialized_bytes_controlled(&report, None)?;
    require(
        report.work.rendered_output_bytes == encoded.len() as u64,
        "analysis did not account exact rendered adapter bytes",
    )?;
    require(
        report.work.peak_intermediate_bytes
            >= original_report_bytes
                .saturating_add(fitted_report_bytes)
                .saturating_add(encoded.len() as u64)
            && report.work.peak_intermediate_bytes <= query.relations.budget.intermediate_bytes(),
        "analysis output fitting did not charge the original report, cloned prefix, and encoded envelope",
    )?;
    require(
        report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::Component
                && finding.status == AnalysisStatus::Candidate
        }),
        "weak topology did not remain a component candidate",
    )?;
    let cycle = report
        .findings
        .iter()
        .find(|finding| {
            finding.kind == AnalysisFindingKind::DependencyCycle
                && finding.status == AnalysisStatus::Candidate
        })
        .ok_or("dependency SCC was not reported")?;
    require(
        cycle.nodes.len() == 2,
        "non-dependency edge entered the SCC",
    )?;
    require(
        cycle.nodes.iter().all(|node| {
            node.next_call.is_some()
                && !node.node.coverage.is_empty()
                && node.node.coverage.iter().all(|coverage| {
                    matches!(
                        coverage.state(),
                        CoverageState::Complete | CoverageState::NoCandidates
                    )
                })
        }),
        "analysis nodes omitted typed next calls or authoritative coverage",
    )?;
    require(
        encoded
            .windows("负责".len())
            .any(|window| window == "负责".as_bytes()),
        "Unicode purpose evidence was not preserved byte-safely",
    )?;

    let mut fitted_prefix = None;
    for padding_per_finding in (512..=16 * 1024).step_by(512) {
        let draft = load_relation_analysis(&store, &query, None)?;
        let result = draft.fit_output::<_, ServiceError, _>(|report, _control| {
            let mut bytes = serde_json::to_vec(report).map_err(ServiceError::from)?;
            bytes.resize(
                bytes
                    .len()
                    .saturating_add(report.findings.len().saturating_mul(padding_per_finding)),
                b'x',
            );
            Ok(bytes)
        });
        if let Ok((candidate, encoded)) = result {
            let total = match candidate.total {
                RelationTotalState::Exact(total) | RelationTotalState::AtLeast(total) => total,
                RelationTotalState::Unknown => 0,
            };
            if candidate.returned > 0 && u64::from(candidate.returned) < total {
                fitted_prefix = Some((candidate, encoded));
                break;
            }
        }
    }
    let (prefix, _encoded) = fitted_prefix.ok_or("no strict output prefix fit was found")?;
    let total = match prefix.total {
        RelationTotalState::Exact(total) | RelationTotalState::AtLeast(total) => total,
        RelationTotalState::Unknown => 0,
    };
    require(
        prefix.returned > 0 && u64::from(prefix.returned) < total,
        "output fitting did not retain a nonempty strict finding prefix",
    )?;
    let cursor = prefix
        .continuation
        .ok_or("output-prefix fitting omitted its replay continuation")?;
    let mut resumed = query.clone();
    resumed.relations.cursor = Some(cursor.clone());
    let resumed = load_relation_analysis(&store, &resumed, None)?;
    let (resumed, _) = resumed
        .fit_output::<_, ServiceError, _>(|report, _control| {
            serde_json::to_vec(report).map_err(ServiceError::from)
        })
        .map_err(|error| io::Error::other(format!("resumed fit failed: {error}")))?;
    require(
        resumed.returned > 0,
        "analysis replay cursor made no progress",
    )?;

    let mut mismatched = query.clone();
    mismatched.include_communities = !mismatched.include_communities;
    mismatched.relations.cursor = Some(cursor);
    require(
        matches!(
            load_relation_analysis(&store, &mismatched, None),
            Err(ServiceError::RelationCursorMismatched {
                field: "analysis query"
            })
        ),
        "analysis cursor accepted changed result-defining options",
    )?;

    let zero_prefix = load_relation_analysis(&store, &query, None)?
        .fit_output::<_, ServiceError, _>(|report, _control| {
            if report.findings.is_empty() {
                serde_json::to_vec(report).map_err(ServiceError::from)
            } else {
                Ok(vec![b'x'; 70 * 1024])
            }
        })?
        .0;
    require(
        zero_prefix.returned == 0
            && zero_prefix.truncated
            && zero_prefix.continuation.is_some()
            && zero_prefix
                .reached_limits
                .contains(&GraphLimitKind::OutputBytes)
            && matches!(
                zero_prefix.total,
                RelationTotalState::Exact(total) | RelationTotalState::AtLeast(total)
                    if total > 0
            ),
        "zero-finding output fit omitted its continuation or typed total",
    )?;

    let draft = load_relation_analysis(&store, &query, None)?;
    require(
        draft
            .fit_output::<_, ServiceError, Vec<u8>>(|_report, _control| Ok(vec![b'x'; 70 * 1024]))
            .is_err(),
        "analysis accepted an oversized empty adapter envelope",
    )?;

    let mut memory_bounded = query.clone();
    memory_bounded.relations.budget = memory_bounded.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(64 * 1024),
        None,
    )?;
    let mut memory_prefix = None;
    for padding_per_finding in (256..=8 * 1024).step_by(256) {
        let draft = load_relation_analysis(&store, &memory_bounded, None)?;
        let result = draft.fit_output::<_, ServiceError, _>(|report, _control| {
            let mut bytes = serde_json::to_vec(report).map_err(ServiceError::from)?;
            bytes.resize(
                bytes
                    .len()
                    .saturating_add(report.findings.len().saturating_mul(padding_per_finding)),
                b'm',
            );
            Ok(bytes)
        });
        if let Ok((candidate, encoded)) = result
            && candidate
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes)
        {
            memory_prefix = Some((candidate, encoded));
            break;
        }
    }
    let (memory_prefix, memory_encoded) =
        memory_prefix.ok_or("no aggregate-memory-limited analysis prefix fit was found")?;
    require(
        memory_prefix.truncated
            && memory_prefix.continuation.is_some()
            && !memory_prefix
                .reached_limits
                .contains(&GraphLimitKind::OutputBytes)
            && memory_prefix.work.peak_intermediate_bytes
                <= memory_bounded.relations.budget.intermediate_bytes()
            && memory_encoded.len() <= memory_bounded.relations.budget.output_bytes() as usize,
        "analysis output prefix crossed or failed to report its aggregate fitting-memory bound",
    )?;

    let cancellation = projectatlas_core::IndexCancellation::new();
    cancellation.cancel();
    let control = IndexWorkControl::new(cancellation, None);
    require(
        load_relation_analysis(&store, &query, Some(&control))
            .err()
            .is_some_and(|error| error.to_string().contains("cancel")),
        "analysis did not propagate cancellation",
    )?;

    let mut expired_render = load_relation_analysis(&store, &query, None)?;
    expired_render.control =
        IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
    require(
        expired_render
            .fit_output::<_, ServiceError, _>(|report, _control| {
                serde_json::to_vec(report).map_err(ServiceError::from)
            })
            .err()
            .is_some_and(|error| error.to_string().contains("deadline")),
        "analysis output fitting ignored an expired retained deadline",
    )?;

    let cancelled_render = load_relation_analysis(&store, &query, None)?;
    cancelled_render.control.cancel();
    require(
        cancelled_render
            .fit_output::<_, ServiceError, _>(|report, _control| {
                serde_json::to_vec(report).map_err(ServiceError::from)
            })
            .err()
            .is_some_and(|error| error.to_string().contains("cancel")),
        "analysis output fitting ignored retained cancellation",
    )?;
    Ok(())
}

#[test]
fn impact_walks_dependency_dependents_but_not_contains_or_references() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;
    let query = analysis_query(RelationAnalysisMode::Architecture)?;
    let relations = load_detailed_relations(&store, &query.relations, None)?;
    let nodes = collect_nodes(&relations, None)?;
    let mut edges = collect_report_edges(&relations, None)?;
    let closure = close_induced_edges(
        &store,
        &query,
        &relations.work,
        Instant::now() + Duration::from_secs(5),
        &nodes,
        &mut edges,
        None,
    )?;
    require(closure.complete, "impact fixture closure was truncated")?;
    let mut impact_query = query.clone();
    impact_query.mode = RelationAnalysisMode::Impact;
    impact_query.vcs = Some(GitImpactSelection::WorkingTree);
    let mut supplemental_work = SupplementalWork::default();
    let findings = impact_findings(
        &store,
        &nodes,
        &edges,
        true,
        true,
        &VcsImpact::Available {
            selection: GitImpactSelection::WorkingTree,
            changed_path_count: 1,
        },
        &["src/b.rs".to_string()],
        &impact_query,
        64 * 1024,
        &mut supplemental_work,
        None,
    )?;
    let impacted = findings
        .iter()
        .flat_map(|finding| finding.nodes.iter())
        .filter_map(|node| entity_path(&node.node.entity))
        .collect::<BTreeSet<_>>();
    require(
        impacted.contains("src/a.rs") && impacted.contains("src/b.rs"),
        "dependency reverse impact omitted the changed node or its caller",
    )?;
    require(
        !impacted.contains("tools/c.rs"),
        "containment/reference relation was treated as dependency impact",
    )?;
    Ok(())
}

#[test]
fn impact_dead_code_control_releases_each_bounded_phase() -> Result<(), Box<dyn Error>> {
    let (temp, read_store) = analysis_store()?;
    drop(read_store);
    let root = temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    let store = AtlasStore::open_for_project(&database, &root)?;
    let query = exact_symbol_impact_query("src/a.rs", "d_unused", "fn d_unused()")?;
    let setup_control =
        IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(5)));
    let relations = load_detailed_relations(&store, &query.relations, Some(&setup_control))?;
    let nodes = collect_nodes(&relations, Some(&setup_control))?;
    let mut edges = collect_report_edges(&relations, Some(&setup_control))?;
    let closure = close_induced_edges(
        &store,
        &query,
        &relations.work,
        Instant::now() + Duration::from_secs(5),
        &nodes,
        &mut edges,
        Some(&setup_control),
    )?;
    require(closure.complete, "dead-code fixture closure was incomplete")?;

    let discovery_cancellation = IndexCancellation::new();
    let discovery_control =
        IndexWorkControl::new(discovery_cancellation.clone(), Some(Duration::from_secs(5)));
    let mut discovery_work = SupplementalWork::default();
    let discovery = cancel_at_analysis_phase(
        AnalysisPhaseEvent::DeadCodeDiscovery,
        discovery_cancellation,
        || {
            impact_findings(
                &store,
                &nodes,
                &edges,
                true,
                true,
                &VcsImpact::NotRequested,
                &[],
                &query,
                64 * 1024,
                &mut discovery_work,
                Some(&discovery_control),
            )
        },
    )?;
    require(
        matches!(
            discovery,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "dead-code discovery continued after deterministic in-phase cancellation",
    )?;

    let traversal_cancellation = IndexCancellation::new();
    let traversal_control =
        IndexWorkControl::new(traversal_cancellation.clone(), Some(Duration::from_secs(5)));
    let mut traversal_edges = collect_report_edges(&relations, None)?;
    let traversal = cancel_at_analysis_phase(
        AnalysisPhaseEvent::Traversal,
        traversal_cancellation,
        || {
            close_induced_edges(
                &store,
                &query,
                &relations.work,
                Instant::now() + Duration::from_secs(5),
                &nodes,
                &mut traversal_edges,
                Some(&traversal_control),
            )
        },
    )?;
    require(
        matches!(
            traversal,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "impact traversal continued after deterministic in-phase cancellation",
    )?;

    let hydration_cancellation = IndexCancellation::new();
    let hydration_control =
        IndexWorkControl::new(hydration_cancellation.clone(), Some(Duration::from_secs(5)));
    let hydration = cancel_at_analysis_phase(
        AnalysisPhaseEvent::SymbolHydration,
        hydration_cancellation,
        || load_admitted_symbols(&store, &nodes, 64 * 1024, Some(&hydration_control)),
    )?;
    require(
        matches!(
            hydration,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "symbol hydration continued after deterministic in-phase cancellation",
    )?;

    let composition_cancellation = IndexCancellation::new();
    let composition_control = IndexWorkControl::new(
        composition_cancellation.clone(),
        Some(Duration::from_secs(5)),
    );
    let composition = cancel_at_analysis_phase(
        AnalysisPhaseEvent::Composition,
        composition_cancellation,
        || load_relation_analysis(&store, &query, Some(&composition_control)),
    )?;
    require(
        matches!(
            composition,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "analysis composition continued after deterministic in-phase cancellation",
    )?;

    let output_cancellation = IndexCancellation::new();
    let output_control =
        IndexWorkControl::new(output_cancellation.clone(), Some(Duration::from_secs(5)));
    let output_draft = load_relation_analysis(&store, &query, Some(&output_control))?;
    let output = cancel_at_analysis_phase(
        AnalysisPhaseEvent::OutputRendering,
        output_cancellation,
        || {
            output_draft.fit_output::<_, ServiceError, _>(|report, _control| {
                serde_json::to_vec(report).map_err(ServiceError::from)
            })
        },
    )?;
    require(
        matches!(
            output,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "output rendering continued after deterministic in-phase cancellation",
    )?;

    let publication_before = store
        .index_publication()?
        .ok_or("analysis publication missing")?;
    let successful = IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(5)));
    let (report, _encoded) = load_relation_analysis(&store, &query, Some(&successful))?
        .fit_output::<_, ServiceError, _>(|report, _control| {
        serde_json::to_vec(report).map_err(ServiceError::from)
    })?;
    require(
        report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DeadCode
                && finding.status == AnalysisStatus::Candidate
        }) && report.work.analyzed_nodes <= query.relations.budget.nodes()
            && report.work.analyzed_edges <= query.relations.budget.edges()
            && report.work.peak_intermediate_bytes <= query.relations.budget.intermediate_bytes()
            && report.work.rendered_output_bytes
                <= u64::from(query.relations.budget.output_bytes()),
        "successful dead-code control changed findings or crossed aggregate limits",
    )?;
    require(
        store.index_publication()?.as_ref() == Some(&publication_before),
        "read-only impact analysis changed the authoritative publication",
    )?;

    require(
        store.project_instance_id()?.is_some(),
        "immediate follow-up read failed after terminal control",
    )
}

#[test]
fn leaf_and_larger_impact_entrypoints_share_every_terminal_phase() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let queries = [
        exact_symbol_impact_query("src/a.rs", "d_unused", "fn d_unused()")?,
        exact_symbol_impact_query("src/a.rs", "a_long", "fn a_long()")?,
    ];
    for query in queries {
        for phase in [
            AnalysisPhaseEvent::DeadCodeDiscovery,
            AnalysisPhaseEvent::Traversal,
            AnalysisPhaseEvent::SymbolHydration,
            AnalysisPhaseEvent::Composition,
        ] {
            let cancellation = IndexCancellation::new();
            let control = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(5)));
            let result = cancel_at_analysis_phase(phase, cancellation, || {
                load_relation_analysis(&store, &query, Some(&control))
            })?;
            require(
                matches!(
                    result,
                    Err(ServiceError::Db(DbError::IndexWork(
                        projectatlas_core::IndexWorkFailure::Cancelled {
                            stage: IndexWorkStage::RepositoryTraversal
                        }
                    )))
                ),
                "impact entrypoint continued after deterministic phase cancellation",
            )?;
        }

        let cancellation = IndexCancellation::new();
        let control = IndexWorkControl::new(cancellation.clone(), Some(Duration::from_secs(5)));
        let draft = load_relation_analysis(&store, &query, Some(&control))?;
        let output =
            cancel_at_analysis_phase(AnalysisPhaseEvent::OutputRendering, cancellation, || {
                draft.fit_output::<_, ServiceError, _>(|report, control| {
                    serialized_bytes_controlled(report, Some(control))
                        .map(|bytes| bytes.to_le_bytes().to_vec())
                })
            })?;
        require(
            matches!(
                output,
                Err(ServiceError::Db(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled {
                        stage: IndexWorkStage::RepositoryTraversal
                    }
                )))
            ),
            "impact entrypoint continued after output cancellation",
        )?;

        let successful =
            IndexWorkControl::new(IndexCancellation::new(), Some(Duration::from_secs(5)));
        let (report, _) = load_relation_analysis(&store, &query, Some(&successful))?
            .fit_output::<_, ServiceError, _>(|report, control| {
                serialized_bytes_controlled(report, Some(control))
                    .map(|bytes| bytes.to_le_bytes().to_vec())
            })?;
        require(
            report.mode == RelationAnalysisMode::Impact
                && report.work.analyzed_nodes <= query.relations.budget.nodes()
                && report.work.analyzed_edges <= query.relations.budget.edges()
                && report.work.peak_intermediate_bytes
                    <= query.relations.budget.intermediate_bytes(),
            "non-expired impact entrypoint crossed its aggregate control",
        )?;
    }
    Ok(())
}

#[test]
fn analysis_modes_are_closed_and_partial_evidence_stays_inconclusive() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;

    let mut calls = analysis_query(RelationAnalysisMode::Architecture)?;
    calls.relations.relation = Some(GraphRelationKind::Legacy(RelationKind::Calls));
    calls.include_communities = false;
    let calls_report = fitted_report(&store, &calls)?;
    require(
        calls_report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::PurposeAlignment
                && finding.status == AnalysisStatus::Confirmed
        }),
        "matching complete purposes did not produce alignment",
    )?;

    let full_report = fitted_report(&store, &analysis_query(RelationAnalysisMode::Architecture)?)?;
    require(
        full_report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::PurposeDrift
                && finding.status == AnalysisStatus::Candidate
        }),
        "conflicting cross-folder purposes did not produce drift",
    )?;
    require(
        full_report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::Community
                && finding
                    .nodes
                    .iter()
                    .any(|node| entity_path(&node.node.entity) == Some("src/a.rs"))
                && finding
                    .nodes
                    .iter()
                    .any(|node| entity_path(&node.node.entity) == Some("tools/c.rs"))
        }),
        "relationship community did not cross folder ownership",
    )?;
    require(
        full_report
            .findings
            .iter()
            .filter_map(|finding| {
                (finding.kind == AnalysisFindingKind::Community)
                    .then_some(finding.community.as_ref())
                    .flatten()
            })
            .any(|community| {
                community.members.iter().any(|member| {
                    entity_path(&member.node.entity) == Some("src/a.rs")
                        && !member.node.coverage.is_empty()
                }) && community.members.iter().any(|member| {
                    entity_path(&member.node.entity) == Some("tools/c.rs")
                        && !member.node.coverage.is_empty()
                }) && community.evidence.iter().all(|edge| edge.weight > 0)
            }),
        "community metadata omitted exact member coverage or weighted evidence",
    )?;

    let mut acyclic = analysis_query(RelationAnalysisMode::Architecture)?;
    acyclic.relations.relation = Some(GraphRelationKind::Legacy(RelationKind::DependsOn));
    acyclic.include_communities = false;
    let acyclic = fitted_report(&store, &acyclic)?;
    require(
        acyclic.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DependencyCycle
                && finding.status == AnalysisStatus::Absent
        }),
        "complete acyclic dependency scope did not produce an exact negative",
    )?;

    let mut trace = analysis_query(RelationAnalysisMode::Trace)?;
    trace.include_communities = false;
    trace.include_cycles = false;
    trace.trace_target = Some(RelationAnchor::File {
        file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
    });
    let trace_report = fitted_report(&store, &trace)?;
    require(
        trace_report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::StaticTrace
                && finding.status == AnalysisStatus::Confirmed
                && finding.nodes.len() == 2
        }),
        "exact static trace did not return a node-simple path",
    )?;
    trace.trace_target = Some(RelationAnchor::File {
        file: RepositoryFilePath::new(Path::new("missing.rs"))?,
    });
    trace.relations.relation = Some(GraphRelationKind::Legacy(RelationKind::DependsOn));
    let absent_trace = fitted_report(&store, &trace)?;
    require(
        absent_trace.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::StaticTrace
                && finding.status == AnalysisStatus::Absent
        }),
        "complete missing trace target did not produce an exact negative",
    )?;

    trace.relations.budget =
        trace
            .relations
            .budget
            .with_aggregate_limits(Some(1), None, None, None, None, None)?;
    let bounded_trace = fitted_report(&store, &trace)?;
    require(
        bounded_trace.truncated
            && bounded_trace.findings.iter().any(|finding| {
                finding.kind == AnalysisFindingKind::StaticTrace
                    && finding.status == AnalysisStatus::Inconclusive
                    && finding.summary.contains("bounded traversal")
            }),
        "truncated missing trace target was reported as an exact negative",
    )?;

    let mut bounded = analysis_query(RelationAnalysisMode::Architecture)?;
    bounded.relations.budget =
        bounded
            .relations
            .budget
            .with_aggregate_limits(Some(1), None, None, None, None, None)?;
    let bounded = fitted_report(&store, &bounded)?;
    require(
        bounded.truncated && bounded.reached_limits.contains(&GraphLimitKind::Edges),
        "edge budget truncation was not explicit in the analysis envelope",
    )?;

    let deadline_limited = load_relation_analysis_with_closure_deadline(
        &store,
        &analysis_query(RelationAnalysisMode::Architecture)?,
        None,
        Some(Instant::now()),
        false,
    )?;
    let (deadline_limited, _) =
        deadline_limited.fit_output::<_, ServiceError, _>(|report, _control| {
            serde_json::to_vec(report).map_err(ServiceError::from)
        })?;
    require(
        deadline_limited.truncated
            && deadline_limited
                .reached_limits
                .contains(&GraphLimitKind::Deadline)
            && deadline_limited
                .findings
                .iter()
                .any(|finding| finding.status == AnalysisStatus::Inconclusive),
        "in-progress closure deadline did not return explicit bounded truncation",
    )?;

    let (_partial_temp, partial_store) = analysis_store_with_coverage(false)?;
    let partial = fitted_report(
        &partial_store,
        &analysis_query(RelationAnalysisMode::Architecture)?,
    )?;
    require(
        partial.findings.iter().any(|finding| {
            finding.status == AnalysisStatus::Inconclusive && finding.summary.contains("coverage")
        }) && if partial.truncated {
            matches!(partial.total, RelationTotalState::AtLeast(_))
        } else {
            matches!(partial.total, RelationTotalState::Exact(_))
        },
        "missing local coverage did not stay visibly inconclusive",
    )?;

    let mut invalid = analysis_query(RelationAnalysisMode::Impact)?;
    invalid.vcs = Some(GitImpactSelection::WorkingTree);
    require(
        load_relation_analysis(&store, &invalid, None).is_err(),
        "impact accepted architecture-only controls",
    )?;
    invalid.include_communities = false;
    invalid.include_cycles = false;
    invalid.trace_target = Some(RelationAnchor::File {
        file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
    });
    require(
        load_relation_analysis(&store, &invalid, None).is_err(),
        "impact accepted a trace-only target",
    )?;
    let mut invalid = analysis_query(RelationAnalysisMode::Architecture)?;
    invalid.include_dead_code = true;
    require(
        load_relation_analysis(&store, &invalid, None).is_err(),
        "architecture accepted impact-only dead-code controls",
    )?;
    Ok(())
}

#[test]
fn communities_are_deterministic_and_partition_a_planted_weak_component()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let project = store
        .project_instance_id()?
        .ok_or("project identity missing")?;
    let generation = IndexGeneration::new(1);
    let paths = [
        "group-a/0.rs",
        "group-a/1.rs",
        "group-a/2.rs",
        "group-b/0.rs",
        "group-b/1.rs",
        "group-b/2.rs",
    ];
    let mut nodes = BTreeMap::new();
    for path in paths {
        let entity = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(path))?,
            },
            generation,
        )?;
        nodes.insert(
            entity.key().canonical_identity().to_string(),
            DetailedRelationNode {
                entity,
                classification: None,
                content_selection: None,
                purpose: RelationPurpose::Unavailable {
                    path: Some(path.to_string()),
                },
                coverage: Vec::new(),
            },
        );
    }
    let key = |path: &str| -> Result<String, io::Error> {
        nodes
            .values()
            .find(|node| entity_path(&node.entity) == Some(path))
            .map(|node| node.entity.key().canonical_identity().to_string())
            .ok_or_else(|| io::Error::other(format!("planted node key missing for {path}")))
    };
    let edge =
        |source: &str, target: &str, kind: GraphRelationKind| -> Result<LocalEdge, io::Error> {
            Ok(LocalEdge {
                source: key(source)?,
                target: key(target)?,
                kind,
                complete: true,
            })
        };
    let calls = GraphRelationKind::Legacy(RelationKind::Calls);
    let references = GraphRelationKind::Extended(ExtendedRelationKind::References);
    let edges = vec![
        edge("group-a/0.rs", "group-a/1.rs", calls)?,
        edge("group-a/1.rs", "group-a/2.rs", calls)?,
        edge("group-a/2.rs", "group-a/0.rs", calls)?,
        edge("group-b/0.rs", "group-b/1.rs", calls)?,
        edge("group-b/1.rs", "group-b/2.rs", calls)?,
        edge("group-b/2.rs", "group-b/0.rs", calls)?,
        edge("group-a/2.rs", "group-b/0.rs", references)?,
        edge(
            "group-a/0.rs",
            "group-a/2.rs",
            GraphRelationKind::Legacy(RelationKind::Contains),
        )?,
    ];
    let mut edges = edges;
    edges
        .last_mut()
        .ok_or("containment regression edge missing")?
        .complete = false;
    let query = analysis_query(RelationAnalysisMode::Architecture)?;
    let first = community_findings(&nodes, &edges, true, &query, None)?;
    let second = community_findings(&nodes, &edges, true, &query, None)?;
    let first_bytes = serde_json::to_vec(&first)?;
    require(
        first_bytes == serde_json::to_vec(&second)?,
        "repeated community analysis was not byte stable",
    )?;
    let communities = first
        .iter()
        .filter_map(|finding| finding.community.as_ref())
        .collect::<Vec<_>>();
    require(
        communities.len() == 2
            && communities
                .iter()
                .all(|community| community.convergence == CommunityConvergence::Converged)
            && communities
                .iter()
                .all(|community| community.coverage == CommunityCoverage::Complete)
            && communities.iter().all(|community| !community.truncated)
            && communities.iter().all(|community| {
                community
                    .members
                    .iter()
                    .all(|member| member.node.entity.generation() == generation)
            }),
        "planted community did not return two complete converged groups",
    )?;
    let group_members = communities
        .iter()
        .map(|community| {
            community
                .members
                .iter()
                .filter_map(|member| entity_path(&member.node.entity))
                .collect::<BTreeSet<_>>()
        })
        .collect::<Vec<_>>();
    require(
        group_members.contains(&BTreeSet::from([
            "group-a/0.rs",
            "group-a/1.rs",
            "group-a/2.rs",
        ])) && group_members.contains(&BTreeSet::from([
            "group-b/0.rs",
            "group-b/1.rs",
            "group-b/2.rs",
        ])),
        "weighted propagation did not preserve the planted cohesive groups",
    )?;
    require(
        weak_components(&nodes, &edges, true).len() == 1
            && communities
                .iter()
                .map(|community| community.members.len())
                .sum::<usize>()
                == nodes.len(),
        "community projection did not improve the giant weak-component baseline",
    )?;
    require(
        communities.iter().all(|community| {
            community
                .weights
                .iter()
                .any(|weight| weight.relation == calls && weight.weight == 8)
                && community
                    .evidence
                    .iter()
                    .all(|evidence| evidence.weight > 0)
        }) && !communities.iter().any(|community| {
            community.evidence.iter().any(|evidence| {
                evidence.relation == GraphRelationKind::Legacy(RelationKind::Contains)
            })
        }),
        "community evidence omitted fixed weights or admitted containment",
    )?;
    require(
        communities
            .iter()
            .map(|community| community.id.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == communities.len(),
        "distinct planted communities did not receive distinct stable IDs",
    )?;
    let partial = community_findings(&nodes, &edges, false, &query, None)?;
    require(
        partial.len() == 1
            && partial[0].status == AnalysisStatus::Inconclusive
            && partial[0].community.as_ref().is_some_and(|community| {
                community.coverage == CommunityCoverage::Partial
                    && community.convergence == CommunityConvergence::Inconclusive
                    && !community.truncated
                    && community.members.len() == nodes.len()
            }),
        "partial community coverage did not remain typed and bounded",
    )?;
    let cancellation = IndexCancellation::new();
    cancellation.cancel();
    let control = IndexWorkControl::new(cancellation, None);
    require(
        community_findings(&nodes, &edges, true, &query, Some(&control)).is_err(),
        "cancelled community analysis continued past its control",
    )?;
    let mut bounded_query = query.clone();
    bounded_query.relations.budget = bounded_query.relations.budget.with_aggregate_limits(
        Some(1),
        Some(3),
        None,
        None,
        None,
        None,
    )?;
    let bounded = community_findings(&nodes, &edges, true, &bounded_query, None)?;
    let bounded_community = bounded
        .first()
        .and_then(|finding| finding.community.as_ref())
        .ok_or("bounded community metadata missing")?;
    require(
        bounded_community.parameters.node_limit == 3
            && bounded_community.parameters.edge_limit == 1
            && bounded_community.id != communities[0].id,
        "community metadata did not use effective caller resource limits",
    )?;
    let parameters = CommunityParameters {
        algorithm_version: COMMUNITY_ALGORITHM_VERSION,
        ordering_version: COMMUNITY_ORDERING_VERSION,
        max_iterations: COMMUNITY_MAX_ITERATIONS,
        node_limit: 3,
        edge_limit: 1,
        output_bytes: 65_536,
        relation: None,
    };
    let (_, bounded_edges, resource_truncated) =
        admitted_community_scope(&nodes, &edges, parameters);
    require(
        resource_truncated && bounded_edges.len() <= parameters.edge_limit as usize,
        "community resource ceilings did not truncate the admitted edge scope",
    )?;
    let (labels, iteration, convergence) = propagate_community_labels(&[], &[], 0, None)?;
    require(
        labels.is_empty() && iteration == 0 && convergence == CommunityConvergence::Converged,
        "empty community graph did not converge without unnecessary rounds",
    )?;
    let (_, iteration, convergence) = propagate_community_labels(
        &["a".to_string(), "b".to_string(), "c".to_string()],
        &[],
        0,
        None,
    )?;
    require(
        iteration == 0 && convergence == CommunityConvergence::IterationLimit,
        "community iteration ceiling did not produce typed non-convergence",
    )?;
    require(
        select_community_label(
            "z",
            BTreeMap::from([
                ("z".to_string(), 1),
                ("b".to_string(), 4),
                ("a".to_string(), 4),
            ]),
        ) == "a",
        "equal community scores did not choose the stable label key",
    )?;
    let singletons = community_findings(&nodes, &[], true, &query, None)?;
    require(
        singletons.len() == nodes.len()
            && singletons.iter().all(|finding| {
                finding.status == AnalysisStatus::Candidate
                    && finding
                        .community
                        .as_ref()
                        .is_some_and(|community| community.members.len() == 1)
            }),
        "sparse disconnected community input did not preserve singleton candidates",
    )?;
    let mut scale_nodes = nodes.clone();
    for index in 0..(MAX_ANALYSIS_NODES as usize - scale_nodes.len()) {
        let path = format!("scale/{index}.rs");
        let entity = GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(&path))?,
            },
            generation,
        )?;
        scale_nodes.insert(
            entity.key().canonical_identity().to_string(),
            DetailedRelationNode {
                entity,
                classification: None,
                content_selection: None,
                purpose: RelationPurpose::Unavailable { path: Some(path) },
                coverage: Vec::new(),
            },
        );
    }
    let scale_keys = scale_nodes.keys().cloned().collect::<Vec<_>>();
    let hub = scale_keys.first().cloned().ok_or("scale hub missing")?;
    let scale_kinds = [
        calls,
        GraphRelationKind::Legacy(RelationKind::Imports),
        GraphRelationKind::Legacy(RelationKind::DependsOn),
        GraphRelationKind::Extended(ExtendedRelationKind::Tests),
    ];
    let scale_edges = scale_keys
        .iter()
        .skip(1)
        .flat_map(|source| {
            scale_kinds.into_iter().map(|kind| LocalEdge {
                source: source.clone(),
                target: hub.clone(),
                kind,
                complete: true,
            })
        })
        .collect::<Vec<_>>();
    let started = Instant::now();
    let scaled = community_findings(&scale_nodes, &scale_edges, true, &query, None)?;
    let elapsed = started.elapsed();
    let scaled_bytes = serde_json::to_vec(&scaled)?;
    require(
        elapsed < Duration::from_secs(5)
            && scale_edges.len() <= MAX_ANALYSIS_EDGES as usize
            && !scaled_bytes.is_empty()
            && scaled.iter().all(|finding| {
                finding
                    .community
                    .as_ref()
                    .is_some_and(|community| community.members.len() <= MAX_ANALYSIS_NODES as usize)
            }),
        "representative high-degree community analysis crossed its bounded envelope",
    )?;
    let mut sparse_budget_query = query;
    sparse_budget_query.relations.budget = sparse_budget_query
        .relations
        .budget
        .with_aggregate_limits(None, None, None, None, Some(64 * 1024), None)?;
    let sparse_budgeted = community_findings(&scale_nodes, &[], true, &sparse_budget_query, None)?;
    require(
        sparse_budgeted.iter().any(|finding| {
            finding.status == AnalysisStatus::Inconclusive
                && finding
                    .community
                    .as_ref()
                    .is_some_and(|community| community.truncated)
        }),
        "sparse singleton construction exceeded its intermediate budget without a typed outcome",
    )?;
    Ok(())
}

#[test]
fn closure_preserves_late_resolution_gaps_and_symbol_findings() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut late_gap = analysis_query(RelationAnalysisMode::Architecture)?;
    late_gap.relations.relation = Some(GraphRelationKind::Extended(
        ExtendedRelationKind::References,
    ));
    late_gap.relations.resolution = RelationResolutionFilter::Any;
    late_gap.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(1, 1, 3, 64 * 1024)?,
    )
    .with_aggregate_limits(Some(100), None, None, None, None, None)?;
    late_gap.include_communities = false;
    late_gap.include_cycles = false;
    let first_page = load_detailed_relations(&store, &late_gap.relations, None)?;
    require(
        first_page.continuation.is_some() && resolution_gap_findings(&first_page, None)?.is_empty(),
        "fixture did not place the ambiguous relation after the first detailed page",
    )?;
    let report = fitted_report(&store, &late_gap)?;
    let gap = report
        .findings
        .iter()
        .find(|finding| finding.kind == AnalysisFindingKind::ResolutionGap)
        .ok_or("analysis closure omitted a later-page resolution gap")?;
    let evidence = gap
        .evidence
        .as_ref()
        .ok_or("resolution gap omitted its logical relation evidence")?;
    require(
        matches!(
            evidence.relation.resolution(),
            RelationResolution::Ambiguous { candidates, .. } if candidates.get() == 2
        ) && evidence.next_call.as_ref().is_some_and(|next| {
            next.direction == RelationDirection::Outbound
                && next.relation == GraphRelationKind::Extended(ExtendedRelationKind::References)
                && next.resolution == RelationResolutionFilter::Ambiguous
                && next.minimum_confidence == ConfidenceClass::Medium
        }),
        "late resolution gap lost candidates or its exact reusable next call",
    )?;

    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    for anchor in ["src/a.rs", "src/b.rs", "tools/c.rs"] {
        let mut query = analysis_query(RelationAnalysisMode::Architecture)?;
        query.relations.anchor = RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new(anchor))?,
        };
        let relations = load_detailed_relations(&store, &query.relations, None)?;
        for (key, node) in collect_nodes(&relations, None)? {
            nodes.entry(key).or_insert(node);
        }
        edges.extend(collect_report_edges(&relations, None)?);
    }
    for (path, name, signature) in [
        ("src/a.rs", "a_long", "fn a_long()"),
        ("tools/c.rs", "c_aux", "fn c_aux()"),
    ] {
        let mut query = analysis_query(RelationAnalysisMode::Architecture)?;
        query.relations.anchor = RelationAnchor::Symbol {
            file: RepositoryFilePath::new(Path::new(path))?,
            name: name.to_string(),
            symbol_kind: Some(SymbolKind::Function),
            parent: None,
            signature: Some(signature.to_string()),
        };
        let relations = load_detailed_relations(&store, &query.relations, None)?;
        for (key, node) in collect_nodes(&relations, None)? {
            nodes.entry(key).or_insert(node);
        }
        edges.extend(collect_report_edges(&relations, None)?);
    }
    let mut work = SupplementalWork::default();
    let structural = structural_findings(&store, &nodes, &edges, true, 64 * 1024, &mut work, None)?;
    let complexity = structural
        .iter()
        .find(|finding| finding.kind == AnalysisFindingKind::StructuralComplexity)
        .ok_or("persisted symbols did not produce structural complexity")?;
    let bottleneck = structural
        .iter()
        .find(|finding| finding.kind == AnalysisFindingKind::Bottleneck)
        .ok_or("persisted symbols did not produce a bottleneck")?;
    require(
        complexity.metric == Some(30)
            && complexity.nodes.iter().any(|node| {
                matches!(
                    node.node.entity.selector(),
                    EntitySelector::Symbol { symbol } if symbol.name.as_str() == "a_long"
                )
            })
            && bottleneck.nodes.iter().any(|node| {
                matches!(
                    node.node.entity.selector(),
                    EntitySelector::Symbol { symbol } if symbol.name.as_str() == "b_hub"
                )
            })
            && work.hydrated_symbols >= 3
            && work.hydrated_symbol_bytes > 0,
        "structural span and graph-degree analyses did not choose independent persisted-symbol winners",
    )?;
    Ok(())
}

#[test]
fn dead_code_requires_complete_exact_usage_scope() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let unused = fitted_report(
        &store,
        &exact_symbol_impact_query("src/a.rs", "d_unused", "fn d_unused()")?,
    )?;
    require(
        unused.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DeadCode
                && finding.status == AnalysisStatus::Candidate
                && finding.nodes.iter().any(|node| {
                    matches!(
                        node.node.entity.selector(),
                        EntitySelector::Symbol { symbol } if symbol.name.as_str() == "d_unused"
                    )
                })
        }),
        "non-exported declaration with containment but no usage inbound was not a dead-code candidate",
    )?;

    let used = fitted_report(
        &store,
        &exact_symbol_impact_query("src/b.rs", "b_hub", "fn b_hub()")?,
    )?;
    require(
        !used.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DeadCode
                && finding.status == AnalysisStatus::Candidate
        }),
        "declaration with trusted inbound calls was reported as dead code",
    )?;

    let mut incomplete = exact_symbol_impact_query("src/a.rs", "a_long", "fn a_long()")?;
    incomplete.relations.resolution = RelationResolutionFilter::Any;
    let incomplete = fitted_report(&store, &incomplete)?;
    require(
        incomplete.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DeadCode
                && finding.status == AnalysisStatus::Inconclusive
                && finding.summary.contains("complete all-family")
        }),
        "ambiguous dead-code scope did not remain inconclusive",
    )?;

    let mut wrong_scope = exact_symbol_impact_query("src/a.rs", "a_long", "fn a_long()")?;
    wrong_scope.relations.direction = RelationDirection::Outbound;
    let wrong_scope = fitted_report(&store, &wrong_scope)?;
    require(
        wrong_scope.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::DeadCode
                && finding.status == AnalysisStatus::Inconclusive
        }),
        "wrong-direction dead-code scope did not remain inconclusive",
    )?;
    Ok(())
}

#[test]
fn vcs_impact_is_typed_for_non_git_working_tree_and_invalid_revision() -> Result<(), Box<dyn Error>>
{
    let (temp, store) = analysis_store()?;
    let mut impact = analysis_query(RelationAnalysisMode::Impact)?;
    impact.include_communities = false;
    impact.include_cycles = false;
    impact.vcs = Some(GitImpactSelection::WorkingTree);
    let unavailable = fitted_report(&store, &impact)?;
    require(
        matches!(unavailable.vcs, VcsImpact::Unavailable { .. }),
        "non-Git impact did not return typed VCS unavailability",
    )?;
    let git_request_only_bytes = u64::try_from(std::mem::size_of::<GitImpactSelection>())
        .unwrap_or(u64::MAX)
        .saturating_mul(2)
        .saturating_add(32);
    let non_git_failure = load_vcs_paths(
        &temp.path().join("analysis-service"),
        GitImpactSelection::WorkingTree,
        4 * 1024,
        Instant::now() + Duration::from_secs(5),
        None,
    );
    require(
        matches!(
            &non_git_failure.report,
            VcsImpact::Unavailable { reason, .. } if reason.contains("git exited")
        ) && non_git_failure.retained_bytes > git_request_only_bytes,
        "non-Git failure did not charge the joined stdout/stderr peak",
    )?;
    let bounded_failure = load_vcs_paths(
        &temp.path().join("analysis-service"),
        GitImpactSelection::WorkingTree,
        256,
        Instant::now() + Duration::from_secs(5),
        None,
    );
    require(
        matches!(bounded_failure.report, VcsImpact::Unavailable { .. })
            && bounded_failure.retained_bytes > git_request_only_bytes,
        "bounded Git failure did not retain its aggregate stream/request peak",
    )?;

    impact.vcs = Some(GitImpactSelection::RevisionRange {
        base: "-invalid".to_string(),
        head: "HEAD".to_string(),
    });
    let invalid = fitted_report(&store, &impact)?;
    require(
        matches!(invalid.vcs, VcsImpact::Unavailable { .. }),
        "invalid revision range was not rejected as typed unavailability",
    )?;

    impact.vcs = Some(GitImpactSelection::RevisionRange {
        base: "main..nested".to_string(),
        head: "HEAD".to_string(),
    });
    let nested = fitted_report(&store, &impact)?;
    require(
        matches!(nested.vcs, VcsImpact::Unavailable { .. }),
        "nested revision expression was not rejected as typed unavailability",
    )?;

    let root = temp.path().join("analysis-service");
    let status = impact::git_command(&root)
        .args(["init", "--quiet"])
        .status()?;
    require(status.success(), "test Git worktree initialization failed")?;
    impact.vcs = Some(GitImpactSelection::WorkingTree);
    let available = fitted_report(&store, &impact)?;
    require(
        matches!(
            available.vcs,
            VcsImpact::Available {
                changed_path_count,
                ..
            } if changed_path_count >= 3
        ),
        "working-tree impact did not return bounded typed VCS evidence",
    )?;
    let deadline_failure = load_vcs_paths(
        &root,
        GitImpactSelection::WorkingTree,
        4 * 1024,
        Instant::now(),
        None,
    );
    require(
        matches!(
            &deadline_failure.report,
            VcsImpact::Unavailable { reason, .. } if reason.contains("deadline")
        ) && deadline_failure.retained_bytes > git_request_only_bytes,
        "deadline failure did not charge buffers joined after child cleanup",
    )?;
    let stream_overflow = load_vcs_paths(
        &root,
        GitImpactSelection::WorkingTree,
        256,
        Instant::now() + Duration::from_secs(5),
        None,
    );
    require(
        matches!(
            &stream_overflow.report,
            VcsImpact::Unavailable { reason, .. } if reason.contains("output exceeded")
        ) && stream_overflow.retained_bytes > git_request_only_bytes,
        "stream overflow did not charge the joined stdout/stderr allocation peak",
    )?;
    Ok(())
}

#[test]
fn vcs_zero_intersection_cursor_freshness_and_shared_budget_are_explicit()
-> Result<(), Box<dyn Error>> {
    let (temp, store) = analysis_store()?;
    let root = temp.path().join("analysis-service");
    initialize_git_fixture(&root)?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/unrelated.md"), "unrelated\n")?;
    let normalization_failure = load_vcs_paths(
        &root,
        GitImpactSelection::WorkingTree,
        512,
        Instant::now() + Duration::from_secs(5),
        None,
    );
    require(
        matches!(
            &normalization_failure.report,
            VcsImpact::Unavailable { reason, .. }
                if reason.contains("normalization exceeded")
        ) && normalization_failure.retained_bytes > 512,
        "failed VCS normalization did not retain its observed over-budget peak",
    )?;

    let mut impact = analysis_query(RelationAnalysisMode::Impact)?;
    impact.include_communities = false;
    impact.include_cycles = false;
    impact.vcs = Some(GitImpactSelection::WorkingTree);
    impact.relations.relation = Some(GraphRelationKind::Extended(
        ExtendedRelationKind::References,
    ));
    impact.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(50, 1, 1, 64 * 1024)?,
    )
    .with_aggregate_limits(Some(100), None, None, None, None, None)?;
    let complete = fitted_report(&store, &impact)?;
    require(
        matches!(
            complete.vcs,
            VcsImpact::Available {
                changed_path_count,
                ..
            } if changed_path_count >= 1
        ) && complete.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::Impact
                && finding.status == AnalysisStatus::Absent
                && finding.metric == Some(0)
        }),
        "valid VCS evidence with zero graph intersection did not produce an exact negative",
    )?;
    let changed_path_count = match complete.vcs {
        VcsImpact::Available {
            changed_path_count, ..
        } => changed_path_count,
        VcsImpact::NotRequested | VcsImpact::Unavailable { .. } => 0,
    };
    require(
        complete.work.vcs_retained_bytes > 32_u64.saturating_add(changed_path_count),
        "VCS work charged only raw path bytes instead of aggregate normalization state",
    )?;

    let mut bounded = impact.clone();
    bounded.relations.budget =
        bounded
            .relations
            .budget
            .with_aggregate_limits(Some(1), None, None, None, None, None)?;
    let bounded = fitted_report(&store, &bounded)?;
    require(
        bounded.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::Impact
                && finding.status == AnalysisStatus::Inconclusive
                && finding.metric == Some(0)
        }),
        "zero VCS intersection under incomplete topology was reported as exact",
    )?;

    let draft = load_relation_analysis(&store, &impact, None)?;
    let prefix = draft
        .fit_output::<_, ServiceError, _>(|report, _control| {
            if report.findings.is_empty() {
                serde_json::to_vec(report).map_err(ServiceError::from)
            } else {
                Ok(vec![b'x'; 70 * 1024])
            }
        })?
        .0;
    let cursor = prefix
        .continuation
        .ok_or("VCS output prefix omitted its replay cursor")?;
    fs::write(root.join("docs/changed-after-cursor.md"), "changed\n")?;
    let mut resumed = impact;
    resumed.relations.cursor = Some(cursor);
    require(
        matches!(
            load_relation_analysis(&store, &resumed, None),
            Err(ServiceError::RelationCursorStale {
                field: "VCS evidence"
            })
        ),
        "analysis cursor accepted changed working-tree evidence",
    )?;

    let exact = exact_symbol_impact_query("src/a.rs", "d_unused", "fn d_unused()")?;
    let relations = load_detailed_relations(&store, &exact.relations, None)?;
    let nodes = collect_nodes(&relations, None)?;
    let mut edges = collect_report_edges(&relations, None)?;
    let closure = close_induced_edges(
        &store,
        &exact,
        &relations.work,
        Instant::now() + Duration::from_secs(5),
        &nodes,
        &mut edges,
        None,
    )?;
    require(
        closure.complete,
        "combined-budget fixture closure was incomplete",
    )?;
    let topology_bytes =
        serde_json::to_vec(&(nodes.values().collect::<Vec<_>>(), &edges))?.len() as u64;
    let combined = fitted_report(&store, &exact)?;
    let finding_bytes = serde_json::to_vec(&combined.findings)?.len() as u64;
    require(
        combined.work.hydrated_symbols > 0
            && combined.work.hydrated_symbol_bytes > 0
            && combined.work.peak_intermediate_bytes <= exact.relations.budget.intermediate_bytes()
            && combined.work.retained_composition_bytes
                == combined
                    .work
                    .vcs_retained_bytes
                    .saturating_add(topology_bytes)
                    .saturating_add(finding_bytes),
        "VCS and symbol hydration exceeded the shared budget or retained dropped symbol rows",
    )?;
    Ok(())
}

#[test]
fn symbol_hydration_respects_shared_bytes_file_count_and_deadline() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let project = store
        .project_instance_id()?
        .ok_or("project identity missing")?;
    let generation = store
        .repository_graph_generation()?
        .ok_or("generation missing")?;
    let mut nodes = BTreeMap::new();
    for index in 0..65 {
        let file = RepositoryFilePath::new(Path::new(&format!("generated/{index}.rs")))?;
        let entity = GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file,
                    name: GraphIdentityText::new(format!("symbol_{index}"))?,
                    kind: SymbolKind::Function,
                    parent: None,
                    signature: GraphIdentityText::new(format!("fn symbol_{index}()"))?,
                },
            },
            generation,
        )?;
        nodes.insert(
            entity.key().canonical_identity().to_string(),
            DetailedRelationNode {
                entity,
                classification: None,
                content_selection: None,
                purpose: RelationPurpose::Unavailable { path: None },
                coverage: Vec::new(),
            },
        );
    }
    let no_bytes = load_admitted_symbols(&store, &nodes, 0, None)?;
    require(
        !no_bytes.complete
            && no_bytes.rows_retained == 0
            && no_bytes
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes),
        "symbol hydration spent an exhausted shared byte allowance",
    )?;
    let high_file_count = load_admitted_symbols(&store, &nodes, 64 * 1024, None)?;
    require(
        !high_file_count.complete
            && high_file_count
                .reached_limits
                .contains(&GraphLimitKind::Rows),
        "high-file symbol hydration did not stop at its declared file ceiling",
    )?;
    let expired = IndexWorkControl::with_deadline(IndexCancellation::new(), Instant::now());
    let deadline = load_admitted_symbols(&store, &nodes, 64 * 1024, Some(&expired));
    require(
        matches!(
            deadline,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::DeadlineExceeded {
                    stage: IndexWorkStage::RepositoryTraversal
                }
            )))
        ),
        "symbol hydration deadline returned partial rows instead of a typed failure",
    )?;
    Ok(())
}

fn fitted_report(
    store: &AtlasStore,
    query: &RelationAnalysisQuery,
) -> Result<RelationAnalysisReport, Box<dyn Error>> {
    let draft = load_relation_analysis(store, query, None)?;
    let (report, _encoded) = draft.fit_output::<_, ServiceError, _>(|report, _control| {
        serde_json::to_vec(report).map_err(ServiceError::from)
    })?;
    Ok(report)
}

fn analysis_query(mode: RelationAnalysisMode) -> Result<RelationAnalysisQuery, Box<dyn Error>> {
    Ok(RelationAnalysisQuery {
        relations: DetailedRelationQuery {
            anchor: RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            direction: RelationDirection::Outbound,
            relation: None,
            minimum_confidence: ConfidenceClass::Low,
            resolution: RelationResolutionFilter::Resolved,
            include_occurrences: false,
            budget: DetailedRelationBudget::from_graph_limits(
                projectatlas_core::graph::GraphLimits::new(50, 1, 3, 64 * 1024)?,
            )
            .with_aggregate_limits(Some(100), None, None, None, None, None)?,
            cursor: None,
            content_selection: projectatlas_core::language::ContentSelection::UnspecifiedLegacy,
        },
        mode,
        trace_target: None,
        vcs: None,
        include_communities: true,
        include_cycles: true,
        include_dead_code: false,
    })
}

fn exact_symbol_impact_query(
    path: &str,
    name: &str,
    signature: &str,
) -> Result<RelationAnalysisQuery, Box<dyn Error>> {
    let mut query = analysis_query(RelationAnalysisMode::Impact)?;
    query.relations.anchor = RelationAnchor::Symbol {
        file: RepositoryFilePath::new(Path::new(path))?,
        name: name.to_string(),
        symbol_kind: Some(SymbolKind::Function),
        parent: None,
        signature: Some(signature.to_string()),
    };
    query.relations.direction = RelationDirection::Inbound;
    query.relations.relation = None;
    query.relations.minimum_confidence = ConfidenceClass::Low;
    query.relations.resolution = RelationResolutionFilter::Resolved;
    query.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(50, 1, 1, 64 * 1024)?,
    )
    .with_aggregate_limits(Some(100), None, None, None, None, None)?;
    query.vcs = Some(GitImpactSelection::WorkingTree);
    query.include_communities = false;
    query.include_cycles = false;
    query.include_dead_code = true;
    Ok(query)
}

fn initialize_git_fixture(root: &Path) -> Result<(), Box<dyn Error>> {
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "core.autocrlf", "false"],
        vec!["config", "user.email", "projectatlas@example.invalid"],
        vec!["config", "user.name", "ProjectAtlas Tests"],
        vec!["add", "--", "src", "tools"],
        vec!["commit", "--quiet", "-m", "test fixture"],
    ] {
        let status = impact::git_command(root).args(args).status()?;
        require(status.success(), "test Git fixture command failed")?;
    }
    Ok(())
}

fn analysis_store() -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_coverage(true)
}

fn analysis_store_with_coverage(
    include_tools_coverage: bool,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("analysis-service");
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("tools"))?;
    fs::write(root.join("src/a.rs"), "pub fn a() {}\n")?;
    fs::write(root.join("src/b.rs"), "pub fn b() {}\n")?;
    fs::write(root.join("tools/c.rs"), "pub fn c() {}\n")?;
    let database = root.join("projectatlas.db");
    let mut store = AtlasStore::open_for_project(&database, &root)?;
    let project = store
        .project_instance_id()?
        .ok_or("project identity missing")?;
    let generation = IndexGeneration::new(1);
    let entity = |path: &str| {
        GraphEntity::new(
            project,
            EntitySelector::File {
                path: RepositoryFilePath::new(Path::new(path))?,
            },
            generation,
        )
    };
    let a = entity("src/a.rs")?;
    let b = entity("src/b.rs")?;
    let c = entity("tools/c.rs")?;
    let symbol_entity = |path: &str, name: &str, signature: &str| {
        GraphEntity::new(
            project,
            EntitySelector::Symbol {
                symbol: SymbolSelector {
                    file: RepositoryFilePath::new(Path::new(path))?,
                    name: GraphIdentityText::new(name)?,
                    kind: SymbolKind::Function,
                    parent: None,
                    signature: GraphIdentityText::new(signature)?,
                },
            },
            generation,
        )
    };
    let a_long = symbol_entity("src/a.rs", "a_long", "fn a_long()")?;
    let d_unused = symbol_entity("src/a.rs", "d_unused", "fn d_unused()")?;
    let b_hub = symbol_entity("src/b.rs", "b_hub", "fn b_hub()")?;
    let c_aux = symbol_entity("tools/c.rs", "c_aux", "fn c_aux()")?;
    let relation = |source: &GraphEntity, target: &GraphEntity, kind| {
        LogicalRelation::new(
            source,
            kind,
            RelationResolution::resolved(target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )
    };
    let relations = vec![
        relation(&a, &b, GraphRelationKind::Legacy(RelationKind::Calls))?,
        relation(&b, &a, GraphRelationKind::Legacy(RelationKind::Calls))?,
        relation(&a, &c, GraphRelationKind::Legacy(RelationKind::Contains))?,
        relation(
            &c,
            &b,
            GraphRelationKind::Extended(ExtendedRelationKind::References),
        )?,
        relation(&a, &c, GraphRelationKind::Legacy(RelationKind::DependsOn))?,
        relation(
            &a,
            &b,
            GraphRelationKind::Extended(ExtendedRelationKind::References),
        )?,
        relation(
            &a,
            &a_long,
            GraphRelationKind::Legacy(RelationKind::Contains),
        )?,
        relation(
            &b,
            &b_hub,
            GraphRelationKind::Legacy(RelationKind::Contains),
        )?,
        relation(
            &c,
            &c_aux,
            GraphRelationKind::Legacy(RelationKind::Contains),
        )?,
        relation(
            &a_long,
            &b_hub,
            GraphRelationKind::Legacy(RelationKind::Calls),
        )?,
        relation(
            &c_aux,
            &b_hub,
            GraphRelationKind::Legacy(RelationKind::Calls),
        )?,
        LogicalRelation::new(
            &a,
            GraphRelationKind::Extended(ExtendedRelationKind::References),
            RelationResolution::Ambiguous {
                reference: GraphIdentityText::new("ambiguous::target")?,
                candidates: NonZeroU32::new(2).ok_or("candidate count missing")?,
            },
            ConfidenceClass::Medium,
            Completeness::Partial,
            generation,
        )?,
    ];
    let mut coverage = ["src/a.rs", "src/b.rs", "tools/c.rs"]
        .into_iter()
        .filter(|path| include_tools_coverage || *path != "tools/c.rs")
        .map(|path| {
            CoverageRecord::new(
                CoverageScope::Path {
                    path: RepositoryNodePath::new(Path::new(path))?,
                },
                None,
                CoverageState::Complete,
                1,
                0,
                generation,
                None,
                None,
            )
            .map_err(Into::into)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    coverage.extend(
        ["src/a.rs", "src/b.rs", "tools/c.rs"]
            .into_iter()
            .filter(|path| include_tools_coverage || *path != "tools/c.rs")
            .map(|path| {
                CoverageRecord::new(
                    CoverageScope::Path {
                        path: RepositoryNodePath::new(Path::new(path))?,
                    },
                    Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents)),
                    CoverageState::NoCandidates,
                    0,
                    0,
                    generation,
                    None,
                    None,
                )
                .map_err(Into::into)
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
    );
    let mut publication = store.begin_index_publication("analysis-service")?;
    publication.begin_scan_replacement()?;
    publication.upsert_scan_node_batch(&[
        test_folder_node("src"),
        test_folder_node("tools"),
        test_node("src/a.rs", "hash-a"),
        test_node("src/b.rs", "hash-b"),
        test_node_in("tools/c.rs", "tools", "hash-c"),
    ])?;
    publication.finish_scan_replacement()?;
    publication.replace_symbol_graph(&SymbolGraph {
        path: "src/a.rs".to_string(),
        language: Some("rust".to_string()),
        parser: ParserKind::TreeSitter,
        symbols: vec![
            analysis_symbol("src/a.rs", "a_long", "fn a_long()", 1, 30, false),
            analysis_symbol("src/a.rs", "d_unused", "fn d_unused()", 31, 31, false),
        ],
        relations: Vec::new(),
    })?;
    publication.replace_symbol_graph(&SymbolGraph {
        path: "src/b.rs".to_string(),
        language: Some("rust".to_string()),
        parser: ParserKind::TreeSitter,
        symbols: vec![analysis_symbol(
            "src/b.rs",
            "b_hub",
            "fn b_hub()",
            1,
            2,
            false,
        )],
        relations: Vec::new(),
    })?;
    publication.replace_symbol_graph(&SymbolGraph {
        path: "tools/c.rs".to_string(),
        language: Some("rust".to_string()),
        parser: ParserKind::TreeSitter,
        symbols: vec![analysis_symbol(
            "tools/c.rs",
            "c_aux",
            "fn c_aux()",
            1,
            2,
            false,
        )],
        relations: Vec::new(),
    })?;
    publication.replace_repository_graph(
        project,
        &[a, b, c, a_long, d_unused, b_hub, c_aux],
        &relations,
        &[],
        &coverage,
    )?;
    publication.complete()?;
    store.set_purpose("src/a.rs", "负责核心调用", PurposeSource::Agent)?;
    store.set_purpose("src/b.rs", "负责核心调用", PurposeSource::Agent)?;
    store.set_purpose("tools/c.rs", "负责辅助引用", PurposeSource::Agent)?;
    drop(store);
    Ok((
        temp,
        AtlasStore::open_read_only_for_project(&database, &root)?,
    ))
}

fn require(condition: bool, message: &str) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message).into())
    }
}

fn test_node(path: &str, hash: &str) -> Node {
    test_node_in(path, "src", hash)
}

fn analysis_symbol(
    path: &str,
    name: &str,
    signature: &str,
    line_start: usize,
    line_end: usize,
    exported: bool,
) -> CodeSymbol {
    CodeSymbol {
        path: path.to_string(),
        language: Some("rust".to_string()),
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature: signature.to_string(),
        exported,
        documentation: None,
        line_start,
        line_end,
        source_selector: None,
        parent: None,
        parser: ParserKind::TreeSitter,
        detail: Some("function_item".to_string()),
    }
}

fn test_node_in(path: &str, parent: &str, hash: &str) -> Node {
    Node {
        path: path.to_string(),
        kind: NodeKind::File,
        parent_path: Some(parent.to_string()),
        extension: Some(".rs".to_string()),
        language: Some("rust".to_string()),
        size_bytes: Some(16),
        mtime_ns: Some(1),
        content_hash: Some(hash.to_string()),
    }
}

fn test_folder_node(path: &str) -> Node {
    Node {
        path: path.to_string(),
        kind: NodeKind::Folder,
        parent_path: Some(".".to_string()),
        extension: None,
        language: None,
        size_bytes: None,
        mtime_ns: Some(1),
        content_hash: None,
    }
}
