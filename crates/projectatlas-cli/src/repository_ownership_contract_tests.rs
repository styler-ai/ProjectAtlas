//! Validate the need-driven repository ownership contract.

use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

/// Repository-intelligence contract compiled into the test binary.
const CONTRACT: &[u8] = include_bytes!(
    "../../../docs/benchmarks/projectatlas-v0.4-repository-intelligence-contracts.json"
);
/// Cargo dependency sections that can own direct path edges.
const CARGO_DEPENDENCY_SECTIONS: [&str; 3] =
    ["dependencies", "dev-dependencies", "build-dependencies"];
/// Workspace-only lint tool outside the product dependency flow.
const LINT_TOOL_PACKAGE: &str = "projectatlas-lints";
/// Candidate crate that must remain absent without a real consumer.
const INDEX_CANDIDATE_PACKAGE: &str = "projectatlas-index";
/// Diagnostic proving declared dependency cycles fail before live Cargo reconciliation.
const DEPENDENCY_CYCLE_DIAGNOSTIC: &str = "product-crate ownership contains a dependency cycle";

/// Need-driven ownership guidance without a speculative topology migration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnershipMap {
    /// Current implementation disposition.
    implementation_state: ImplementationState,
    /// Trigger for extracting an ownership boundary.
    split_policy: SplitPolicy,
    /// Whether a repository-wide topology refactor is permitted.
    all_at_once_refactor_allowed: bool,
    /// Existing crate ownership boundaries.
    owners: Vec<OwnerAssignment>,
    /// Test placement and workflow ownership.
    test_ownership: Vec<TestOwnership>,
    /// Rule for creating a separately consumable index crate.
    new_crate_policy: NewCratePolicy,
    /// Workspace tools intentionally outside the product dependency flow.
    independent_workspace_tools: Vec<IndependentWorkspaceTool>,
    /// Scaffolding rejected before a demonstrated need exists.
    rejected_scaffolding: Vec<RejectedScaffolding>,
}

/// Current implementation disposition for the ownership map.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum ImplementationState {
    /// Ownership is recorded while production modules remain unchanged.
    OwnershipRecordedWithoutRefactor,
}

/// Accepted trigger for extracting a module boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum SplitPolicy {
    /// Extract only the smallest required boundary on its first feature touch.
    FirstFeatureTouchOnly,
}

/// One existing crate and the responsibilities it owns.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerAssignment {
    /// Existing crate identity.
    owner: CrateOwner,
    /// Exact responsibility identities assigned to the crate.
    responsibilities: Vec<Responsibility>,
    /// Direct product-crate dependencies.
    depends_on: Vec<CrateOwner>,
}

/// Existing `ProjectAtlas` crate owners in the repository-intelligence flow.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
enum CrateOwner {
    /// Graph and language domain contracts.
    #[serde(rename = "projectatlas-core")]
    Core,
    /// Schema, migration, and persistence concerns.
    #[serde(rename = "projectatlas-db")]
    Database,
    /// Gitignore-aware discovery and stable file identity.
    #[serde(rename = "projectatlas-fs")]
    Filesystem,
    /// Language registry, extraction, and resolution concerns.
    #[serde(rename = "projectatlas-symbols")]
    Symbols,
    /// Agent-facing repository-intelligence use cases.
    #[serde(rename = "projectatlas-service")]
    Service,
    /// Index/watch orchestration and public adapters.
    #[serde(rename = "projectatlas-cli")]
    Cli,
}

impl CrateOwner {
    /// Return the Cargo package identity for this product owner.
    const fn package_name(self) -> &'static str {
        match self {
            Self::Core => "projectatlas-core",
            Self::Database => "projectatlas-db",
            Self::Filesystem => "projectatlas-fs",
            Self::Symbols => "projectatlas-symbols",
            Self::Service => "projectatlas-service",
            Self::Cli => "projectatlas-cli",
        }
    }
}

/// Closed responsibility catalog for the intended ownership map.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum Responsibility {
    /// Typed graph-domain contracts.
    GraphDomainContracts,
    /// Typed language-domain contracts.
    LanguageDomainContracts,
    /// Database schema and migration ownership.
    SchemaAndMigrations,
    /// Typed graph persistence.
    GraphPersistence,
    /// Lexical search indexes.
    LexicalIndexes,
    /// Repository discovery that inherits `.gitignore` dynamically.
    GitignoreAwareDiscovery,
    /// Stable file-content identity.
    FileIdentity,
    /// Generated and embedded language registry.
    LanguageRegistry,
    /// Syntax and manifest fact extraction.
    SyntaxExtraction,
    /// Cross-file and provider-backed resolution.
    SemanticResolution,
    /// Language-specific adapters.
    LanguageAdapters,
    /// Ranking and retrieval use cases.
    RankingAndRetrieval,
    /// Summary and exact-slice use cases.
    SummaryAndSlice,
    /// Relations and bounded graph-analysis use cases.
    GraphRelationsAndAnalysis,
    /// Full, incremental, and watch orchestration.
    IndexAndWatchOrchestration,
    /// Command-line and MCP adapters.
    CliAndMcpAdapters,
}

/// One test scope and its placement rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TestOwnership {
    /// Test scope.
    scope: TestScope,
    /// Owning placement rule.
    owner_rule: TestOwnerRule,
}

/// Test scopes covered by the ownership guidance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum TestScope {
    /// Unit tests for domain and service behavior.
    Unit,
    /// Real-process boundary tests.
    Process,
    /// Command-line workflow tests.
    Cli,
    /// MCP workflow tests.
    Mcp,
    /// Packaged installer tests.
    Installer,
}

/// Accepted test placement rules.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum TestOwnerRule {
    /// Keep focused unit tests beside the owning logic.
    BesideOwningLogic,
    /// Keep boundary tests with the affected workflow family.
    AffectedWorkflowFamily,
}

/// Need and consumer gates for a possible index-orchestration crate.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewCratePolicy {
    /// Only currently named candidate crate.
    candidate: NewCrateCandidate,
    /// Current candidate state.
    state: NewCrateState,
    /// Evidence required before crate creation.
    creation_trigger: NewCrateTrigger,
    /// Whether an imagined consumer satisfies the gate.
    hypothetical_consumer_sufficient: bool,
}

/// Closed candidate catalog for a possible new crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum NewCrateCandidate {
    /// Separately consumable scan/index orchestration.
    #[serde(rename = "projectatlas-index")]
    ProjectAtlasIndex,
}

/// Current state of the possible new crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum NewCrateState {
    /// No crate is created by this ownership-recording task.
    NotCreated,
}

/// Evidence required before creating a new crate.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum NewCrateTrigger {
    /// A demonstrated boundary and a real independent consumer both exist.
    DemonstratedBoundaryAndIndependentConsumer,
}

/// One workspace tool intentionally independent of the product dependency flow.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndependentWorkspaceTool {
    /// Independent crate identity.
    owner: IndependentToolOwner,
    /// Tool-only responsibility.
    responsibility: IndependentToolResponsibility,
    /// Required dependency-DAG relationship.
    dependency_rule: IndependentToolDependencyRule,
}

/// Closed independent workspace-tool catalog.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum IndependentToolOwner {
    /// Source-policy lint executable.
    #[serde(rename = "projectatlas-lints")]
    ProjectAtlasLints,
}

/// Responsibility owned by an independent workspace tool.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum IndependentToolResponsibility {
    /// Enforce repository-owned source policy.
    SourcePolicyEnforcement,
}

/// Dependency relationship required of an independent workspace tool.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum IndependentToolDependencyRule {
    /// Neither side of the product flow may acquire an edge to this tool.
    NoProductFlowEdges,
}

/// Speculative structures forbidden by the first-touch policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "kebab-case")]
enum RejectedScaffolding {
    /// Empty placeholder modules.
    EmptyModules,
    /// Traits or factories with only one implementation.
    OneImplementationTraitsOrFactories,
    /// Generic providers without demonstrated variability.
    PrematureGenericProviders,
    /// Crates created only to resemble a target directory map.
    TopologyOnlyCrates,
}

/// Direct local dependency graph derived from the live Cargo workspace.
#[derive(Debug)]
struct WorkspaceDependencyGraph {
    /// Exact workspace package identities.
    members: BTreeSet<String>,
    /// Deduplicated direct workspace-member dependencies by package.
    dependencies: BTreeMap<String, BTreeSet<String>>,
}

/// Return the canonical repository root containing the workspace manifest.
fn repository_root() -> Result<PathBuf, Box<dyn Error>> {
    Ok(fs::canonicalize(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."),
    )?)
}

/// Parse one Cargo manifest with the workspace's existing canonical TOML parser.
fn parse_manifest(path: &Path) -> Result<TomlValue, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    Ok(toml::from_str(&source)?)
}

/// Resolve a dependency declaration to its local path when it is path-backed.
fn local_dependency_path(
    dependency_name: &str,
    declaration: &TomlValue,
    member_dir: &Path,
    workspace_root: &Path,
    workspace_dependencies: &toml::value::Table,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let Some(declaration) = declaration.as_table() else {
        return Ok(None);
    };
    if let Some(path) = declaration.get("path").and_then(TomlValue::as_str) {
        return Ok(Some(member_dir.join(path)));
    }
    if declaration.get("workspace").and_then(TomlValue::as_bool) != Some(true) {
        return Ok(None);
    }
    let inherited = workspace_dependencies.get(dependency_name).ok_or_else(|| {
        io::Error::other(format!(
            "workspace dependency {dependency_name} is not declared"
        ))
    })?;
    Ok(inherited
        .as_table()
        .and_then(|table| table.get("path"))
        .and_then(TomlValue::as_str)
        .map(|path| workspace_root.join(path)))
}

/// Collect direct workspace-member path dependencies from one Cargo table.
fn collect_dependency_sections(
    container: &toml::value::Table,
    member_dir: &Path,
    workspace_root: &Path,
    workspace_dependencies: &toml::value::Table,
    package_by_path: &BTreeMap<PathBuf, String>,
    dependencies: &mut BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    for section_name in CARGO_DEPENDENCY_SECTIONS {
        let Some(section) = container.get(section_name).and_then(TomlValue::as_table) else {
            continue;
        };
        for (dependency_name, declaration) in section {
            let Some(path) = local_dependency_path(
                dependency_name,
                declaration,
                member_dir,
                workspace_root,
                workspace_dependencies,
            )?
            else {
                continue;
            };
            let normalized = fs::canonicalize(path)?;
            if let Some(package) = package_by_path.get(&normalized) {
                dependencies.insert(package.clone());
            } else {
                require(
                    !normalized.starts_with(workspace_root),
                    format!(
                        "workspace-local dependency path {} is not a workspace member",
                        normalized.display()
                    ),
                )?;
            }
        }
    }
    Ok(())
}

/// Derive normalized direct local dependency edges from every workspace member manifest.
fn workspace_dependency_graph() -> Result<WorkspaceDependencyGraph, Box<dyn Error>> {
    let root = repository_root()?;
    let root_manifest = parse_manifest(&root.join("Cargo.toml"))?;
    let workspace = root_manifest
        .get("workspace")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| io::Error::other("root Cargo.toml has no workspace table"))?;
    let member_rows = workspace
        .get("members")
        .and_then(TomlValue::as_array)
        .ok_or_else(|| io::Error::other("workspace members are missing"))?;
    let workspace_dependencies = workspace
        .get("dependencies")
        .and_then(TomlValue::as_table)
        .ok_or_else(|| io::Error::other("workspace dependencies are missing"))?;

    let mut manifests = BTreeMap::new();
    let mut package_by_path = BTreeMap::new();
    for member in member_rows {
        let relative = member
            .as_str()
            .ok_or_else(|| io::Error::other("workspace member path is not a string"))?;
        let member_dir = fs::canonicalize(root.join(relative))?;
        let manifest = parse_manifest(&member_dir.join("Cargo.toml"))?;
        let package = manifest
            .get("package")
            .and_then(TomlValue::as_table)
            .and_then(|table| table.get("name"))
            .and_then(TomlValue::as_str)
            .ok_or_else(|| {
                io::Error::other(format!("workspace member {relative} has no package name"))
            })?
            .to_owned();
        require(
            package_by_path
                .insert(member_dir.clone(), package.clone())
                .is_none(),
            format!("workspace member path {relative} is duplicated"),
        )?;
        require(
            manifests
                .insert(package.clone(), (member_dir, manifest))
                .is_none(),
            format!("workspace package {package} is duplicated"),
        )?;
    }

    let mut dependencies = BTreeMap::new();
    for (package, (member_dir, manifest)) in &manifests {
        let manifest_table = manifest
            .as_table()
            .ok_or_else(|| io::Error::other(format!("manifest for {package} is not a table")))?;
        let mut package_dependencies = BTreeSet::new();
        collect_dependency_sections(
            manifest_table,
            member_dir,
            &root,
            workspace_dependencies,
            &package_by_path,
            &mut package_dependencies,
        )?;
        if let Some(targets) = manifest.get("target").and_then(TomlValue::as_table) {
            for target in targets.values().filter_map(TomlValue::as_table) {
                collect_dependency_sections(
                    target,
                    member_dir,
                    &root,
                    workspace_dependencies,
                    &package_by_path,
                    &mut package_dependencies,
                )?;
            }
        }
        require(
            dependencies
                .insert(package.clone(), package_dependencies)
                .is_none(),
            format!("workspace dependency row {package} is duplicated"),
        )?;
    }

    Ok(WorkspaceDependencyGraph {
        members: manifests.into_keys().collect(),
        dependencies,
    })
}

/// Return whether a closed dependency graph has no multi-node cycle.
fn graph_is_acyclic(graph: &BTreeMap<CrateOwner, BTreeSet<CrateOwner>>) -> bool {
    let mut remaining = graph.clone();
    while let Some(owner) = remaining
        .iter()
        .find_map(|(owner, dependencies)| dependencies.is_empty().then_some(*owner))
    {
        remaining.remove(&owner);
        for dependencies in remaining.values_mut() {
            dependencies.remove(&owner);
        }
    }
    remaining.is_empty()
}

/// Decode the ownership map from a complete contract value.
fn ownership_map(value: &Value) -> Result<OwnershipMap, Box<dyn Error>> {
    let map = value
        .get("ownership_map")
        .ok_or_else(|| io::Error::other("ownership_map is missing"))?;
    Ok(serde_json::from_value(map.clone())?)
}

/// Validate exact owners, live Cargo edges, test placement, and need-driven split policy.
fn validate_ownership_map(
    map: &OwnershipMap,
    workspace_graph: &WorkspaceDependencyGraph,
) -> Result<(), Box<dyn Error>> {
    require(
        map.implementation_state == ImplementationState::OwnershipRecordedWithoutRefactor,
        "ownership recording performed or claimed a production refactor",
    )?;
    require(
        map.split_policy == SplitPolicy::FirstFeatureTouchOnly,
        "module extraction is not limited to first feature touch",
    )?;
    require(
        !map.all_at_once_refactor_allowed,
        "all-at-once repository topology refactor is permitted",
    )?;

    let expected_owners = BTreeMap::from([
        (
            CrateOwner::Core,
            BTreeSet::from([
                Responsibility::GraphDomainContracts,
                Responsibility::LanguageDomainContracts,
            ]),
        ),
        (
            CrateOwner::Database,
            BTreeSet::from([
                Responsibility::SchemaAndMigrations,
                Responsibility::GraphPersistence,
                Responsibility::LexicalIndexes,
            ]),
        ),
        (
            CrateOwner::Filesystem,
            BTreeSet::from([
                Responsibility::GitignoreAwareDiscovery,
                Responsibility::FileIdentity,
            ]),
        ),
        (
            CrateOwner::Symbols,
            BTreeSet::from([
                Responsibility::LanguageRegistry,
                Responsibility::SyntaxExtraction,
                Responsibility::SemanticResolution,
                Responsibility::LanguageAdapters,
            ]),
        ),
        (
            CrateOwner::Service,
            BTreeSet::from([
                Responsibility::RankingAndRetrieval,
                Responsibility::SummaryAndSlice,
                Responsibility::GraphRelationsAndAnalysis,
            ]),
        ),
        (
            CrateOwner::Cli,
            BTreeSet::from([
                Responsibility::IndexAndWatchOrchestration,
                Responsibility::CliAndMcpAdapters,
            ]),
        ),
    ]);
    let mut actual_owners = BTreeMap::new();
    let mut actual_dependencies = BTreeMap::new();
    for assignment in &map.owners {
        let responsibilities = assignment
            .responsibilities
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        require(
            responsibilities.len() == assignment.responsibilities.len(),
            "one owner repeats a responsibility",
        )?;
        require(
            actual_owners
                .insert(assignment.owner, responsibilities)
                .is_none(),
            "one crate owner appears more than once",
        )?;
        let dependencies = assignment
            .depends_on
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        require(
            dependencies.len() == assignment.depends_on.len(),
            "one crate owner repeats a dependency edge",
        )?;
        require(
            !dependencies.contains(&assignment.owner),
            "one crate owner depends on itself",
        )?;
        require(
            actual_dependencies
                .insert(assignment.owner, dependencies)
                .is_none(),
            "one crate dependency row appears more than once",
        )?;
    }
    require(
        actual_owners == expected_owners,
        "crate ownership or responsibilities drifted",
    )?;
    require(
        graph_is_acyclic(&actual_dependencies),
        DEPENDENCY_CYCLE_DIAGNOSTIC,
    )?;

    let product_packages = actual_owners
        .keys()
        .map(|owner| owner.package_name().to_owned())
        .collect::<BTreeSet<_>>();
    let mut expected_workspace_members = product_packages.clone();
    expected_workspace_members.insert(LINT_TOOL_PACKAGE.to_owned());
    require(
        workspace_graph.members == expected_workspace_members,
        "live Cargo workspace members drifted from declared product and tool ownership",
    )?;
    require(
        workspace_graph
            .dependencies
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
            == workspace_graph.members,
        "one live Cargo workspace member has no dependency row",
    )?;
    require(
        !workspace_graph.members.contains(INDEX_CANDIDATE_PACKAGE)
            && workspace_graph
                .dependencies
                .values()
                .all(|dependencies| !dependencies.contains(INDEX_CANDIDATE_PACKAGE)),
        "projectatlas-index exists without the required independent consumer",
    )?;
    let lint_dependencies = workspace_graph
        .dependencies
        .get(LINT_TOOL_PACKAGE)
        .ok_or_else(|| io::Error::other("projectatlas-lints dependency row is missing"))?;
    require(
        lint_dependencies.is_disjoint(&product_packages)
            && product_packages.iter().all(|package| {
                workspace_graph
                    .dependencies
                    .get(package)
                    .is_some_and(|dependencies| !dependencies.contains(LINT_TOOL_PACKAGE))
            }),
        "projectatlas-lints has a product-flow dependency edge",
    )?;

    let contract_dependencies = actual_dependencies
        .iter()
        .map(|(owner, dependencies)| {
            (
                owner.package_name().to_owned(),
                dependencies
                    .iter()
                    .map(|dependency| dependency.package_name().to_owned())
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut live_product_dependencies = BTreeMap::new();
    for package in &product_packages {
        let dependencies = workspace_graph.dependencies.get(package).ok_or_else(|| {
            io::Error::other(format!("live Cargo dependency row {package} is missing"))
        })?;
        live_product_dependencies.insert(
            package.clone(),
            dependencies
                .intersection(&product_packages)
                .cloned()
                .collect::<BTreeSet<_>>(),
        );
    }
    require(
        contract_dependencies == live_product_dependencies,
        "declared product-crate dependencies drifted from live Cargo path dependencies",
    )?;

    let expected_test_ownership = BTreeMap::from([
        (TestScope::Unit, TestOwnerRule::BesideOwningLogic),
        (TestScope::Process, TestOwnerRule::AffectedWorkflowFamily),
        (TestScope::Cli, TestOwnerRule::AffectedWorkflowFamily),
        (TestScope::Mcp, TestOwnerRule::AffectedWorkflowFamily),
        (TestScope::Installer, TestOwnerRule::AffectedWorkflowFamily),
    ]);
    let mut actual_test_ownership = BTreeMap::new();
    for assignment in &map.test_ownership {
        require(
            actual_test_ownership
                .insert(assignment.scope, assignment.owner_rule)
                .is_none(),
            "one test scope appears more than once",
        )?;
    }
    require(
        actual_test_ownership == expected_test_ownership,
        "test ownership or placement drifted",
    )?;

    require(
        map.new_crate_policy.candidate == NewCrateCandidate::ProjectAtlasIndex
            && map.new_crate_policy.state == NewCrateState::NotCreated
            && map.new_crate_policy.creation_trigger
                == NewCrateTrigger::DemonstratedBoundaryAndIndependentConsumer
            && !map.new_crate_policy.hypothetical_consumer_sufficient,
        "new-crate creation is not gated by a demonstrated independent consumer",
    )?;

    require(
        map.independent_workspace_tools.len() == 1
            && map.independent_workspace_tools[0].owner == IndependentToolOwner::ProjectAtlasLints
            && map.independent_workspace_tools[0].responsibility
                == IndependentToolResponsibility::SourcePolicyEnforcement
            && map.independent_workspace_tools[0].dependency_rule
                == IndependentToolDependencyRule::NoProductFlowEdges,
        "independent workspace-tool ownership or dependency rule drifted",
    )?;

    let rejected = map
        .rejected_scaffolding
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    require(
        rejected.len() == map.rejected_scaffolding.len(),
        "rejected scaffolding contains duplicates",
    )?;
    require(
        rejected
            == BTreeSet::from([
                RejectedScaffolding::EmptyModules,
                RejectedScaffolding::OneImplementationTraitsOrFactories,
                RejectedScaffolding::PrematureGenericProviders,
                RejectedScaffolding::TopologyOnlyCrates,
            ]),
        "speculative scaffolding policy drifted",
    )
}

/// Fail a focused contract test with an actionable message.
fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Return one declared owner's mutable dependency list for adversarial tests.
fn dependencies_mut<'a>(
    value: &'a mut Value,
    owner: &str,
) -> Result<&'a mut Vec<Value>, Box<dyn Error>> {
    let owners = value["ownership_map"]["owners"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("owners is not an array"))?;
    let assignment = owners
        .iter_mut()
        .find(|assignment| assignment["owner"].as_str() == Some(owner))
        .ok_or_else(|| io::Error::other(format!("owner {owner} is missing")))?;
    assignment["depends_on"].as_array_mut().ok_or_else(|| {
        io::Error::other(format!("owner {owner} dependencies are not an array")).into()
    })
}

/// ARRI-3.1: ownership guidance remains complete and need-driven.
#[test]
fn arri_3_1_ownership_map_is_need_driven() -> Result<(), Box<dyn Error>> {
    let contract: Value = serde_json::from_slice(CONTRACT)?;
    let workspace_graph = workspace_dependency_graph()?;
    validate_ownership_map(&ownership_map(&contract)?, &workspace_graph)?;

    let mut missing_owner = contract.clone();
    missing_owner["ownership_map"]["owners"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("owners is not an array"))?
        .pop();
    require(
        ownership_map(&missing_owner)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted a missing crate owner",
    )?;

    let mut missing_test_owner = contract.clone();
    missing_test_owner["ownership_map"]["test_ownership"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("test_ownership is not an array"))?
        .pop();
    require(
        ownership_map(&missing_test_owner)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted incomplete test ownership",
    )?;

    let mut all_at_once = contract.clone();
    all_at_once["ownership_map"]["all_at_once_refactor_allowed"] = Value::Bool(true);
    require(
        ownership_map(&all_at_once)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted an all-at-once refactor",
    )?;

    let mut hypothetical_owner = contract;
    hypothetical_owner["ownership_map"]["owners"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("owners is not an array"))?
        .push(json!({
            "owner": "projectatlas-index",
            "responsibilities": ["index-and-watch-orchestration"],
            "depends_on": []
        }));
    require(
        ownership_map(&hypothetical_owner)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map assigned production work to a hypothetical crate",
    )?;

    let mut duplicate_edge = serde_json::from_slice(CONTRACT)?;
    dependencies_mut(&mut duplicate_edge, "projectatlas-db")?
        .push(Value::String("projectatlas-core".to_owned()));
    require(
        ownership_map(&duplicate_edge)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted a duplicate dependency edge",
    )?;

    let mut self_edge = serde_json::from_slice(CONTRACT)?;
    dependencies_mut(&mut self_edge, "projectatlas-core")?
        .push(Value::String("projectatlas-core".to_owned()));
    require(
        ownership_map(&self_edge)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted a self dependency edge",
    )?;

    let mut dependency_cycle = serde_json::from_slice(CONTRACT)?;
    dependencies_mut(&mut dependency_cycle, "projectatlas-cli")?
        .retain(|dependency| dependency.as_str() != Some("projectatlas-core"));
    dependencies_mut(&mut dependency_cycle, "projectatlas-core")?
        .push(Value::String("projectatlas-cli".to_owned()));
    let cycle_error = validate_ownership_map(&ownership_map(&dependency_cycle)?, &workspace_graph)
        .err()
        .ok_or_else(|| {
            io::Error::other("ownership map accepted a declared-owner dependency cycle")
        })?;
    require(
        cycle_error.to_string() == DEPENDENCY_CYCLE_DIAGNOSTIC,
        "declared-owner cycle did not fail at the acyclicity gate",
    )?;

    let mut undeclared_edge = serde_json::from_slice(CONTRACT)?;
    dependencies_mut(&mut undeclared_edge, "projectatlas-cli")?
        .push(Value::String("projectatlas-index".to_owned()));
    require(
        ownership_map(&undeclared_edge)
            .and_then(|map| validate_ownership_map(&map, &workspace_graph))
            .is_err(),
        "ownership map accepted an undeclared dependency edge",
    )
}
