use super::analysis_test_observer::{AnalysisPhaseEvent, observe_analysis_phase};
use super::*;
use projectatlas_core::graph::{
    CoverageRecord, CoverageScope, CoverageState, ExternalSelector, GraphIdentityText,
    LogicalRelation, PackageSelector, RelationResolution, RepositoryFilePath, RepositoryNodePath,
    SymbolSelector,
};
use projectatlas_core::language::ContentClassification;
use projectatlas_core::symbols::{ParserKind, SymbolGraph, SymbolKind};
use projectatlas_core::{IndexGeneration, Node, NodeKind, PurposeSource};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fs;
use std::io;
use std::num::NonZeroU32;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

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
        bounded.truncated
            && bounded.reached_limits.contains(&GraphLimitKind::Edges)
            && !bounded
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes),
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
fn entrypoint_profile_reports_reachable_and_unreachable_without_persistence()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "public-rust".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    let before = store.index_publication()?;
    let report = fitted_report(&store, &query)?;
    let profile = report
        .entrypoint_profile
        .as_ref()
        .ok_or("entrypoint profile metadata missing")?;
    require(
        profile.coverage == EntrypointProfileCoverage::Complete
            && profile.reachable > 0
            && profile.unreachable_candidates > 0,
        "entrypoint profile did not classify a complete reachable and unreachable scope",
    )?;
    require(
        report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::EntrypointReachability
                && finding.status == AnalysisStatus::Confirmed
                && finding.nodes.iter().any(|node| {
                    matches!(
                        node.node.entity.selector(),
                        EntitySelector::Symbol { symbol }
                            if symbol.name.as_str() == "b_hub"
                    )
                })
        }) && !report.findings.iter().any(|finding| {
            finding.status == AnalysisStatus::Candidate
                && finding.nodes.iter().any(|node| {
                    matches!(
                        node.node.entity.selector(),
                        EntitySelector::Symbol { symbol }
                            if symbol.name.as_str() == "b_hub"
                    )
                })
        }),
        "mixed admitted relation families did not preserve the node-simple union",
    )?;
    require(
        report.findings.iter().any(|finding| {
            finding.kind == AnalysisFindingKind::EntrypointReachability
                && finding.status == AnalysisStatus::Candidate
                && finding.nodes.iter().any(|node| {
                    matches!(
                        node.node.entity.selector(),
                        EntitySelector::Symbol { symbol }
                            if symbol.name.as_str() == "d_unused"
                    )
                })
        }),
        "entrypoint profile omitted the unreachable candidate finding",
    )?;
    require(
        store.index_publication()? == before,
        "entrypoint profile changed the authoritative publication",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_rechecks_terminal_frontier_at_exact_edge_limit() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = terminal_entrypoint_store(false)?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.include_communities = false;
    query.include_cycles = false;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(1),
        Some(8),
        Some(8),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "terminal-edge-bound".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let report = fitted_report(&store, &query)?;
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete && profile.reachable == 2
        }) && !report.reached_limits.contains(&GraphLimitKind::Edges),
        "an empty terminal frontier was rejected at the exact edge limit",
    )?;

    let (_temp, store) = terminal_entrypoint_store(true)?;
    let report = fitted_report(&store, &query)?;
    require(
        report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && report.reached_limits.contains(&GraphLimitKind::Edges),
        "a pending terminal edge was not retained as an edge-limit truncation",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_marks_unanchorable_local_targets_inconclusive() -> Result<(), Box<dyn Error>>
{
    let selectors = [
        EntitySelector::Folder {
            path: RepositoryNodePath::new(Path::new("src"))?,
        },
        EntitySelector::Package {
            package: PackageSelector {
                manager: GraphIdentityText::new("cargo")?,
                name: GraphIdentityText::new("analysis-service")?,
                manifest: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
        },
    ];
    for selector in selectors {
        let (_temp, store) = analysis_store_with_target(selector)?;
        let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
        query.relations.resolution = RelationResolutionFilter::Any;
        query.relations.budget = query.relations.budget.with_aggregate_limits(
            Some(100),
            Some(20),
            Some(20),
            Some(100),
            Some(256 * 1024),
            None,
        )?;
        query.include_communities = false;
        query.include_cycles = false;
        query.entrypoint_profile = Some(EntrypointProfile {
            name: "unanchorable-local".to_string(),
            anchors: vec![RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            }],
            relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
        });
        let report = fitted_report(&store, &query)?;
        require(
            report
                .entrypoint_profile
                .as_ref()
                .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
                && report.findings.iter().all(|finding| {
                    finding.kind == AnalysisFindingKind::EntrypointReachability
                        && finding.status == AnalysisStatus::Inconclusive
                })
                && report
                    .entrypoint_profile
                    .as_ref()
                    .is_some_and(|profile| profile.unreachable_candidates == 0),
            "an unanchorable local target produced a confident entrypoint result",
        )?;
    }

    let (_temp, store) = analysis_store()?;
    let project = store
        .project_instance_id()?
        .ok_or("analysis fixture project identity missing")?;
    let project_entity =
        GraphEntity::new(project, EntitySelector::Project, IndexGeneration::new(1))?;
    require(
        RelationResolution::resolved(&project_entity).is_err(),
        "the project aggregate became a directly resolvable entrypoint target",
    )?;

    let (_temp, store) = analysis_store_with_target(EntitySelector::External {
        external: ExternalSelector {
            system: GraphIdentityText::new("crates.io")?,
            identity: GraphIdentityText::new("analysis-service@1")?,
        },
    })?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(20),
        Some(20),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "external-control".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let report = fitted_report(&store, &query)?;
    require(
        report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Complete)
            && report.findings.iter().all(|finding| {
                finding.kind != AnalysisFindingKind::EntrypointReachability
                    || finding.status != AnalysisStatus::Inconclusive
            }),
        "an external resolved target incorrectly made complete entrypoint coverage partial",
    )?;

    let (_temp, store) = analysis_store_with_external_candidate()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(20),
        Some(20),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "external-candidate-control".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let report = fitted_report(&store, &query)?;
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete
                && profile.unreachable_candidates > 0
        }) && report.findings.iter().any(|finding| {
            finding.status == AnalysisStatus::Candidate
                && finding.nodes.iter().any(|node| {
                    matches!(
                        node.node.entity.selector(),
                        EntitySelector::Symbol { symbol }
                            if symbol.name.as_str() == "d_unused"
                    )
                })
        }),
        "an external candidate relation did not remain valid negative evidence",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_rejects_candidate_generation_and_purpose_changes()
-> Result<(), Box<dyn Error>> {
    let candidate_query = || -> Result<RelationAnalysisQuery, Box<dyn Error>> {
        let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
        query.relations.resolution = RelationResolutionFilter::Any;
        query.relations.budget = query.relations.budget.with_aggregate_limits(
            Some(100),
            Some(20),
            Some(20),
            Some(100),
            Some(256 * 1024),
            None,
        )?;
        query.include_communities = false;
        query.include_cycles = false;
        query.entrypoint_profile = Some(EntrypointProfile {
            name: "candidate-stale".to_string(),
            anchors: vec![RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            }],
            relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
        });
        Ok(query)
    };

    let (temp, stale_store) = analysis_store()?;
    let root = temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    stale_store.finish_index_read_snapshot()?;
    let writer = Rc::new(RefCell::new(Some(AtlasStore::open_for_project(
        &database, &root,
    )?)));
    let refreshed = Rc::new(Cell::new(false));
    let writer_for_observer = Rc::clone(&writer);
    let refreshed_for_observer = Rc::clone(&refreshed);
    let generation_query = candidate_query()?;
    let generation_stale = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::CandidateTraversal
                && !refreshed_for_observer.replace(true)
                && let Some(mut writer) = writer_for_observer.borrow_mut().take()
                && let Ok(refresh) = writer.begin_index_projection_refresh("analysis-service")
            {
                drop(refresh.complete());
            }
        },
        || load_relation_analysis(&stale_store, &generation_query, None),
    );
    if !(refreshed.get()
        && generation_stale
            .as_ref()
            .err()
            .is_some_and(|error| error.to_string().contains("typed graph generation")))
    {
        return Err(io::Error::other(format!(
            "candidate generation transition was not refused: refreshed={}, error={:?}",
            refreshed.get(),
            generation_stale.as_ref().err().map(ToString::to_string)
        ))
        .into());
    }

    let (temp, stale_store) = analysis_store()?;
    let root = temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    stale_store.finish_index_read_snapshot()?;
    let writer = Rc::new(RefCell::new(Some(AtlasStore::open_for_project(
        &database, &root,
    )?)));
    let revised = Rc::new(Cell::new(false));
    let writer_for_observer = Rc::clone(&writer);
    let revised_for_observer = Rc::clone(&revised);
    let purpose_query = candidate_query()?;
    let purpose_stale = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::CandidateTraversal
                && !revised_for_observer.replace(true)
                && let Some(writer) = writer_for_observer.borrow_mut().take()
            {
                drop(writer.set_purpose(
                    "src/a.rs",
                    "purpose changed during analysis",
                    PurposeSource::Agent,
                ));
            }
        },
        || load_relation_analysis(&stale_store, &purpose_query, None),
    );
    require(
        revised.get()
            && matches!(
                purpose_stale,
                Err(ServiceError::RelationCursorStale {
                    field: "entrypoint authored purpose revision"
                })
            ),
        "candidate purpose revision transition was not refused at the candidate boundary",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_rejects_generation_change_after_empty_candidate_page()
-> Result<(), Box<dyn Error>> {
    let (temp, stale_store) = analysis_store()?;
    let root = temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    stale_store.finish_index_read_snapshot()?;
    let writer = Rc::new(RefCell::new(Some(AtlasStore::open_for_project(
        &database, &root,
    )?)));
    let refreshed = Rc::new(Cell::new(false));
    let writer_for_observer = Rc::clone(&writer);
    let refreshed_for_observer = Rc::clone(&refreshed);
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.content_selection = projectatlas_core::language::ContentSelection::Source;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(30),
        Some(30),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "empty-candidate-page-stale".to_string(),
        anchors: vec![
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("tools/c.rs"))?,
            },
            RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                name: "a_long".to_string(),
                symbol_kind: Some(SymbolKind::Function),
                parent: None,
                signature: Some("fn a_long()".to_string()),
            },
            RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                name: "d_unused".to_string(),
                symbol_kind: Some(SymbolKind::Function),
                parent: None,
                signature: Some("fn d_unused()".to_string()),
            },
            RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
                name: "b_hub".to_string(),
                symbol_kind: Some(SymbolKind::Function),
                parent: None,
                signature: Some("fn b_hub()".to_string()),
            },
            RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("tools/c.rs"))?,
                name: "c_aux".to_string(),
                symbol_kind: Some(SymbolKind::Function),
                parent: None,
                signature: Some("fn c_aux()".to_string()),
            },
        ],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    let stale = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::CandidateEnumeration
                && !refreshed_for_observer.replace(true)
                && let Some(mut writer) = writer_for_observer.borrow_mut().take()
                && let Ok(refresh) = writer.begin_index_projection_refresh("analysis-service")
            {
                drop(refresh.complete());
            }
        },
        || load_relation_analysis(&stale_store, &query, None),
    );
    require(
        refreshed.get()
            && stale
                .as_ref()
                .err()
                .is_some_and(|error| error.to_string().contains("typed graph generation")),
        "empty candidate enumeration did not reject a generation transition",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_does_not_use_disabled_occurrences_as_row_budget() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store_with_external_candidate()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(20),
        Some(20),
        Some(1),
        Some(1024 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "disabled-occurrence-row-budget".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    let report = fitted_report(&store, &query)?;
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete
                && profile.unreachable_candidates > 0
        }) && !report.reached_limits.contains(&GraphLimitKind::Rows)
            && !report.reached_limits.contains(&GraphLimitKind::Occurrences),
        "disabled occurrence collection incorrectly constrained entrypoint relation rows",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_inspects_retained_frontier_at_exact_node_limit() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.content_selection = projectatlas_core::language::ContentSelection::Source;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(7),
        Some(7),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "retained-frontier-at-node-limit".to_string(),
        anchors: vec![
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            RelationAnchor::Symbol {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                name: "d_unused".to_string(),
                symbol_kind: Some(SymbolKind::Function),
                parent: None,
                signature: Some("fn d_unused()".to_string()),
            },
        ],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    let report = fitted_report(&store, &query)?;
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete
                && profile.unreachable_candidates == 0
        }) && !report.reached_limits.contains(&GraphLimitKind::Nodes)
            && !report.reached_limits.contains(&GraphLimitKind::Visited),
        "a retained frontier anchor was refused at the exact node limit",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_reuses_retained_candidate_endpoints() -> Result<(), Box<dyn Error>> {
    for (target, expected_coverage) in [
        ("reachable", EntrypointProfileCoverage::Complete),
        ("new", EntrypointProfileCoverage::Partial),
    ] {
        let (_temp, store) = analysis_store_with_candidate_relation(target)?;
        let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
        query.relations.content_selection = ContentSelection::Source;
        query.relations.resolution = RelationResolutionFilter::Any;
        query.relations.budget = query.relations.budget.with_aggregate_limits(
            Some(100),
            Some(7),
            Some(7),
            Some(100),
            Some(256 * 1024),
            None,
        )?;
        query.include_communities = false;
        query.include_cycles = false;
        query.entrypoint_profile = Some(EntrypointProfile {
            name: format!("candidate-endpoint-{target}"),
            anchors: vec![RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            }],
            relations: vec![
                GraphRelationKind::Legacy(RelationKind::Calls),
                GraphRelationKind::Legacy(RelationKind::Contains),
                GraphRelationKind::Legacy(RelationKind::DependsOn),
            ],
        });
        let report = fitted_report(&store, &query)?;
        let profile = report
            .entrypoint_profile
            .as_ref()
            .ok_or("candidate endpoint profile metadata missing")?;
        if expected_coverage == EntrypointProfileCoverage::Complete {
            require(
                profile.coverage == expected_coverage
                    && profile.unreachable_candidates == 1
                    && !report.reached_limits.contains(&GraphLimitKind::Nodes)
                    && !report.reached_limits.contains(&GraphLimitKind::Visited),
                "a retained candidate endpoint consumed the global node or visited slot",
            )?;
        } else {
            require(
                profile.coverage == expected_coverage
                    && profile.unreachable_candidates == 0
                    && report.reached_limits.contains(&GraphLimitKind::Nodes)
                    && report.reached_limits.contains(&GraphLimitKind::Visited)
                    && report
                        .findings
                        .iter()
                        .all(|finding| finding.status == AnalysisStatus::Inconclusive),
                "a new candidate endpoint escaped the global node or visited bound",
            )?;
        }
    }
    Ok(())
}

#[test]
fn entrypoint_profile_reconciles_candidate_node_and_visited_limits_independently()
-> Result<(), Box<dyn Error>> {
    for (nodes, visited, expected_limit, unexpected_limit) in [
        (8, 7, GraphLimitKind::Visited, GraphLimitKind::Nodes),
        (7, 8, GraphLimitKind::Nodes, GraphLimitKind::Visited),
    ] {
        let (_temp, store) = analysis_store_with_candidate_relation("new")?;
        let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
        query.relations.content_selection = ContentSelection::Source;
        query.relations.resolution = RelationResolutionFilter::Any;
        query.relations.budget = query.relations.budget.with_aggregate_limits(
            Some(100),
            Some(nodes),
            Some(visited),
            Some(100),
            Some(256 * 1024),
            None,
        )?;
        query.include_communities = false;
        query.include_cycles = false;
        query.entrypoint_profile = Some(EntrypointProfile {
            name: format!("candidate-asymmetric-{nodes}-{visited}"),
            anchors: vec![RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            }],
            relations: vec![
                GraphRelationKind::Legacy(RelationKind::Calls),
                GraphRelationKind::Legacy(RelationKind::Contains),
                GraphRelationKind::Legacy(RelationKind::DependsOn),
            ],
        });
        let report = fitted_report(&store, &query)?;
        require(
            report.entrypoint_profile.as_ref().is_some_and(|profile| {
                profile.coverage == EntrypointProfileCoverage::Partial
                    && profile.unreachable_candidates == 0
            }) && report.reached_limits.contains(&expected_limit)
                && !report.reached_limits.contains(&unexpected_limit),
            "candidate validation did not reconcile node and visited limits independently",
        )?;
        require(
            report
                .findings
                .iter()
                .all(|finding| finding.status == AnalysisStatus::Inconclusive),
            "asymmetric candidate limit retained a confirmed entrypoint finding",
        )?;
    }
    Ok(())
}

#[test]
fn entrypoint_profile_scopes_coverage_to_admitted_relations() -> Result<(), Box<dyn Error>> {
    let run = |partial_calls| -> Result<RelationAnalysisReport, Box<dyn Error>> {
        let (_temp, store) = analysis_store_with_relation_coverage(partial_calls)?;
        let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
        query.relations.resolution = RelationResolutionFilter::Any;
        query.relations.budget = query.relations.budget.with_aggregate_limits(
            Some(100),
            Some(20),
            Some(20),
            Some(100),
            Some(256 * 1024),
            None,
        )?;
        query.include_communities = false;
        query.include_cycles = false;
        query.entrypoint_profile = Some(EntrypointProfile {
            name: "calls-coverage-scope".to_string(),
            anchors: vec![RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            }],
            relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
        });
        fitted_report(&store, &query)
    };

    let complete = run(false)?;
    require(
        complete
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Complete),
        "unrelated partial Documents coverage poisoned a Calls-only profile",
    )?;
    let partial = run(true)?;
    require(
        partial
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && partial.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "partial admitted Calls coverage did not remain inconclusive",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_drops_nodes_from_overflowing_retained_frontier() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.content_selection = projectatlas_core::language::ContentSelection::Source;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(5),
        Some(5),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "overflowing-retained-frontier".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    let report = fitted_report(&store, &query)?;
    let c_aux_present = report.findings.iter().any(|finding| {
        finding.nodes.iter().any(|node| {
            matches!(
                node.node.entity.selector(),
                EntitySelector::Symbol { symbol } if symbol.name.as_str() == "c_aux"
            )
        })
    });
    require(
        report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && report.reached_limits.contains(&GraphLimitKind::Nodes)
            && report
                .findings
                .iter()
                .all(|finding| finding.status == AnalysisStatus::Inconclusive)
            && !c_aux_present,
        "a node discovered beyond the retained frontier budget was published",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_bounds_candidate_entity_hydration_by_remaining_bytes()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(20),
        Some(20),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-byte-budget".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let remaining = Rc::new(Cell::new(None));
    let remaining_for_observer = Rc::clone(&remaining);
    let report = observe_analysis_phase(
        move |event| {
            if let AnalysisPhaseEvent::CandidateEntityHydration {
                remaining_intermediate_bytes,
            } = event
            {
                remaining_for_observer.set(Some(remaining_intermediate_bytes));
            }
        },
        || load_relation_analysis(&store, &query, None),
    )?;
    let remaining = remaining
        .get()
        .ok_or("candidate entity hydration did not expose its remaining budget")?;
    require(
        remaining < query.relations.budget.intermediate_bytes()
            && report.report.work.relations.intermediate_bytes
                <= query.relations.budget.intermediate_bytes(),
        "candidate entity hydration did not consume only the remaining profile budget",
    )?;

    let mut bounded_query = query;
    bounded_query.relations.budget = bounded_query.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(64 * 1024),
        None,
    )?;
    let bounded_seen = Rc::new(Cell::new(false));
    let bounded_seen_for_observer = Rc::clone(&bounded_seen);
    let bounded_report = observe_analysis_phase(
        move |event| {
            if matches!(event, AnalysisPhaseEvent::CandidateEntityHydration { .. }) {
                bounded_seen_for_observer.set(true);
            }
        },
        || load_relation_analysis(&store, &bounded_query, None),
    )?;
    require(
        !bounded_seen.get()
            && bounded_report
                .report
                .entrypoint_profile
                .as_ref()
                .is_some_and(|profile| {
                    profile.coverage == EntrypointProfileCoverage::Partial
                        && profile.unreachable_candidates == 0
                })
            && bounded_report
                .report
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes)
            && bounded_report.report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "a byte-limited candidate scope produced confident reachability output",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_translates_candidate_decoded_byte_exhaustion() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store_with_large_candidates()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(30),
        Some(30),
        Some(100),
        Some(DetailedRelationBudget::MAX_INTERMEDIATE_BYTES),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-decoded-byte-boundary".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });

    let remaining = Rc::new(Cell::new(None));
    let remaining_for_observer = Rc::clone(&remaining);
    observe_analysis_phase(
        move |event| {
            if let AnalysisPhaseEvent::CandidateEntityHydration {
                remaining_intermediate_bytes,
            } = event
            {
                remaining_for_observer.set(Some(remaining_intermediate_bytes));
            }
        },
        || load_relation_analysis(&store, &query, None),
    )?;
    let remaining = remaining
        .get()
        .ok_or("candidate entity hydration did not expose its remaining budget")?;
    let pre_candidate_work = query
        .relations
        .budget
        .intermediate_bytes()
        .checked_sub(remaining)
        .ok_or("candidate byte probe exceeded its budget")?;
    let bounded_budget = pre_candidate_work
        .checked_add(64 * 1024)
        .ok_or("candidate byte boundary overflowed")?;
    let mut bounded_query = query;
    bounded_query.relations.budget = bounded_query.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(bounded_budget),
        None,
    )?;
    let report = load_relation_analysis(&store, &bounded_query, None)?.report;
    require(
        remaining > 0
            && report.entrypoint_profile.as_ref().is_some_and(|profile| {
                profile.coverage == EntrypointProfileCoverage::Partial
                    && profile.unreachable_candidates == 0
            })
            && report
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes)
            && report
                .findings
                .iter()
                .all(|finding| finding.status == AnalysisStatus::Inconclusive)
            && report.work.peak_intermediate_bytes <= bounded_budget,
        "candidate decoded-byte exhaustion escaped as an error or exceeded its budget",
    )?;
    Ok(())
}

#[test]
fn entrypoint_candidate_page_filters_content_before_truncation_sentinel()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store_with_external_candidate()?;
    let project = store
        .project_instance_id()?
        .ok_or("analysis fixture project identity missing")?;
    let generation = store
        .repository_graph_generation()?
        .ok_or("analysis fixture graph generation missing")?;
    let page = store.repository_graph_entrypoint_candidates_page_bounded(
        project,
        generation,
        7,
        ContentSelection::Source,
        RepositoryGraphReadBudget::new(1, 7, 256 * 1024, 16, 16)?,
        None,
    )?;
    require(
        !page.page.truncated
            && page.page.rows.len() == 7
            && page.page.rows.iter().all(|entity| {
                !matches!(
                    entity.selector(),
                    EntitySelector::File { path } if path.as_str() == "docs/guide.md"
                )
            }),
        "content filtering was applied after the candidate-page sentinel",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_honors_content_and_confidence_filters() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut source_only = analysis_query(RelationAnalysisMode::Entrypoint)?;
    source_only.relations.resolution = RelationResolutionFilter::Any;
    source_only.relations.content_selection = ContentSelection::Source;
    source_only.include_communities = false;
    source_only.include_cycles = false;
    source_only.entrypoint_profile = Some(EntrypointProfile {
        name: "source-only".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let source_report = fitted_report(&store, &source_only)?;
    require(
        source_report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Complete)
            && source_report.findings.iter().all(|finding| {
                finding.nodes.iter().all(|node| {
                    !matches!(
                        node.node.entity.selector(),
                        EntitySelector::File { path } if path.as_str() == "docs/guide.md"
                    )
                })
            }),
        "source-only entrypoint profile admitted a documentation candidate",
    )?;

    let mut exact = analysis_query(RelationAnalysisMode::Entrypoint)?;
    exact.relations.resolution = RelationResolutionFilter::Any;
    exact.relations.minimum_confidence = ConfidenceClass::Exact;
    exact.include_communities = false;
    exact.include_cycles = false;
    exact.entrypoint_profile = Some(EntrypointProfile {
        name: "exact-confidence".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )],
    });
    let exact_report = fitted_report(&store, &exact)?;
    let mut low = exact;
    low.relations.minimum_confidence = ConfidenceClass::Low;
    let low_report = fitted_report(&store, &low)?;
    require(
        exact_report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Complete)
            && low_report
                .entrypoint_profile
                .as_ref()
                .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && exact_report
                .findings
                .iter()
                .all(|finding| finding.status != AnalysisStatus::Inconclusive)
            && low_report
                .findings
                .iter()
                .all(|finding| finding.status == AnalysisStatus::Inconclusive),
        "entrypoint profile did not preserve the requested minimum confidence",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_keeps_cross_class_document_targets_out_of_frontier()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store_with_document_relation()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.content_selection = ContentSelection::Documentation;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget = query.relations.budget.with_aggregate_limits(
        Some(100),
        Some(20),
        Some(20),
        Some(100),
        Some(256 * 1024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "documentation-cross-class-frontier".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("docs/guide.md"))?,
        }],
        relations: vec![GraphRelationKind::Extended(ExtendedRelationKind::Documents)],
    });
    let report = fitted_report(&store, &query)?;
    let source_target_present = report.findings.iter().any(|finding| {
        finding.nodes.iter().any(|node| {
            matches!(
                node.node.entity.selector(),
                EntitySelector::File { path } if path.as_str() == "src/a.rs"
            )
        })
    });
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete && profile.reachable == 1
        }) && !report.reached_limits.contains(&GraphLimitKind::Depth)
            && !source_target_present,
        "documentation entrypoint traversal admitted a cross-class source target",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_filters_non_candidate_entities_before_node_limit()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store_with_external_candidate()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.content_selection = ContentSelection::Source;
    query.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(50, 1, 3, 256 * 1_024)?,
    )
    .with_aggregate_limits(
        Some(100),
        Some(8),
        Some(100),
        Some(100),
        Some(256 * 1_024),
        None,
    )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-page-filter".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });

    let report = fitted_report(&store, &query)?;
    require(
        report.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Complete
                && profile.unreachable_candidates > 0
        }) && !report.reached_limits.contains(&GraphLimitKind::Nodes),
        "non-candidate graph entities consumed the entrypoint candidate limit",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_falls_back_before_exceeding_composition_budget() -> Result<(), Box<dyn Error>>
{
    let (temp, initial_store) = analysis_store()?;
    let root = temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    drop(initial_store);
    let writable = AtlasStore::open_for_project(&database, &root)?;
    writable.set_purpose(
        "src/a.rs",
        &format!("composition boundary {}", "x".repeat(20_000)),
        PurposeSource::Agent,
    )?;
    drop(writable);
    let store = AtlasStore::open_read_only_for_project(&database, &root)?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.relations.budget =
        DetailedRelationBudget::from_graph_limits(GraphLimits::new(50, 1, 3, 1024 * 1024)?)
            .with_aggregate_limits(
                None,
                None,
                None,
                None,
                Some(DetailedRelationBudget::MAX_INTERMEDIATE_BYTES),
                None,
            )?;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "composition-boundary".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });

    let complete = load_relation_analysis(&store, &query, None)?;
    let complete_profile = complete
        .report
        .entrypoint_profile
        .as_ref()
        .ok_or("complete entrypoint profile missing")?;
    let complete_composition =
        serialized_bytes_controlled(&(&complete.report.findings, complete_profile), None)?;
    let mut fallback_findings = complete.report.findings.clone();
    for finding in &mut fallback_findings {
        finding.status = AnalysisStatus::Inconclusive;
        finding.nodes.clear();
    }
    let mut fallback_profile = complete_profile.clone();
    fallback_profile.coverage = EntrypointProfileCoverage::Partial;
    fallback_profile.unreachable_candidates = 0;
    let fallback_composition =
        serialized_bytes_controlled(&(&fallback_findings, &fallback_profile), None)?;
    require(
        complete_profile.coverage == EntrypointProfileCoverage::Complete
            && complete_profile.unreachable_candidates > 0
            && complete_composition > fallback_composition,
        "entrypoint fixture did not produce a larger complete composition",
    )?;
    let boundary_budget = complete
        .report
        .work
        .peak_intermediate_bytes
        .saturating_sub(1);
    let mut boundary_query = query;
    boundary_query.relations.budget = boundary_query.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(boundary_budget),
        None,
    )?;
    let boundary = load_relation_analysis(&store, &boundary_query, None)?.report;
    require(
        boundary.entrypoint_profile.as_ref().is_some_and(|profile| {
            profile.coverage == EntrypointProfileCoverage::Partial
                && profile.unreachable_candidates == 0
        }) && boundary.work.composition_truncated
            && boundary
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes)
            && boundary
                .entrypoint_profile
                .as_ref()
                .is_some_and(|profile| profile.reachable == 0)
            && boundary.findings.iter().all(|finding| {
                finding.status == AnalysisStatus::Inconclusive
                    && finding.metric.is_none()
                    && finding.nodes.is_empty()
            })
            && boundary.work.peak_intermediate_bytes <= boundary_budget,
        "entrypoint composition fallback exceeded its declared byte budget",
    )?;
    Ok(())
}

#[test]
fn entrypoint_classification_hydration_honors_control_and_byte_budget() -> Result<(), Box<dyn Error>>
{
    let (_temp, store) = analysis_store()?;
    let project = store
        .project_instance_id()?
        .ok_or("analysis fixture project identity missing")?;
    let generation = store
        .repository_graph_generation()?
        .ok_or("analysis fixture graph generation missing")?;
    let entity = GraphEntity::new(
        project,
        EntitySelector::File {
            path: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        },
        generation,
    )?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.content_selection = ContentSelection::Source;
    let budget = query.relations.budget;

    let cancellation = IndexCancellation::new();
    let control = IndexWorkControl::with_deadline(
        cancellation.clone(),
        Instant::now() + Duration::from_secs(5),
    );
    let cancelled = cancel_at_analysis_phase(
        AnalysisPhaseEvent::ClassificationHydration,
        cancellation,
        || {
            super::load_entrypoint_candidate_classifications(
                &store,
                std::slice::from_ref(&entity),
                budget,
                &mut DetailedRelationWork::default(),
                Some(&control),
            )
        },
    )?;
    require(
        matches!(
            cancelled,
            Err(ServiceError::Db(DbError::IndexWork(
                projectatlas_core::IndexWorkFailure::Cancelled { .. }
            )))
        ),
        "classification hydration ignored cancellation",
    )?;

    let mut work = DetailedRelationWork {
        intermediate_bytes: budget
            .intermediate_bytes()
            .saturating_sub(super::classification_path_bytes("src/a.rs")?),
        ..DetailedRelationWork::default()
    };
    let bounded = super::load_entrypoint_candidate_classifications(
        &store,
        std::slice::from_ref(&entity),
        budget,
        &mut work,
        None,
    )?;
    require(
        bounded.is_none(),
        "classification hydration accepted data beyond the intermediate-byte budget",
    )?;
    require(
        work.database_requested_rows == 0
            && work.database_returned_rows == 0
            && work.database_decoded_bytes == 0
            && work.hydrated_classification_paths == 0
            && work.intermediate_bytes == budget.intermediate_bytes(),
        "classification hydration materialized a batch beyond its remaining budget",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_rejects_ambiguous_cursor_and_wrong_scope_and_stays_inconclusive()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let file_anchor = |path: &str| {
        Ok::<_, Box<dyn Error>>(RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new(path))?,
        })
    };
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "invalid".to_string(),
        anchors: vec![file_anchor("src/a.rs")?, file_anchor("src/a.rs")?],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    require(
        load_relation_analysis(&store, &query, None).is_err(),
        "duplicate entrypoint anchors were accepted",
    )?;

    query.entrypoint_profile = Some(EntrypointProfile {
        name: "wrong-root".to_string(),
        anchors: vec![file_anchor("missing.rs")?],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    require(
        load_relation_analysis(&store, &query, None).is_err(),
        "a wrong-root entrypoint was accepted",
    )?;

    query.entrypoint_profile = Some(EntrypointProfile {
        name: "cursor".to_string(),
        anchors: vec![file_anchor("src/a.rs")?],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    query.relations.cursor = Some("stale".to_string());
    let cursor_result = load_relation_analysis(&store, &query, None);
    let cursor_error = cursor_result.err().map(|error| error.to_string());
    require(
        cursor_error
            .as_deref()
            .is_some_and(|error| error.contains("relation cursors")),
        "entrypoint profiles accepted a replay cursor",
    )?;

    let (_partial_temp, partial_store) = analysis_store_with_coverage(false)?;
    query.relations.cursor = None;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "partial".to_string(),
        anchors: vec![file_anchor("tools/c.rs")?],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    let report = fitted_report(&partial_store, &query)?;
    require(
        report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "incomplete entrypoint coverage produced a confident finding",
    )?;
    Ok(())
}

#[test]
fn entrypoint_profile_refuses_unreplayable_output_and_charges_shared_limits()
-> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Entrypoint)?;
    query.relations.resolution = RelationResolutionFilter::Any;
    query.include_communities = false;
    query.include_cycles = false;
    query.entrypoint_profile = Some(EntrypointProfile {
        name: "bounded".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });

    let Err(output_error) = load_relation_analysis(&store, &query, None)?
        .fit_output::<_, ServiceError, _>(|report, _control| {
            let _ = report;
            Ok(vec![0_u8; 65_537])
        })
    else {
        return Err("entrypoint output unexpectedly accepted a non-replayable prefix".into());
    };
    require(
        output_error
            .to_string()
            .contains("entrypoint profile output"),
        "entrypoint output refusal did not identify the typed output boundary",
    )?;

    let mut oversized_construction = load_relation_analysis(&store, &query, None)?;
    let construction_budget = oversized_construction.budget.intermediate_bytes();
    oversized_construction.report.work.peak_intermediate_bytes = construction_budget + 1;
    let Err(construction_error) =
        oversized_construction.fit_output::<_, ServiceError, _>(|report, _control| {
            serde_json::to_vec(report).map_err(ServiceError::from)
        })
    else {
        return Err("entrypoint output fitting ignored construction work".into());
    };
    require(
        construction_error
            .to_string()
            .contains("aggregate intermediate-byte budget"),
        "entrypoint output fitting did not preserve the construction peak",
    )?;

    let mut uncertain = query.clone();
    uncertain.entrypoint_profile = Some(EntrypointProfile {
        name: "dynamic-reference".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )],
    });
    let uncertain_report = fitted_report(&store, &uncertain)?;
    require(
        uncertain_report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && uncertain_report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "ambiguous or dynamic references became a confident entrypoint result",
    )?;

    let mut over_budget = query.clone();
    over_budget.entrypoint_profile = Some(EntrypointProfile {
        name: "over-budget".to_string(),
        anchors: vec![
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
        ],
        relations: vec![GraphRelationKind::Legacy(RelationKind::Calls)],
    });
    over_budget.relations.budget = over_budget.relations.budget.with_aggregate_limits(
        Some(10),
        Some(1),
        Some(1),
        None,
        Some(256 * 1_024),
        None,
    )?;
    let preflight_seen = Rc::new(Cell::new(false));
    let preflight_seen_observer = Rc::clone(&preflight_seen);
    let over_budget_result = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::Traversal {
                preflight_seen_observer.set(true);
            }
        },
        || load_relation_analysis(&store, &over_budget, None),
    );
    let preflight_error = over_budget_result.as_ref().err().map(ToString::to_string);
    require(
        !preflight_seen.get()
            && preflight_error
                .as_deref()
                .is_some_and(|error| error.contains("anchor count exceeds the node budget")),
        "over-budget entrypoint anchors entered traversal before typed rejection",
    )?;

    let publication_before_cancel = store.index_publication()?;
    let cancellation = IndexCancellation::new();
    let cancel_seen = Rc::new(Cell::new(false));
    let cancel_seen_observer = Rc::clone(&cancel_seen);
    let cancellation_for_observer = cancellation.clone();
    let cancel_control = IndexWorkControl::with_deadline(
        cancellation,
        Instant::now() + std::time::Duration::from_secs(5),
    );
    let cancelled = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::Traversal && !cancel_seen_observer.replace(true) {
                cancellation_for_observer.cancel();
            }
        },
        || load_relation_analysis(&store, &query, Some(&cancel_control)),
    );
    require(
        cancel_seen.get()
            && matches!(
                cancelled,
                Err(ServiceError::Db(DbError::IndexWork(
                    projectatlas_core::IndexWorkFailure::Cancelled { .. }
                )))
            )
            && store.index_publication()? == publication_before_cancel,
        "cancelled entrypoint traversal returned a partial result or changed publication",
    )?;

    let (stale_temp, stale_store) = analysis_store()?;
    let root = stale_temp.path().join("analysis-service");
    let database = root.join("projectatlas.db");
    stale_store.finish_index_read_snapshot()?;
    let writer = Rc::new(RefCell::new(Some(AtlasStore::open_for_project(
        &database, &root,
    )?)));
    let refreshed = Rc::new(Cell::new(false));
    let refresh_succeeded = Rc::new(Cell::new(false));
    let writer_for_observer = Rc::clone(&writer);
    let refreshed_for_observer = Rc::clone(&refreshed);
    let refresh_succeeded_for_observer = Rc::clone(&refresh_succeeded);
    let stale = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::Traversal && !refreshed_for_observer.replace(true) {
                let succeeded = if let Some(mut writer) = writer_for_observer.borrow_mut().take() {
                    match writer.begin_index_projection_refresh("analysis-service") {
                        Ok(refresh) => refresh.complete().is_ok(),
                        Err(_) => false,
                    }
                } else {
                    false
                };
                refresh_succeeded_for_observer.set(succeeded);
            }
        },
        || load_relation_analysis(&stale_store, &query, None),
    );
    let stale_error = stale.as_ref().err().map(ToString::to_string);
    require(
        refreshed.get()
            && refresh_succeeded.get()
            && stale_error
                .as_deref()
                .is_some_and(|error| error.contains("typed graph generation")),
        "entrypoint traversal did not reject a generation transition",
    )?;

    let mut cycle = query.clone();
    cycle.entrypoint_profile = Some(EntrypointProfile {
        name: "cycle".to_string(),
        anchors: vec![
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
            },
            RelationAnchor::File {
                file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
            },
        ],
        relations: vec![
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ],
    });
    cycle.relations.budget = cycle.relations.budget.with_aggregate_limits(
        Some(10),
        Some(20),
        Some(10),
        Some(20),
        Some(256 * 1_024),
        None,
    )?;
    let cycle_report = fitted_report(&store, &cycle)?;
    if !(cycle_report
        .entrypoint_profile
        .as_ref()
        .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Complete)
        && cycle_report.work.relations.inspected_edges == 9
        && !cycle_report.reached_limits.contains(&GraphLimitKind::Edges))
    {
        return Err(io::Error::other(format!(
            "a tight cyclic profile re-expanded an original anchor: coverage={:?}, limits={:?}, work={:?}",
            cycle_report.entrypoint_profile.as_ref().map(|profile| profile.coverage),
            cycle_report.reached_limits,
            cycle_report.work.relations,
        ))
        .into());
    }
    let cycle_replay = fitted_report(&store, &cycle)?;
    require(
        cycle_report == cycle_replay,
        "identical multi-anchor entrypoint requests were not deterministic",
    )?;

    let mut bounded = query.clone();
    bounded.relations.budget = bounded.relations.budget.with_aggregate_limits(
        Some(2),
        Some(4),
        Some(4),
        Some(10),
        Some(64 * 1_024),
        None,
    )?;
    let mut candidate_limits = query.clone();
    candidate_limits.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(50, 1, 3, 256 * 1_024)?,
    )
    .with_aggregate_limits(
        Some(1),
        Some(100),
        Some(100),
        Some(100),
        Some(256 * 1_024),
        None,
    )?;
    candidate_limits.relations.content_selection = ContentSelection::Source;
    candidate_limits.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-limit-reason".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/b.rs"))?,
        }],
        relations: vec![GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )],
    });
    let candidate_limit_report = fitted_report(&store, &candidate_limits)?;
    require(
        candidate_limit_report
            .reached_limits
            .contains(&GraphLimitKind::Edges),
        "candidate traversal did not preserve its typed edge limit reason",
    )?;

    let report = fitted_report(&store, &bounded)?;
    let profile = report
        .entrypoint_profile
        .as_ref()
        .ok_or("bounded entrypoint profile metadata missing")?;
    require(
        profile.coverage == EntrypointProfileCoverage::Partial
            && report.truncated
            && report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "shared entrypoint limits produced confident or complete output",
    )?;
    require(
        report.work.relations.inspected_edges <= 2
            && report.work.analyzed_nodes <= 4
            && report.work.relations.visited_nodes <= 4
            && report.work.peak_intermediate_bytes <= 64 * 1_024,
        "entrypoint traversal crossed a caller aggregate ceiling",
    )?;

    let mut candidate_continuation = query.clone();
    candidate_continuation.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(1, 1, 3, 256 * 1_024)?,
    )
    .with_aggregate_limits(
        Some(100),
        Some(100),
        Some(100),
        Some(100),
        Some(256 * 1_024),
        None,
    )?;
    candidate_continuation.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-continuation".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("docs/guide.md"))?,
        }],
        relations: vec![GraphRelationKind::Extended(
            ExtendedRelationKind::References,
        )],
    });
    let candidate_continuation_seen = Rc::new(Cell::new(false));
    let candidate_continuation_observer = Rc::clone(&candidate_continuation_seen);
    let continuation_report = observe_analysis_phase(
        move |event| {
            if let AnalysisPhaseEvent::CandidateReport {
                has_continuation: true,
                ..
            } = event
            {
                candidate_continuation_observer.set(true);
            }
        },
        || fitted_report(&store, &candidate_continuation),
    )?;
    require(
        candidate_continuation_seen.get()
            && continuation_report
                .reached_limits
                .contains(&GraphLimitKind::Rows)
            && continuation_report
                .entrypoint_profile
                .as_ref()
                .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && continuation_report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "candidate continuation did not preserve the outer typed row limit",
    )?;

    let mut candidate_visited = query;
    candidate_visited.entrypoint_profile = Some(EntrypointProfile {
        name: "candidate-visited".to_string(),
        anchors: vec![RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new("src/a.rs"))?,
        }],
        relations: vec![GraphRelationKind::Extended(ExtendedRelationKind::Documents)],
    });
    candidate_visited.relations.budget = candidate_visited.relations.budget.with_aggregate_limits(
        Some(100),
        Some(100),
        Some(2),
        Some(100),
        None,
        None,
    )?;
    let candidate_visited_report = fitted_report(&store, &candidate_visited)?;
    require(
        candidate_visited_report
            .entrypoint_profile
            .as_ref()
            .is_some_and(|profile| profile.coverage == EntrypointProfileCoverage::Partial)
            && candidate_visited_report
                .reached_limits
                .contains(&GraphLimitKind::Visited)
            && candidate_visited_report.work.analyzed_nodes == 2
            && candidate_visited_report.findings.iter().all(|finding| {
                finding.kind == AnalysisFindingKind::EntrypointReachability
                    && finding.status == AnalysisStatus::Inconclusive
            }),
        "validated disconnected candidates escaped the shared visited ceiling",
    )?;

    let mut node_bounded = bounded;
    node_bounded.relations.budget = node_bounded.relations.budget.with_aggregate_limits(
        Some(100),
        Some(1),
        Some(100),
        Some(100),
        Some(256 * 1_024),
        None,
    )?;
    node_bounded.relations.content_selection = ContentSelection::Source;
    let classification_seen = Rc::new(Cell::new(false));
    let classification_seen_observer = Rc::clone(&classification_seen);
    let node_report = observe_analysis_phase(
        move |event| {
            if event == AnalysisPhaseEvent::ClassificationHydration {
                classification_seen_observer.set(true);
            }
        },
        || fitted_report(&store, &node_bounded),
    )?;
    require(
        node_report.reached_limits.contains(&GraphLimitKind::Nodes)
            && !node_report.reached_limits.contains(&GraphLimitKind::Edges)
            && !classification_seen.get(),
        "node exhaustion was reported as an edge limit",
    )?;
    Ok(())
}

#[test]
fn community_closure_scopes_scope_gaps_to_admitted_relations() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut all_relations = analysis_query(RelationAnalysisMode::Architecture)?;
    all_relations.relations.anchor = RelationAnchor::File {
        file: RepositoryFilePath::new(Path::new("tools/c.rs"))?,
    };
    all_relations.relations.budget = DetailedRelationBudget::from_graph_limits(
        projectatlas_core::graph::GraphLimits::new(100, 100, 8, 64 * 1024)?,
    );
    let relations = load_detailed_relations(&store, &all_relations.relations, None)?;
    let mut nodes = collect_nodes(&relations, None)?;
    for anchor in ["src/b.rs", "src/a.rs"] {
        let mut query = all_relations.relations.clone();
        query.anchor = RelationAnchor::File {
            file: RepositoryFilePath::new(Path::new(anchor))?,
        };
        for (key, node) in collect_nodes(&load_detailed_relations(&store, &query, None)?, None)? {
            nodes.entry(key).or_insert(node);
        }
    }
    let containment_target = nodes
        .iter()
        .find_map(|(key, node)| {
            matches!(
                node.entity.selector(),
                EntitySelector::Symbol { symbol } if symbol.name.as_str() == "c_aux"
            )
            .then_some(key.clone())
        })
        .ok_or("containment target missing from all-family fixture")?;
    let weighted_target = nodes
        .iter()
        .find_map(|(key, node)| {
            matches!(
                node.entity.selector(),
                EntitySelector::Symbol { symbol } if symbol.name.as_str() == "b_hub"
            )
            .then_some(key.clone())
        })
        .ok_or("weighted target missing from all-family fixture")?;
    nodes.remove(&containment_target);
    let mut contains_edges = Vec::new();
    let contains_closure = close_induced_edges(
        &store,
        &all_relations,
        &relations.work,
        Instant::now() + Duration::from_secs(5),
        &nodes,
        &mut contains_edges,
        None,
    )?;
    require(
        !contains_closure.induced_scope_closed
            && contains_closure.community_scope == CommunityClosureScope::Closed,
        "containment-only closure gap changed community scope completeness",
    )?;
    require(
        relation_evidence_complete(
            &relations,
            &nodes,
            &contains_edges,
            &all_relations,
            &contains_closure,
            true,
        ),
        "containment-only closure gap made community evidence inconclusive",
    )?;
    let containment_findings =
        community_findings(&nodes, &contains_edges, true, &all_relations, None)?;
    require(
        containment_findings
            .iter()
            .all(|finding| finding.status == AnalysisStatus::Candidate),
        "containment-only closure gap did not preserve community candidates",
    )?;

    nodes.remove(&weighted_target);
    let mut weighted_edges = Vec::new();
    let weighted_closure = close_induced_edges(
        &store,
        &all_relations,
        &relations.work,
        Instant::now() + Duration::from_secs(5),
        &nodes,
        &mut weighted_edges,
        None,
    )?;
    require(
        weighted_closure.community_scope == CommunityClosureScope::Open,
        "out-of-scope weighted closure gap was ignored by community scope",
    )?;
    require(
        !relation_evidence_complete(
            &relations,
            &nodes,
            &weighted_edges,
            &all_relations,
            &weighted_closure,
            true,
        ),
        "out-of-scope weighted closure gap was reported as complete community evidence",
    )?;
    let weighted_findings =
        community_findings(&nodes, &weighted_edges, false, &all_relations, None)?;
    require(
        weighted_findings
            .iter()
            .any(|finding| finding.status == AnalysisStatus::Inconclusive),
        "out-of-scope weighted closure gap did not produce inconclusive communities",
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
    let (_, bounded_edges, resource_limits) = admitted_community_scope(&nodes, &edges, parameters);
    require(
        resource_limits == vec![GraphLimitKind::Nodes, GraphLimitKind::Edges]
            && bounded_edges.len() <= parameters.edge_limit as usize,
        "community resource ceilings did not truncate the admitted edge scope",
    )?;
    let mut node_limited_query = query.clone();
    node_limited_query.relations.budget = node_limited_query
        .relations
        .budget
        .with_aggregate_limits(Some(100), Some(3), None, None, None, None)?;
    let (_, _, node_limits) = community_findings_with_budget(
        &nodes,
        &edges,
        true,
        &node_limited_query,
        node_limited_query.relations.budget.intermediate_bytes(),
        0,
        None,
    )?;
    require(
        node_limits == vec![GraphLimitKind::Nodes],
        "node-only community truncation reported an unrelated limit",
    )?;
    let mut edge_limited_query = query.clone();
    edge_limited_query.relations.budget = edge_limited_query
        .relations
        .budget
        .with_aggregate_limits(Some(1), Some(50), None, None, None, None)?;
    let (_, _, edge_limits) = community_findings_with_budget(
        &nodes,
        &edges,
        true,
        &edge_limited_query,
        edge_limited_query.relations.budget.intermediate_bytes(),
        0,
        None,
    )?;
    require(
        edge_limits == vec![GraphLimitKind::Edges],
        "edge-only community truncation reported an unrelated limit",
    )?;
    let mut contains_edges = Vec::new();
    for _ in 0..2048 {
        contains_edges.push(LocalEdge {
            source: edges[0].source.clone(),
            target: edges[0].target.clone(),
            kind: GraphRelationKind::Legacy(RelationKind::Contains),
            complete: true,
        });
    }
    let nodes_only_working_upper_bound = community_working_set_upper_bound(&nodes, &[], None)?;
    let contains_only_working_upper_bound =
        community_working_set_upper_bound(&nodes, &contains_edges, None)?;
    require(
        nodes_only_working_upper_bound == contains_only_working_upper_bound,
        "excluded containment edges consumed community working-set budget",
    )?;
    let mut mixed_edges = contains_edges;
    mixed_edges.push(edges[0].clone());
    let mixed_working_upper_bound = community_working_set_upper_bound(&nodes, &mixed_edges, None)?;
    require(
        mixed_working_upper_bound > contains_only_working_upper_bound,
        "admitted community edges did not consume working-set budget",
    )?;
    let (mixed, _, mixed_limits) = community_findings_with_budget(
        &nodes,
        &mixed_edges,
        true,
        &query,
        query.relations.budget.intermediate_bytes(),
        0,
        None,
    )?;
    require(
        mixed_limits.is_empty()
            && mixed.iter().all(|finding| {
                finding.status == AnalysisStatus::Candidate
                    && finding
                        .community
                        .as_ref()
                        .is_some_and(|community| !community.truncated)
            }),
        "excluded containment edges prevented the admitted partition from fitting",
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
    let mut dense_edges = scale_edges.clone();
    dense_edges.extend([
        LocalEdge {
            source: scale_keys[1].clone(),
            target: scale_keys[1].clone(),
            kind: calls,
            complete: true,
        },
        LocalEdge {
            source: scale_keys[2].clone(),
            target: scale_keys[1].clone(),
            kind: calls,
            complete: true,
        },
        LocalEdge {
            source: scale_keys[3].clone(),
            target: scale_keys[1].clone(),
            kind: references,
            complete: true,
        },
        LocalEdge {
            source: scale_keys[4].clone(),
            target: scale_keys[1].clone(),
            kind: GraphRelationKind::Legacy(RelationKind::DependsOn),
            complete: true,
        },
    ]);
    require(
        scale_nodes.len() == MAX_ANALYSIS_NODES as usize
            && dense_edges.len() == MAX_ANALYSIS_EDGES as usize,
        "dense community regression did not reach the legal node and edge ceilings",
    )?;
    let mut dense_budget_query = query.clone();
    dense_budget_query.relations.budget =
        dense_budget_query.relations.budget.with_aggregate_limits(
            Some(MAX_ANALYSIS_EDGES),
            Some(MAX_ANALYSIS_NODES),
            None,
            None,
            Some(64 * 1024),
            None,
        )?;
    let dense_working_upper_bound =
        community_working_set_upper_bound(&scale_nodes, &dense_edges, None)?;
    let (dense_budgeted, dense_working_set_bytes, dense_limits) = community_findings_with_budget(
        &scale_nodes,
        &dense_edges,
        true,
        &dense_budget_query,
        dense_budget_query.relations.budget.intermediate_bytes(),
        0,
        None,
    )?;
    let dense_budgeted_bytes = serde_json::to_vec(&dense_budgeted)?;
    let dense_community = dense_budgeted
        .first()
        .and_then(|finding| finding.community.as_ref())
        .ok_or("dense community truncation metadata missing")?;
    require(
        dense_working_upper_bound > dense_budget_query.relations.budget.intermediate_bytes()
            && dense_working_set_bytes == 0
            && dense_limits == vec![GraphLimitKind::IntermediateBytes]
            && dense_working_set_bytes.saturating_add(dense_budgeted_bytes.len() as u64)
                <= dense_budget_query.relations.budget.intermediate_bytes()
            && dense_budgeted_bytes.len()
                <= dense_budget_query.relations.budget.intermediate_bytes() as usize
            && dense_budgeted.len() == 1
            && dense_budgeted[0].kind == AnalysisFindingKind::Community
            && dense_budgeted[0].status == AnalysisStatus::Inconclusive
            && dense_community.truncated
            && dense_community.parameters.node_limit == MAX_ANALYSIS_NODES
            && dense_community.parameters.edge_limit == MAX_ANALYSIS_EDGES,
        "dense minimum-budget community construction did not refuse before graph-sized allocation",
    )?;
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
    let marker_parameters = CommunityParameters {
        algorithm_version: COMMUNITY_ALGORITHM_VERSION,
        ordering_version: COMMUNITY_ORDERING_VERSION,
        max_iterations: COMMUNITY_MAX_ITERATIONS,
        node_limit: query.relations.budget.nodes().min(MAX_ANALYSIS_NODES),
        edge_limit: query.relations.budget.edges().min(MAX_ANALYSIS_EDGES),
        output_bytes: query.relations.budget.output_bytes(),
        relation: query.relations.relation,
    };
    let marker_weights = community_relation_weights();
    let marker = community_truncation_finding(
        &marker_weights,
        marker_parameters,
        0,
        CommunityCoverage::Complete,
    );
    let marker_bytes = serialized_bytes_controlled(&marker, None)?;
    let marker_vec_bytes = serialized_bytes_controlled(&vec![marker], None)?;
    let first_append_bytes = serialized_findings_append_bytes(marker_vec_bytes, 1, 0);
    let subsequent_append_bytes = serialized_findings_append_bytes(marker_vec_bytes, 1, 1);
    require(
        first_append_bytes == marker_bytes
            && subsequent_append_bytes == marker_bytes.saturating_add(1),
        "community marker append accounting lost JSON framing or separator bytes",
    )?;
    let working_set_bytes = community_working_set_upper_bound(&nodes, &[], None)?;
    require(
        first_append_bytes <= working_set_bytes,
        "community marker unexpectedly exceeded the fixed working-set charge",
    )?;
    let (fitted_marker, _, fitted_limits) =
        community_findings_with_budget(&nodes, &[], true, &query, working_set_bytes, 0, None)?;
    let fitted_marker_bytes = serde_json::to_vec(&fitted_marker)?;
    require(
        fitted_limits == vec![GraphLimitKind::IntermediateBytes]
            && fitted_marker.len() == 1
            && fitted_marker[0].status == AnalysisStatus::Inconclusive
            && fitted_marker[0]
                .community
                .as_ref()
                .is_some_and(|community| community.truncated)
            && fitted_marker_bytes.len() <= working_set_bytes as usize,
        "a fitting community truncation marker lost its typed bounded outcome",
    )?;
    let (omitted_marker, _, omitted_limits) = community_findings_with_budget(
        &nodes,
        &[],
        true,
        &query,
        first_append_bytes.saturating_sub(1),
        0,
        None,
    )?;
    let (equal_marker, _, equal_limits) =
        community_findings_with_budget(&nodes, &[], true, &query, first_append_bytes, 0, None)?;
    let (plus_marker, _, plus_limits) = community_findings_with_budget(
        &nodes,
        &[],
        true,
        &query,
        first_append_bytes.saturating_add(1),
        0,
        None,
    )?;
    let (existing_marker, _, existing_limits) = community_findings_with_budget(
        &nodes,
        &[],
        true,
        &query,
        subsequent_append_bytes,
        1,
        None,
    )?;
    let (existing_omitted_marker, _, existing_omitted_limits) = community_findings_with_budget(
        &nodes,
        &[],
        true,
        &query,
        subsequent_append_bytes.saturating_sub(1),
        1,
        None,
    )?;
    let (omitted_marker_repeat, _, _) = community_findings_with_budget(
        &nodes,
        &[],
        true,
        &query,
        first_append_bytes.saturating_sub(1),
        0,
        None,
    )?;
    require(
        omitted_limits == vec![GraphLimitKind::IntermediateBytes]
            && omitted_marker.is_empty()
            && serde_json::to_vec(&omitted_marker)? == serde_json::to_vec(&omitted_marker_repeat)?
            && first_append_bytes > first_append_bytes.saturating_sub(1),
        "an oversized community truncation marker was retained",
    )?;
    require(
        equal_limits == vec![GraphLimitKind::IntermediateBytes]
            && plus_limits == vec![GraphLimitKind::IntermediateBytes]
            && existing_limits == vec![GraphLimitKind::IntermediateBytes]
            && existing_omitted_limits == vec![GraphLimitKind::IntermediateBytes]
            && equal_marker.len() == 1
            && plus_marker.len() == 1
            && existing_marker.len() == 1
            && existing_omitted_marker.is_empty()
            && serialized_findings_append_bytes(
                serialized_bytes_controlled(&equal_marker, None)?,
                equal_marker.len(),
                0,
            ) <= first_append_bytes
            && serialized_findings_append_bytes(
                serialized_bytes_controlled(&plus_marker, None)?,
                plus_marker.len(),
                0,
            ) <= first_append_bytes.saturating_add(1),
        "preflight marker boundary did not retain only fitting actual JSON bytes",
    )?;
    let mut marker_query = query.clone();
    marker_query.include_cycles = false;
    let mut marker_work = SupplementalWork::default();
    let report_findings = architecture_findings(
        &store,
        &BTreeMap::new(),
        &[],
        true,
        true,
        &marker_query,
        first_append_bytes.saturating_sub(1),
        0,
        &mut marker_work,
        None,
    )?;
    require(
        report_findings.is_empty()
            && marker_work.composition_truncated
            && marker_work
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes),
        "report composition lost the typed limit when a community marker did not fit",
    )?;
    let mut composed_work = SupplementalWork::default();
    let composed_findings = architecture_findings(
        &store,
        &BTreeMap::new(),
        &[],
        true,
        true,
        &marker_query,
        subsequent_append_bytes,
        1,
        &mut composed_work,
        None,
    )?;
    require(
        composed_findings.len() == 1
            && composed_findings[0].community.is_some()
            && composed_work.composition_truncated
            && composed_work
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes),
        "community marker did not compose after an existing non-community finding",
    )?;
    let mut zero_budget_work = SupplementalWork::default();
    let zero_budget_findings = architecture_findings(
        &store,
        &BTreeMap::new(),
        &[],
        true,
        true,
        &marker_query,
        0,
        1,
        &mut zero_budget_work,
        None,
    )?;
    require(
        zero_budget_findings.is_empty()
            && zero_budget_work.composition_truncated
            && zero_budget_work
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes),
        "zero remaining community allowance lost typed truncation truth",
    )?;
    let candidate_key = nodes
        .keys()
        .next()
        .cloned()
        .ok_or("candidate marker node missing")?;
    let candidate_keys = vec![candidate_key.clone()];
    let candidate_labels = BTreeMap::from([(candidate_key.clone(), candidate_key)]);
    let candidate_bound = community_candidate_upper_bound(
        &nodes,
        &candidate_keys,
        &[],
        &marker_weights,
        marker_parameters,
        None,
    )?;
    require(
        candidate_bound > marker_bytes,
        "community candidate bound did not exercise marker truncation",
    )?;
    let (candidate_marker, candidate_limits) = community_candidate_findings(
        &nodes,
        &candidate_keys,
        &[],
        &candidate_labels,
        &marker_weights,
        marker_parameters,
        0,
        CommunityConvergence::Converged,
        None,
        0,
        first_append_bytes,
    )?;
    let (candidate_equal, candidate_equal_limits) = community_candidate_findings(
        &nodes,
        &candidate_keys,
        &[],
        &candidate_labels,
        &marker_weights,
        marker_parameters,
        0,
        CommunityConvergence::Converged,
        None,
        0,
        first_append_bytes,
    )?;
    let (candidate_plus, candidate_plus_limits) = community_candidate_findings(
        &nodes,
        &candidate_keys,
        &[],
        &candidate_labels,
        &marker_weights,
        marker_parameters,
        0,
        CommunityConvergence::Converged,
        None,
        0,
        first_append_bytes.saturating_add(1),
    )?;
    let (candidate_existing, candidate_existing_limits) = community_candidate_findings(
        &nodes,
        &candidate_keys,
        &[],
        &candidate_labels,
        &marker_weights,
        marker_parameters,
        0,
        CommunityConvergence::Converged,
        None,
        1,
        subsequent_append_bytes,
    )?;
    let (candidate_existing_omitted, candidate_existing_omitted_limits) =
        community_candidate_findings(
            &nodes,
            &candidate_keys,
            &[],
            &candidate_labels,
            &marker_weights,
            marker_parameters,
            0,
            CommunityConvergence::Converged,
            None,
            1,
            subsequent_append_bytes.saturating_sub(1),
        )?;
    let (candidate_omitted, candidate_omitted_limits) = community_candidate_findings(
        &nodes,
        &candidate_keys,
        &[],
        &candidate_labels,
        &marker_weights,
        marker_parameters,
        0,
        CommunityConvergence::Converged,
        None,
        0,
        first_append_bytes.saturating_sub(1),
    )?;
    require(
        candidate_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_marker.len() == 1
            && candidate_marker[0].community.is_some()
            && candidate_equal_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_equal.len() == 1
            && candidate_plus_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_plus.len() == 1
            && candidate_existing_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_existing.len() == 1
            && candidate_existing_omitted_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_existing_omitted.is_empty()
            && candidate_omitted_limits == vec![GraphLimitKind::IntermediateBytes]
            && candidate_omitted.is_empty(),
        "candidate truncation did not honor the remaining marker budget",
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
fn zero_remaining_community_allowance_preserves_report_truth() -> Result<(), Box<dyn Error>> {
    let (_temp, store) = analysis_store()?;
    let mut query = analysis_query(RelationAnalysisMode::Architecture)?;
    query.include_communities = true;
    query.include_cycles = false;
    query.relations.resolution = RelationResolutionFilter::Resolved;
    query.relations.relation = None;

    let calibration_budget = 80_000_u64;
    let mut calibration_query = query.clone();
    calibration_query.relations.budget = calibration_query.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(calibration_budget),
        None,
    )?;
    let calibration_observed = Rc::new(RefCell::new(None));
    let calibration_writer = Rc::clone(&calibration_observed);
    let _calibration = observe_analysis_phase(
        move |event| {
            if let AnalysisPhaseEvent::CompositionBudget {
                symbol_byte_budget,
                existing_finding_append_bytes,
                community_budget,
            } = event
            {
                *calibration_writer.borrow_mut() = Some((
                    symbol_byte_budget,
                    existing_finding_append_bytes,
                    community_budget,
                ));
            }
        },
        || load_relation_analysis(&store, &calibration_query, None),
    )?;
    let (_, existing_append_bytes, calibration_community_budget) = calibration_observed
        .borrow()
        .ok_or("architecture community budget was not observed")?;
    require(
        existing_append_bytes > 0 && calibration_community_budget > 0,
        "calibration did not retain a preceding finding and community allowance",
    )?;

    // Both requests use five-digit intermediate budgets, so their serialized
    // cursor shapes and relation work are identical. Subtracting the
    // independently observed remaining allowance reaches the exact append
    // boundary without searching neighboring budgets.
    let boundary_budget = calibration_budget
        .checked_sub(calibration_community_budget)
        .ok_or("calibration community allowance exceeded the request budget")?;
    require(
        boundary_budget >= 64 * 1024,
        "calibrated community boundary fell below the product budget floor",
    )?;
    let mut boundary_query = query;
    boundary_query.relations.budget = boundary_query.relations.budget.with_aggregate_limits(
        None,
        None,
        None,
        None,
        Some(boundary_budget),
        None,
    )?;
    let boundary_observed = Rc::new(RefCell::new(None));
    let boundary_writer = Rc::clone(&boundary_observed);
    let draft = observe_analysis_phase(
        move |event| {
            if let AnalysisPhaseEvent::CompositionBudget {
                symbol_byte_budget,
                existing_finding_append_bytes,
                community_budget,
            } = event
            {
                *boundary_writer.borrow_mut() = Some((
                    symbol_byte_budget,
                    existing_finding_append_bytes,
                    community_budget,
                ));
            }
        },
        || load_relation_analysis(&store, &boundary_query, None),
    )?;
    let (symbol_byte_budget, existing_append_bytes, community_budget) = boundary_observed
        .borrow()
        .ok_or("boundary architecture community budget was not observed")?;
    let report = draft.report;

    // Serialize the explicit request and retained finding vector independently
    // of the production byte-accounting helpers under test. The fixture starts
    // with no resolution gaps, so the returned architecture vector owns its
    // complete JSON array envelope.
    let request_bytes = serde_json::to_vec(&boundary_query.relations.budget)?;
    let request_json: serde_json::Value = serde_json::from_slice(&request_bytes)?;
    let findings_vector_bytes = serde_json::to_vec(&report.findings)?;
    let expected_existing_append_bytes = findings_vector_bytes
        .len()
        .checked_sub(2)
        .ok_or("serialized findings vector omitted its JSON array envelope")?;
    require(
        request_json
            .get("intermediate_bytes")
            .and_then(serde_json::Value::as_u64)
            == Some(boundary_budget)
            && findings_vector_bytes.starts_with(b"[")
            && findings_vector_bytes.ends_with(b"]")
            && u64::try_from(expected_existing_append_bytes)? == existing_append_bytes
            && symbol_byte_budget == existing_append_bytes
            && community_budget == 0
            && existing_append_bytes > 0
            && report
                .findings
                .iter()
                .any(|finding| finding.kind == AnalysisFindingKind::Component)
            && report.truncated
            && report
                .reached_limits
                .contains(&GraphLimitKind::IntermediateBytes)
            && report.work.composition_truncated
            && report.work.retained_composition_bytes <= boundary_budget
            && matches!(report.total, RelationTotalState::AtLeast(_))
            && !matches!(report.total, RelationTotalState::Exact(_))
            && report
                .findings
                .iter()
                .all(|finding| finding.community.is_none()),
        "zero remaining community allowance did not preserve report truncation truth",
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
        entrypoint_profile: None,
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

fn terminal_entrypoint_store(
    include_terminal_edge: bool,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("terminal-entrypoint");
    fs::create_dir_all(root.join("src"))?;
    for (path, contents) in [
        ("src/a.rs", "pub fn a() {}\n"),
        ("src/b.rs", "pub fn b() {}\n"),
    ] {
        fs::write(root.join(path), contents)?;
    }
    if include_terminal_edge {
        fs::write(root.join("src/c.rs"), "pub fn c() {}\n")?;
    }
    let database = root.join("projectatlas.db");
    let mut store = AtlasStore::open_for_project(&database, &root)?;
    let project = store
        .project_instance_id()?
        .ok_or("terminal entrypoint project identity missing")?;
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
    let c = include_terminal_edge
        .then(|| entity("src/c.rs"))
        .transpose()?;
    let calls = GraphRelationKind::Legacy(RelationKind::Calls);
    let relation = |source: &GraphEntity, target: &GraphEntity| {
        LogicalRelation::new(
            source,
            calls,
            RelationResolution::resolved(target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )
    };
    let mut relations = vec![relation(&a, &b)?];
    if let Some(c) = c.as_ref() {
        relations.push(relation(&b, c)?);
    }
    let entities = c.as_ref().map_or_else(
        || vec![a.clone(), b.clone()],
        |c| vec![a.clone(), b.clone(), c.clone()],
    );
    let coverage = entities
        .iter()
        .map(|entity| {
            let path = match entity.selector() {
                EntitySelector::File { path } => path.as_str(),
                _ => unreachable!("terminal fixture only contains files"),
            };
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
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut publication = store.begin_index_publication("terminal-entrypoint")?;
    publication.begin_scan_replacement()?;
    let scan_nodes = entities
        .iter()
        .map(|entity| match entity.selector() {
            EntitySelector::File { path } => test_node(path.as_str(), path.as_str()),
            _ => unreachable!("terminal fixture only contains files"),
        })
        .collect::<Vec<_>>();
    publication.upsert_scan_node_batch(&scan_nodes)?;
    publication.finish_scan_replacement()?;
    publication.replace_repository_graph(project, &entities, &relations, &[], &coverage)?;
    publication.complete()?;
    drop(store);
    Ok((
        temp,
        AtlasStore::open_read_only_for_project(&database, &root)?,
    ))
}

fn analysis_store_with_coverage(
    include_tools_coverage: bool,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_options(include_tools_coverage, None, false, 0, false, None, None)
}

fn analysis_store_with_target(
    target_selector: EntitySelector,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_options(true, Some(target_selector), false, 0, false, None, None)
}

fn analysis_store_with_external_candidate()
-> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_options(true, None, true, 0, false, None, None)
}

fn analysis_store_with_large_candidates() -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>>
{
    analysis_store_with_options(true, None, false, 20, false, None, None)
}

fn analysis_store_with_document_relation() -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>>
{
    analysis_store_with_options(true, None, false, 0, true, None, None)
}

fn analysis_store_with_candidate_relation(
    target: &str,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_options(true, None, false, 0, false, Some(target), None)
}

fn analysis_store_with_relation_coverage(
    partial_calls: bool,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    analysis_store_with_options(true, None, false, 0, false, None, Some(partial_calls))
}

fn analysis_store_with_options(
    include_tools_coverage: bool,
    target_selector: Option<EntitySelector>,
    external_candidate: bool,
    large_candidate_count: usize,
    document_relation: bool,
    candidate_relation_target: Option<&str>,
    relation_coverage_partial_calls: Option<bool>,
) -> Result<(tempfile::TempDir, AtlasStore), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("analysis-service");
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("tools"))?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("src/a.rs"), "pub fn a() {}\n")?;
    fs::write(root.join("src/b.rs"), "pub fn b() {}\n")?;
    fs::write(root.join("tools/c.rs"), "pub fn c() {}\n")?;
    fs::write(root.join("docs/guide.md"), "# Guide\n")?;
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
    let guide = entity("docs/guide.md")?;
    let extra_target = target_selector
        .map(|selector| GraphEntity::new(project, selector, generation))
        .transpose()?;
    let candidate_external = external_candidate
        .then(|| {
            GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("crates.io")?,
                        identity: GraphIdentityText::new("candidate@1")?,
                    },
                },
                generation,
            )
        })
        .transpose()?;
    let candidate_external_second = external_candidate
        .then(|| {
            GraphEntity::new(
                project,
                EntitySelector::External {
                    external: ExternalSelector {
                        system: GraphIdentityText::new("crates.io")?,
                        identity: GraphIdentityText::new("candidate@2")?,
                    },
                },
                generation,
            )
        })
        .transpose()?;
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
    let candidate_relation_target = candidate_relation_target
        .map(|target| -> Result<GraphEntity, Box<dyn Error>> {
            match target {
                "reachable" => Ok(a.clone()),
                "new" => Ok(GraphEntity::new(
                    project,
                    EntitySelector::Package {
                        package: PackageSelector {
                            manager: GraphIdentityText::new("cargo")?,
                            name: GraphIdentityText::new("candidate-package")?,
                            manifest: RepositoryFilePath::new(Path::new("src/a.rs"))?,
                        },
                    },
                    generation,
                )?),
                _ => Err(io::Error::other("unknown candidate relation target").into()),
            }
        })
        .transpose()?;
    let large_candidates = (0..large_candidate_count)
        .map(|index| {
            symbol_entity(
                "src/a.rs",
                &format!("byte_candidate_{index}"),
                &format!("candidate_{index}_{}", "x".repeat(4_000)),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
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
    let mut relations = vec![
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
    if document_relation {
        relations.push(LogicalRelation::new(
            &guide,
            GraphRelationKind::Extended(ExtendedRelationKind::Documents),
            RelationResolution::resolved(&a)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?);
    }
    if let Some(target) = extra_target.as_ref() {
        let relation = if matches!(target.selector(), EntitySelector::External { .. }) {
            LogicalRelation::new(
                &a,
                GraphRelationKind::Legacy(RelationKind::Calls),
                RelationResolution::external(target)?,
                ConfidenceClass::Exact,
                Completeness::Complete,
                generation,
            )?
        } else {
            relation(&a, target, GraphRelationKind::Legacy(RelationKind::Calls))?
        };
        relations.push(relation);
    }
    if let Some(target) = candidate_external.as_ref() {
        relations.push(LogicalRelation::new(
            &d_unused,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::external(target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?);
    }
    if let Some(target) = candidate_external_second.as_ref() {
        relations.push(LogicalRelation::new(
            &d_unused,
            GraphRelationKind::Legacy(RelationKind::Calls),
            RelationResolution::external(target)?,
            ConfidenceClass::Exact,
            Completeness::Complete,
            generation,
        )?);
    }
    if let Some(target) = candidate_relation_target.as_ref() {
        relations.push(relation(
            &d_unused,
            target,
            GraphRelationKind::Legacy(RelationKind::Calls),
        )?);
    }
    let mut coverage_paths = vec!["src/a.rs", "src/b.rs", "tools/c.rs", "docs/guide.md"];
    if extra_target
        .as_ref()
        .is_some_and(|target| matches!(target.selector(), EntitySelector::Folder { .. }))
    {
        coverage_paths.push("src");
    }
    let mut coverage = coverage_paths
        .iter()
        .copied()
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
        coverage_paths
            .iter()
            .copied()
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
    if let Some(partial_calls) = relation_coverage_partial_calls {
        let (calls_state, calls_covered, calls_omitted, calls_reason) = if partial_calls {
            (
                CoverageState::Partial,
                1,
                1,
                Some(GraphIdentityText::new("partial calls fixture")?),
            )
        } else {
            (CoverageState::Complete, 1, 0, None)
        };
        coverage.push(CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/a.rs"))?,
            },
            Some(GraphRelationKind::Legacy(RelationKind::Calls)),
            calls_state,
            calls_covered,
            calls_omitted,
            generation,
            calls_reason,
            None,
        )?);
        coverage.push(CoverageRecord::new(
            CoverageScope::Path {
                path: RepositoryNodePath::new(Path::new("src/a.rs"))?,
            },
            Some(GraphRelationKind::Extended(ExtendedRelationKind::Documents)),
            CoverageState::Partial,
            1,
            1,
            generation,
            Some(GraphIdentityText::new("partial documents fixture")?),
            None,
        )?);
    }
    let mut publication = store.begin_index_publication("analysis-service")?;
    publication.begin_scan_replacement()?;
    publication.upsert_scan_node_batch(&[
        test_folder_node("src"),
        test_folder_node("tools"),
        test_folder_node("docs"),
        test_node("src/a.rs", "hash-a"),
        test_node("src/b.rs", "hash-b"),
        test_node_in("tools/c.rs", "tools", "hash-c"),
        classified_test_node("docs/guide.md", "hash-guide", ".md", "markdown"),
    ])?;
    publication.upsert_file_content_classification_batch(&[
        projectatlas_db::FileContentClassification {
            path: "src/a.rs".to_string(),
            classification: ContentClassification::Source,
        },
        projectatlas_db::FileContentClassification {
            path: "src/b.rs".to_string(),
            classification: ContentClassification::Source,
        },
        projectatlas_db::FileContentClassification {
            path: "tools/c.rs".to_string(),
            classification: ContentClassification::Source,
        },
        projectatlas_db::FileContentClassification {
            path: "docs/guide.md".to_string(),
            classification: ContentClassification::Documentation,
        },
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
    let mut entities = vec![a, b, c, guide, a_long, d_unused, b_hub, c_aux];
    entities.extend(large_candidates);
    if let Some(target) = extra_target {
        entities.push(target);
    }
    if let Some(target) = candidate_relation_target {
        entities.push(target);
    }
    if let Some(target) = candidate_external {
        entities.push(target);
    }
    if let Some(target) = candidate_external_second {
        entities.push(target);
    }
    publication.replace_repository_graph(project, &entities, &relations, &[], &coverage)?;
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

fn classified_test_node(path: &str, hash: &str, extension: &str, language: &str) -> Node {
    Node {
        path: path.to_string(),
        kind: NodeKind::File,
        parent_path: Some("docs".to_string()),
        extension: Some(extension.to_string()),
        language: Some(language.to_string()),
        size_bytes: Some(8),
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
