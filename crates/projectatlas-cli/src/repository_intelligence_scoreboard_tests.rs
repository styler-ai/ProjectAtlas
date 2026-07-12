//! Deterministic source projection for the repository-intelligence scoreboard.

use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SCOREBOARD: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-phase-scoreboard.json");
const INTELLIGENCE_DESIGN: &str =
    include_str!("../../../openspec/changes/advance-rust-repository-intelligence/design.md");
const SOURCE_IDS: &[&str] = &[
    "capability_registry",
    "evaluation_manifest",
    "repository_intelligence_contracts",
    "surface_contract",
    "canonical_graph_contract",
    "measurement_results",
    "architecture_cost_results",
    "compatibility_results",
    "hosted_results",
    "review_results",
];
const DECISION_FUNCTION_IDS: &[&str] = &[
    "all_required_members_pass",
    "corrected_lower_bound_floor",
    "corrected_upper_bound_ceiling",
    "exact_equivalence",
    "hard_budget",
    "compatibility_replay",
    "agent_non_inferiority",
    "zero_growth",
    "complete_containment",
    "review_clearance",
];
const RESULT_SCHEMA_IDS: &[&str] = &[
    "agent_workflow_result",
    "architecture_cost_result",
    "compatibility_result",
    "containment_result",
    "correctness_family_result",
    "determinism_result",
    "dimension_result",
    "evaluation_incremental_result",
    "evaluation_index_result",
    "evaluation_query_result",
    "resource_result",
];
const DIMENSION_FIELDS: &[&str] = &[
    "blocker",
    "decision_function",
    "eligibility",
    "group",
    "id",
    "label",
    "result_schema",
    "scope",
    "source_refs",
    "state",
];
const PHASE_READINESS_FIELDS: &[&str] = &["blocker", "id", "label", "scope", "state"];
const EXPECTED_PHASE_READINESS_GATES: &[&str] = &[
    "contracts.frozen",
    "sqlite-feasibility.complete",
    "architecture-decisions.accepted",
    "phase-reviews.resolved",
];
const EXPECTED_DIMENSIONS: &[(&str, &str)] = &[
    ("capability.accepted-coverage", "correctness"),
    ("correctness.per-family-accuracy", "correctness"),
    ("compatibility.cli", "compatibility"),
    ("compatibility.mcp", "compatibility"),
    ("determinism.full-incremental", "determinism"),
    ("determinism.worker-count", "determinism"),
    ("determinism.repeated-run", "determinism"),
    ("correctness.platform-unicode", "correctness"),
    ("containment.parser-and-optional-pack", "containment"),
    ("containment.network-filesystem-execution", "containment"),
    ("performance.full-scan-time", "performance"),
    ("performance.incremental-time", "performance"),
    ("resource.complete-process-tree-rss", "resources"),
    ("writes.physical-database", "writes"),
    ("writes.physical-wal-shm-journal", "writes"),
    ("writes.physical-staging-temp", "writes"),
    ("writes.physical-sidecar", "writes"),
    ("writes.logical", "writes"),
    ("bytes.persistent-index", "bytes"),
    ("bytes.package-artifact", "bytes"),
    ("bytes.install-transfer", "bytes"),
    ("bytes.installed-logical", "bytes"),
    ("bytes.installed-allocated", "bytes"),
    ("latency.sqlite-warm", "latency"),
    ("latency.service-warm", "latency"),
    ("latency.mcp-e2e-warm", "latency"),
    ("latency.cold-startup", "latency"),
    ("agent.answer-quality", "agent-workflow"),
    ("agent.unsupported-assertions", "agent-workflow"),
    ("agent.workflow-context-tokens", "agent-workflow"),
    ("agent.workflow-file-reads", "agent-workflow"),
    ("agent.workflow-tool-calls", "agent-workflow"),
    ("agent.mandatory-call-sequence", "agent-workflow"),
    ("architecture.production-code-growth", "complexity"),
    ("architecture.generated-code-growth", "complexity"),
    ("architecture.crate-and-dependency-growth", "complexity"),
    ("architecture.unsafe-native-ffi-boundaries", "containment"),
    ("architecture.contract-owner-duplication", "complexity"),
    ("surface.cli-growth", "public-surface"),
    ("surface.mcp-growth", "public-surface"),
    ("surface.plugin-install-effort", "public-surface"),
    ("quality.warnings-and-review-blockers", "quality"),
];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum DimensionState {
    Pending,
    Missing,
    Ineligible,
    Fail,
    Pass,
    Informational,
}

#[derive(Clone, Debug)]
enum SourceDocument {
    Present(Value),
    Missing,
}

impl SourceDocument {
    const fn state(&self) -> &'static str {
        match self {
            Self::Present(_) => "present",
            Self::Missing => "missing",
        }
    }
}

#[derive(Clone, Debug)]
struct SourceDocuments(BTreeMap<String, SourceDocument>);

impl SourceDocuments {
    fn load(root: &Path, scoreboard: &Value) -> Result<Self, String> {
        let declarations = scoreboard["source_artifacts"]
            .as_object()
            .ok_or_else(|| "source_artifacts is not an object".to_string())?;
        validate_exact_keys(declarations, SOURCE_IDS, "source type")?;
        let mut documents = BTreeMap::new();
        for &id in SOURCE_IDS {
            documents.insert(id.to_string(), load_source(root, &declarations[id], id)?);
        }
        Ok(Self(documents))
    }

    fn get(&self, id: &str) -> Result<&SourceDocument, String> {
        self.0
            .get(id)
            .ok_or_else(|| format!("source type {id} is not loaded"))
    }

    fn get_mut(&mut self, id: &str) -> Result<&mut SourceDocument, String> {
        self.0
            .get_mut(id)
            .ok_or_else(|| format!("source type {id} is not loaded"))
    }

    fn all_present(&self) -> bool {
        self.0
            .values()
            .all(|source| matches!(source, SourceDocument::Present(_)))
    }
}

#[derive(Default)]
struct StateCounts {
    pending: usize,
    missing: usize,
    ineligible: usize,
    fail: usize,
    pass: usize,
    informational: usize,
}

impl StateCounts {
    fn add(&mut self, state: DimensionState) {
        match state {
            DimensionState::Pending => self.pending += 1,
            DimensionState::Missing => self.missing += 1,
            DimensionState::Ineligible => self.ineligible += 1,
            DimensionState::Fail => self.fail += 1,
            DimensionState::Pass => self.pass += 1,
            DimensionState::Informational => self.informational += 1,
        }
    }

    const fn blocking(&self) -> usize {
        self.pending + self.missing + self.ineligible + self.fail + self.informational
    }
}

fn parse(source: &str) -> Result<Value, String> {
    serde_json::from_str(source).map_err(|error| error.to_string())
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn validate_exact_keys(
    actual: &Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual = actual.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(format!("{label} catalog drifted: {actual:?}"));
    }
    Ok(())
}

fn expected_dimension_catalog() -> BTreeMap<String, String> {
    EXPECTED_DIMENSIONS
        .iter()
        .map(|&(id, group)| (id.to_string(), group.to_string()))
        .collect()
}

fn expected_group_counts() -> BTreeMap<String, usize> {
    let mut groups = BTreeMap::new();
    for &(_, group) in EXPECTED_DIMENSIONS {
        *groups.entry(group.to_string()).or_default() += 1;
    }
    groups
}

fn phase_readiness(scoreboard: &Value) -> Result<(StateCounts, Vec<String>), String> {
    let contract_gate_ids = scoreboard["phase_exit_contract"]["required_gate_ids"]
        .as_array()
        .ok_or_else(|| "required_gate_ids is not an array".to_string())?
        .iter()
        .map(|id| id.as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    if contract_gate_ids != EXPECTED_PHASE_READINESS_GATES {
        return Err("Phase 0 readiness contract drifted".to_string());
    }
    let gates = scoreboard["phase_readiness_gates"]
        .as_array()
        .ok_or_else(|| "phase_readiness_gates is not an array".to_string())?;
    let mut gate_ids = Vec::with_capacity(gates.len());
    let mut counts = StateCounts::default();
    let mut blocking_gate_ids = Vec::new();
    for gate in gates {
        let row = gate
            .as_object()
            .ok_or_else(|| "Phase 0 readiness gate is not an object".to_string())?;
        if row.len() != PHASE_READINESS_FIELDS.len()
            || PHASE_READINESS_FIELDS
                .iter()
                .any(|field| !row.contains_key(*field))
        {
            return Err("Phase 0 readiness gate schema drifted".to_string());
        }
        let id = gate["id"]
            .as_str()
            .ok_or_else(|| "Phase 0 readiness gate ID is not a string".to_string())?;
        gate_ids.push(id);
        let state: DimensionState = serde_json::from_value(gate["state"].clone())
            .map_err(|error| format!("Phase 0 readiness gate {id} has invalid state: {error}"))?;
        counts.add(state);
        if state == DimensionState::Pass {
            if !gate["blocker"].is_null() {
                return Err(format!("passing Phase 0 readiness gate {id} has a blocker"));
            }
        } else {
            let blocker = gate["blocker"].as_str().unwrap_or_default();
            if blocker.is_empty() {
                return Err(format!(
                    "blocking Phase 0 readiness gate {id} has no blocker"
                ));
            }
            blocking_gate_ids.push(id.to_string());
        }
    }
    if gate_ids != EXPECTED_PHASE_READINESS_GATES {
        return Err("Phase 0 readiness gate catalog drifted".to_string());
    }
    Ok((counts, blocking_gate_ids))
}

fn load_source(root: &Path, declaration: &Value, id: &str) -> Result<SourceDocument, String> {
    let Some(path) = declaration["path"].as_str() else {
        if declaration["path_pattern"].is_string() {
            return Ok(SourceDocument::Missing);
        }
        return Err(format!("source {id} has neither path nor path_pattern"));
    };
    let source_path = root.join(path);
    let source = match fs::read_to_string(&source_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceDocument::Missing);
        }
        Err(error) => return Err(format!("failed to read {}: {error}", source_path.display())),
    };
    let document: Value = serde_json::from_str(&source)
        .map_err(|error| format!("failed to parse {}: {error}", source_path.display()))?;
    for field in ["format", "schema_id", "schema_version"] {
        if let Some(expected) = declaration.get(field)
            && document.get(field) != Some(expected)
        {
            return Err(format!("source {id} does not match declared {field}"));
        }
    }
    if let Some(pointer) = declaration["required_pointer"].as_str()
        && document.pointer(pointer).is_none()
    {
        return Err(format!(
            "source {id} does not resolve required pointer {pointer}"
        ));
    }
    Ok(SourceDocument::Present(document))
}

fn resolve_reference(reference: &str, sources: &SourceDocuments) -> Result<bool, String> {
    let (source_id, pointer) = reference
        .split_once('#')
        .map_or((reference, None), |(source, pointer)| {
            (source, Some(pointer))
        });
    if !SOURCE_IDS.contains(&source_id) {
        return Err(format!("unknown scoreboard source {source_id}"));
    }
    if pointer.is_some_and(|pointer| !pointer.starts_with('/')) {
        return Err(format!(
            "invalid JSON pointer in source reference {reference}"
        ));
    }
    let SourceDocument::Present(document) = sources.get(source_id)? else {
        return Ok(false);
    };
    if let Some(pointer) = pointer
        && document.pointer(pointer).is_none()
    {
        return Err(format!(
            "source {source_id} does not resolve JSON pointer {pointer}"
        ));
    }
    Ok(true)
}

fn project_scoreboard(scoreboard: &Value, sources: &SourceDocuments) -> Result<Value, String> {
    let declarations = scoreboard["source_artifacts"]
        .as_object()
        .ok_or_else(|| "source_artifacts is not an object".to_string())?;
    validate_exact_keys(declarations, SOURCE_IDS, "source type")?;
    let decisions = scoreboard["decision_functions"]
        .as_object()
        .ok_or_else(|| "decision_functions is not an object".to_string())?;
    validate_exact_keys(decisions, DECISION_FUNCTION_IDS, "decision function")?;
    let result_schemas = scoreboard["result_schemas"]
        .as_object()
        .ok_or_else(|| "result_schemas is not an object".to_string())?;
    validate_exact_keys(result_schemas, RESULT_SCHEMA_IDS, "result schema")?;
    for schema in result_schemas.values() {
        if let Some(reference) = schema["source"].as_str() {
            resolve_reference(reference, sources)?;
        }
    }

    let fields = scoreboard["generation_contract"]["dimension_fields"]
        .as_array()
        .ok_or_else(|| "dimension_fields is not an array".to_string())?
        .iter()
        .map(|field| field.as_str().unwrap_or_default())
        .collect::<BTreeSet<_>>();
    if fields != DIMENSION_FIELDS.iter().copied().collect() {
        return Err("dimension projection fields drifted".to_string());
    }
    let dimensions = scoreboard["dimensions"]
        .as_array()
        .ok_or_else(|| "dimensions is not an array".to_string())?;
    let mut dimension_catalog = BTreeMap::new();
    let mut groups = BTreeMap::<String, usize>::new();
    let mut counts = StateCounts::default();
    for dimension in dimensions {
        let row = dimension
            .as_object()
            .ok_or_else(|| "dimension is not an object".to_string())?;
        if row.len() != DIMENSION_FIELDS.len()
            || DIMENSION_FIELDS
                .iter()
                .any(|field| !row.contains_key(*field))
        {
            return Err("dimension row does not match the projection schema".to_string());
        }
        let id = dimension["id"]
            .as_str()
            .ok_or_else(|| "dimension ID is not a string".to_string())?;
        if !id.contains('.') || id.contains(char::is_whitespace) {
            return Err(format!("dimension ID is duplicate or malformed: {id}"));
        }
        let group = dimension["group"]
            .as_str()
            .ok_or_else(|| format!("dimension {id} group is not a string"))?;
        if dimension_catalog
            .insert(id.to_string(), group.to_string())
            .is_some()
        {
            return Err(format!("dimension ID is duplicate or malformed: {id}"));
        }
        *groups.entry(group.to_string()).or_default() += 1;
        let decision = dimension["decision_function"].as_str().unwrap_or_default();
        let result_schema = dimension["result_schema"].as_str().unwrap_or_default();
        if !decisions.contains_key(decision) || !result_schemas.contains_key(result_schema) {
            return Err(format!("dimension {id} has a dangling contract reference"));
        }

        let state: DimensionState = serde_json::from_value(dimension["state"].clone())
            .map_err(|error| format!("dimension {id} has invalid state: {error}"))?;
        counts.add(state);
        let source_refs = dimension["source_refs"]
            .as_array()
            .ok_or_else(|| format!("dimension {id} source_refs is not an array"))?;
        if source_refs.is_empty() {
            return Err(format!("dimension {id} has no source references"));
        }
        let mut every_source_resolved = true;
        for reference in source_refs {
            every_source_resolved &= resolve_reference(
                reference
                    .as_str()
                    .ok_or_else(|| format!("dimension {id} source reference is not a string"))?,
                sources,
            )?;
        }
        if state == DimensionState::Pass && !every_source_resolved {
            return Err(format!(
                "dimension {id} cannot pass while a required source is missing"
            ));
        }
    }

    let expected_dimensions = expected_dimension_catalog();
    if dimension_catalog != expected_dimensions {
        return Err(format!(
            "scoreboard dimension catalog drifted: {dimension_catalog:?}"
        ));
    }
    if groups != expected_group_counts() {
        return Err(format!("scoreboard group coverage drifted: {groups:?}"));
    }

    let (readiness_counts, blocking_gate_ids) = phase_readiness(scoreboard)?;
    let mut projected = scoreboard.clone();
    let projected_sources = projected["source_artifacts"]
        .as_object_mut()
        .ok_or_else(|| "projected source_artifacts is not an object".to_string())?;
    for &id in SOURCE_IDS {
        projected_sources[id]["current_state"] =
            Value::String(sources.get(id)?.state().to_string());
    }
    projected["dimension_count"] = Value::from(dimensions.len());
    let current_release = projected["current_release_acceptance"]
        .as_object_mut()
        .ok_or_else(|| "current_release_acceptance is not an object".to_string())?;
    let release_passes = counts.pass == dimensions.len() && sources.all_present();
    current_release["state"] =
        Value::String(if release_passes { "pass" } else { "blocked" }.to_string());
    current_release["required_dimension_count"] = Value::from(dimensions.len());
    current_release["passing_dimension_count"] = Value::from(counts.pass);
    current_release["blocking_dimension_count"] = Value::from(counts.blocking());
    for (state, count) in [
        ("pending", counts.pending),
        ("missing", counts.missing),
        ("ineligible", counts.ineligible),
        ("fail", counts.fail),
        ("informational", counts.informational),
    ] {
        current_release["blocking_states"][state] = Value::from(count);
    }
    current_release["claim_state"] = Value::String("no-claim".to_string());

    let current_phase = projected["current_phase_exit"]
        .as_object_mut()
        .ok_or_else(|| "current_phase_exit is not an object".to_string())?;
    let phase_passes = readiness_counts.pass == EXPECTED_PHASE_READINESS_GATES.len();
    current_phase["state"] =
        Value::String(if phase_passes { "pass" } else { "blocked" }.to_string());
    current_phase["required_readiness_gate_count"] =
        Value::from(EXPECTED_PHASE_READINESS_GATES.len());
    current_phase["passing_readiness_gate_count"] = Value::from(readiness_counts.pass);
    current_phase["blocking_readiness_gate_count"] = Value::from(readiness_counts.blocking());
    for (state, count) in [
        ("pending", readiness_counts.pending),
        ("missing", readiness_counts.missing),
        ("ineligible", readiness_counts.ineligible),
        ("fail", readiness_counts.fail),
        ("informational", readiness_counts.informational),
    ] {
        current_phase["blocking_states"][state] = Value::from(count);
    }
    current_phase["blocking_gate_ids"] =
        Value::Array(blocking_gate_ids.into_iter().map(Value::String).collect());
    current_phase["claim_state"] = Value::String("no-claim".to_string());
    projected["claim_state"] = Value::String("no-claim".to_string());
    Ok(projected)
}

fn validate_checked_scoreboard(
    scoreboard: &Value,
    sources: &SourceDocuments,
) -> Result<(), String> {
    if project_scoreboard(scoreboard, sources)? != *scoreboard {
        return Err(
            "checked scoreboard differs from its deterministic source projection".to_string(),
        );
    }
    Ok(())
}

fn validate_phase_zero_campaign_boundary(design: &str) -> Result<(), String> {
    for required in [
        "Phase 0 freezes the closed native-grammar and versioned-WASM candidate inventory",
        "Phase 0 freezes the typed readiness gate and labeled evaluation contract",
        "Phase 0 calibration contract pins Cargo and rustc executable identities",
        "Phase 0 ends with a no-claim implementation-readiness scoreboard",
        "full calibrated pinned campaigns remain in Section 11",
        "after eligible Section 11 measurements establish realistic sequencing",
    ] {
        if !design.contains(required) {
            return Err(format!(
                "Phase 0 readiness boundary is missing required language: {required}"
            ));
        }
    }
    for forbidden in [
        "Phase 0 benchmarks a broad installable grammar pack",
        "Phase 0 and the optional-semantic phase evaluate",
        "Phase 0 calibration pins Cargo and rustc and observes",
        "calibrated benchmark harness",
        "parser-pack/FTS/ANN spikes",
        "after Phase 0 measurements establish realistic sequencing",
    ] {
        if design.contains(forbidden) {
            return Err(format!(
                "Phase 0 still claims a deferred release campaign: {forbidden}"
            ));
        }
    }
    Ok(())
}

#[test]
fn checked_scoreboard_is_reproducible_source_projection() -> Result<(), String> {
    let scoreboard = parse(SCOREBOARD)?;
    if scoreboard["schema_version"] != 1
        || scoreboard["format"] != "projectatlas.phase-scoreboard-contract"
        || scoreboard["scoreboard_id"] != "projectatlas-v0.4-repository-intelligence-scoreboard"
        || scoreboard["phase_id"] != "phase-0-truth-and-baselines"
        || scoreboard["contract_state"] != "defined-not-evaluated"
        || scoreboard["claim_state"] != "no-claim"
    {
        return Err("scoreboard identity or no-claim state drifted".to_string());
    }
    let sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    validate_checked_scoreboard(&scoreboard, &sources)?;
    if scoreboard["phase_exit_contract"]["release_evidence_affects_phase_exit"] != false
        || scoreboard["release_acceptance_contract"]["aggregate_compensation_allowed"] != false
        || scoreboard["release_acceptance_contract"]["optional_member_compensation_allowed"]
            != false
        || scoreboard["one_page_rendering_contract"]["required_sections"]
            .as_array()
            .is_none_or(|sections| {
                !sections
                    .iter()
                    .any(|section| section == "independent-dimension-table")
            })
        || scoreboard["one_page_rendering_contract"]["prohibited_output"]
            .as_array()
            .is_none_or(|outputs| outputs.iter().all(|output| output != "composite score"))
    {
        return Err("one-page scoreboard can hide an independent failure".to_string());
    }
    let quality = scoreboard["dimensions"]
        .as_array()
        .and_then(|dimensions| {
            dimensions
                .iter()
                .find(|dimension| dimension["id"] == "quality.warnings-and-review-blockers")
        })
        .ok_or_else(|| "quality review dimension is missing".to_string())?;
    let quality_blocker = quality["blocker"]
        .as_str()
        .ok_or_else(|| "quality review blocker is not a string".to_string())?;
    if quality["state"] != "pending"
        || !quality_blocker.contains("final v0.4")
        || !quality_blocker.contains("release-review")
        || quality_blocker.contains("Phase 0")
    {
        return Err(
            "final release-review evidence was confused with resolved Phase 0 review evidence"
                .to_string(),
        );
    }
    Ok(())
}

#[test]
fn phase_zero_contract_defers_claim_eligible_campaigns() -> Result<(), String> {
    validate_phase_zero_campaign_boundary(INTELLIGENCE_DESIGN)?;

    for replacement in [
        "Phase 0 benchmarks a broad installable grammar pack",
        "Phase 0 and the optional-semantic phase evaluate",
    ] {
        let drifted = INTELLIGENCE_DESIGN.replacen(
            "Phase 0 freezes the closed native-grammar and versioned-WASM candidate inventory",
            replacement,
            1,
        );
        if validate_phase_zero_campaign_boundary(&drifted).is_ok() {
            return Err(format!(
                "Phase 0 release-campaign drift was accepted: {replacement}"
            ));
        }
    }
    Ok(())
}

#[test]
fn missing_release_source_does_not_block_phase_readiness() -> Result<(), String> {
    let mut scoreboard = parse(SCOREBOARD)?;
    for gate in scoreboard["phase_readiness_gates"]
        .as_array_mut()
        .ok_or_else(|| "phase_readiness_gates is not an array".to_string())?
    {
        gate["state"] = Value::String("pass".to_string());
        gate["blocker"] = Value::Null;
    }
    let mut sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    *sources.get_mut("capability_registry")? = SourceDocument::Missing;
    let projected = project_scoreboard(&scoreboard, &sources)?;
    if projected["source_artifacts"]["capability_registry"]["current_state"] != "missing"
        || projected["current_phase_exit"]["state"] != "pass"
        || projected["current_phase_exit"]["claim_state"] != "no-claim"
        || projected["current_release_acceptance"]["state"] != "blocked"
        || projected["current_release_acceptance"]["claim_state"] != "no-claim"
        || projected["claim_state"] != "no-claim"
    {
        return Err("final release evidence incorrectly affected Phase 0 readiness".to_string());
    }
    Ok(())
}

#[test]
fn missing_json_pointer_is_rejected() -> Result<(), String> {
    let scoreboard = parse(SCOREBOARD)?;
    let mut sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    let SourceDocument::Present(capabilities) = sources.get_mut("capability_registry")? else {
        return Err("capability registry unexpectedly missing".to_string());
    };
    capabilities
        .as_object_mut()
        .ok_or_else(|| "capability registry is not an object".to_string())?
        .remove("accepted_set_policy");
    let Err(error) = project_scoreboard(&scoreboard, &sources) else {
        return Err("a missing JSON pointer passed projection".to_string());
    };
    if !error.contains("does not resolve JSON pointer /accepted_set_policy") {
        return Err(format!("unexpected missing-pointer error: {error}"));
    }
    Ok(())
}

#[test]
fn inconsistent_projection_counts_are_rejected() -> Result<(), String> {
    let mut scoreboard = parse(SCOREBOARD)?;
    let expected_readiness_gates = EXPECTED_PHASE_READINESS_GATES.len();
    scoreboard["current_phase_exit"]["passing_readiness_gate_count"] =
        Value::from(expected_readiness_gates.saturating_sub(1));
    let sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    if validate_checked_scoreboard(&scoreboard, &sources).is_ok() {
        return Err("stale phase counts passed validation".to_string());
    }
    scoreboard["current_phase_exit"]["passing_readiness_gate_count"] =
        Value::from(expected_readiness_gates);
    scoreboard["current_phase_exit"]["state"] = Value::String("blocked".to_string());
    if validate_checked_scoreboard(&scoreboard, &sources).is_ok() {
        return Err("inconsistent phase state passed validation".to_string());
    }
    Ok(())
}

#[test]
fn missing_evidence_cannot_be_declared_pass() -> Result<(), String> {
    let mut scoreboard = parse(SCOREBOARD)?;
    let dimension = scoreboard["dimensions"]
        .as_array_mut()
        .and_then(|dimensions| {
            dimensions
                .iter_mut()
                .find(|dimension| dimension["id"] == "performance.full-scan-time")
        })
        .ok_or_else(|| "performance.full-scan-time dimension is missing".to_string())?;
    dimension["state"] = Value::String("pass".to_string());
    let sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    let Err(error) = project_scoreboard(&scoreboard, &sources) else {
        return Err("missing result evidence produced a passing dimension".to_string());
    };
    if !error.contains("cannot pass while a required source is missing") {
        return Err(format!("unexpected false-pass error: {error}"));
    }
    Ok(())
}

#[test]
fn every_non_pass_state_blocks_release_acceptance() -> Result<(), String> {
    let mut scoreboard = parse(SCOREBOARD)?;
    let mut sources = SourceDocuments::load(&repository_root(), &scoreboard)?;
    let expected_dimension_count = EXPECTED_DIMENSIONS.len();
    let current = project_scoreboard(&scoreboard, &sources)?;
    if current["dimension_count"] != expected_dimension_count
        || current["current_release_acceptance"]["required_dimension_count"]
            != expected_dimension_count
        || current["current_release_acceptance"]["passing_dimension_count"] != 0
        || current["current_release_acceptance"]["blocking_dimension_count"]
            != expected_dimension_count
        || current["current_release_acceptance"]["state"] != "blocked"
        || current["claim_state"] != "no-claim"
    {
        return Err("current release-acceptance decision is not blocked and no-claim".to_string());
    }

    for &source_id in SOURCE_IDS {
        if matches!(sources.get(source_id)?, SourceDocument::Missing) {
            *sources.get_mut(source_id)? = SourceDocument::Present(Value::Null);
        }
    }
    for dimension in scoreboard["dimensions"]
        .as_array_mut()
        .ok_or_else(|| "dimensions is not an array".to_string())?
    {
        dimension["state"] = Value::String("pass".to_string());
    }
    let projected_pass = project_scoreboard(&scoreboard, &sources)?;
    if projected_pass["current_release_acceptance"]["state"] != "pass"
        || projected_pass["current_release_acceptance"]["passing_dimension_count"]
            != expected_dimension_count
        || projected_pass["current_release_acceptance"]["blocking_dimension_count"] != 0
    {
        return Err("complete all-pass fixture did not pass release acceptance".to_string());
    }

    for (state_id, state) in [
        ("pending", DimensionState::Pending),
        ("missing", DimensionState::Missing),
        ("ineligible", DimensionState::Ineligible),
        ("fail", DimensionState::Fail),
        ("informational", DimensionState::Informational),
    ] {
        let mut candidate = scoreboard.clone();
        candidate["dimensions"][0]["state"] = Value::String(state_id.to_string());
        let projected = project_scoreboard(&candidate, &sources)?;
        if projected["current_release_acceptance"]["state"] != "blocked"
            || projected["current_release_acceptance"]["passing_dimension_count"]
                != expected_dimension_count.saturating_sub(1)
            || projected["current_release_acceptance"]["blocking_dimension_count"] != 1
            || projected["current_release_acceptance"]["blocking_states"][state_id] != 1
            || projected["claim_state"] != "no-claim"
        {
            return Err(format!(
                "blocking state {state:?} did not block release acceptance"
            ));
        }
    }

    let mut missing_dimension = scoreboard.clone();
    missing_dimension["dimensions"]
        .as_array_mut()
        .ok_or_else(|| "dimensions is not an array".to_string())?
        .pop();
    let Err(missing_dimension_error) = project_scoreboard(&missing_dimension, &sources) else {
        return Err("a missing required dimension passed release acceptance".to_string());
    };
    if !missing_dimension_error.contains("scoreboard dimension catalog drifted") {
        return Err(format!(
            "unexpected missing-dimension error: {missing_dimension_error}"
        ));
    }

    let mut hosted_gap = sources;
    *hosted_gap.get_mut("hosted_results")? = SourceDocument::Missing;
    let Err(hosted_error) = project_scoreboard(&scoreboard, &hosted_gap) else {
        return Err("missing hosted evidence passed release acceptance".to_string());
    };
    if !hosted_error.contains("cannot pass while a required source is missing") {
        return Err(format!("unexpected hosted-evidence error: {hosted_error}"));
    }
    Ok(())
}

#[test]
fn each_unready_phase_gate_blocks_phase_exit() -> Result<(), String> {
    let mut all_ready = parse(SCOREBOARD)?;
    for gate in all_ready["phase_readiness_gates"]
        .as_array_mut()
        .ok_or_else(|| "phase_readiness_gates is not an array".to_string())?
    {
        gate["state"] = Value::String("pass".to_string());
        gate["blocker"] = Value::Null;
    }
    let sources = SourceDocuments::load(&repository_root(), &all_ready)?;
    let ready = project_scoreboard(&all_ready, &sources)?;
    if ready["current_phase_exit"]["state"] != "pass"
        || ready["current_phase_exit"]["passing_readiness_gate_count"]
            != EXPECTED_PHASE_READINESS_GATES.len()
        || ready["current_release_acceptance"]["state"] != "blocked"
        || ready["claim_state"] != "no-claim"
    {
        return Err("all-ready Phase 0 fixture did not pass independently".to_string());
    }

    for (index, expected_id) in EXPECTED_PHASE_READINESS_GATES.iter().enumerate() {
        let mut candidate = all_ready.clone();
        candidate["phase_readiness_gates"][index]["state"] = Value::String("pending".to_string());
        candidate["phase_readiness_gates"][index]["blocker"] =
            Value::String(format!("{expected_id} remains unresolved"));
        let projected = project_scoreboard(&candidate, &sources)?;
        if projected["current_phase_exit"]["state"] != "blocked"
            || projected["current_phase_exit"]["passing_readiness_gate_count"]
                != EXPECTED_PHASE_READINESS_GATES.len().saturating_sub(1)
            || projected["current_phase_exit"]["blocking_readiness_gate_count"] != 1
            || projected["current_phase_exit"]["blocking_gate_ids"]
                != Value::Array(vec![Value::String((*expected_id).to_string())])
            || projected["claim_state"] != "no-claim"
        {
            return Err(format!(
                "unready Phase 0 gate {expected_id} did not block the phase exit"
            ));
        }
    }
    Ok(())
}
