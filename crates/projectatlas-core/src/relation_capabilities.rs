//! Own the accepted relation-family inventory and its content-free projections.

use crate::graph::{ExtendedRelationKind, GraphRelationKind};
use crate::symbols::RelationKind;
use blake3::Hasher;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::{self, Write as _};

/// Version of the accepted relation-family inventory.
pub const ACCEPTED_RELATION_FAMILY_INVENTORY_VERSION: u32 = 1;
/// Frozen digest of the accepted version-one inventory.
pub const ACCEPTED_RELATION_FAMILY_INVENTORY_V1_DIGEST: &str =
    "aae2984ba57c42a387081c0f601025f127a32104c2662cb8fa9855b4cfd61bcc";

/// Stable product-level relation family.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationFamilyId {
    /// Declarations, containment, imports, calls, and references.
    StructuralType,
    /// Package and manifest dependencies.
    PackageManifest,
    /// Test-to-subject relationships.
    Test,
    /// Route and protocol registration to handlers.
    RouteProtocol,
    /// Configuration or environment selection.
    ConfigurationEnvironment,
    /// Deployment and infrastructure provisioning.
    DeploymentInfrastructure,
    /// Bounded statically visible reads and writes.
    StaticDataAccess,
    /// Optional inferred semantic similarity.
    InferredSimilarity,
    /// Optional inferred version-control co-change.
    InferredCoChange,
}

impl RelationFamilyId {
    /// Return the stable payload and documentation label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StructuralType => "structural_type",
            Self::PackageManifest => "package_manifest",
            Self::Test => "test",
            Self::RouteProtocol => "route_protocol",
            Self::ConfigurationEnvironment => "configuration_environment",
            Self::DeploymentInfrastructure => "deployment_infrastructure",
            Self::StaticDataAccess => "static_data_access",
            Self::InferredSimilarity => "inferred_similarity",
            Self::InferredCoChange => "inferred_co_change",
        }
    }
}

/// Whether one accepted family is active or separately gated.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationFamilyState {
    /// Produced, persisted, invalidated, queryable, and covered now.
    Active,
    /// Typed but unavailable until its optional capability passes its own gates.
    OptionalDisabled,
}

impl RelationFamilyState {
    /// Return the stable settings label.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::OptionalDisabled => "optional_disabled",
        }
    }
}

/// One end-to-end accepted relation-family contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RelationFamilyCapability {
    /// Stable product family.
    pub id: RelationFamilyId,
    /// Current lifecycle state.
    pub state: RelationFamilyState,
    /// Persisted graph relation kinds owned by the family.
    pub graph_relations: &'static [GraphRelationKind],
    /// Responsibility owner that emits current facts.
    pub producer: &'static str,
    /// Typed persistence owner.
    pub persistence: &'static str,
    /// Invalidation owner.
    pub invalidation: &'static str,
    /// Existing bounded query consumer.
    pub query_consumer: &'static str,
    /// Positive fixture locator.
    pub positive_fixture: &'static str,
    /// Negative or ambiguity fixture locator.
    pub negative_fixture: &'static str,
    /// Honest coverage statement.
    pub coverage: &'static str,
}

/// Structural and type relation kinds.
const STRUCTURAL_RELATIONS: &[GraphRelationKind] = &[
    GraphRelationKind::Legacy(RelationKind::Contains),
    GraphRelationKind::Legacy(RelationKind::Imports),
    GraphRelationKind::Legacy(RelationKind::Calls),
];
/// Package and manifest relation kinds.
const PACKAGE_RELATIONS: &[GraphRelationKind] =
    &[GraphRelationKind::Legacy(RelationKind::DependsOn)];
/// Test relation kinds.
const TEST_RELATIONS: &[GraphRelationKind] =
    &[GraphRelationKind::Extended(ExtendedRelationKind::Tests)];
/// Route and protocol relation kinds.
const ROUTE_RELATIONS: &[GraphRelationKind] =
    &[GraphRelationKind::Extended(ExtendedRelationKind::RoutesTo)];
/// Configuration and environment relation kinds.
const CONFIGURATION_RELATIONS: &[GraphRelationKind] = &[GraphRelationKind::Extended(
    ExtendedRelationKind::Configures,
)];
/// Deployment and infrastructure relation kinds.
const DEPLOYMENT_RELATIONS: &[GraphRelationKind] =
    &[GraphRelationKind::Extended(ExtendedRelationKind::Deploys)];
/// Static data-access relation kinds.
const DATA_ACCESS_RELATIONS: &[GraphRelationKind] = &[
    GraphRelationKind::Extended(ExtendedRelationKind::Reads),
    GraphRelationKind::Extended(ExtendedRelationKind::Writes),
];
/// Empty persisted set for separately gated inferred families.
const NO_RELATIONS: &[GraphRelationKind] = &[];

/// Accepted direct and separately gated inferred relation families.
pub static RELATION_FAMILY_CAPABILITIES: &[RelationFamilyCapability] = &[
    RelationFamilyCapability {
        id: RelationFamilyId::StructuralType,
        state: RelationFamilyState::Active,
        graph_relations: STRUCTURAL_RELATIONS,
        producer: "projectatlas-symbols",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "provider-owned structural facts with parser-strength coverage",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::PackageManifest,
        state: RelationFamilyState::Active,
        graph_relations: PACKAGE_RELATIONS,
        producer: "projectatlas-symbols/manifest",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "accepted manifest dependencies only",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::Test,
        state: RelationFamilyState::Active,
        graph_relations: TEST_RELATIONS,
        producer: "projectatlas-cli/graph-projection",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "statically resolved calls and imports from recognized test paths",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::RouteProtocol,
        state: RelationFamilyState::Active,
        graph_relations: ROUTE_RELATIONS,
        producer: "projectatlas-cli/graph-projection",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "recognized static route registrations only; dynamic registrations abstain",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::ConfigurationEnvironment,
        state: RelationFamilyState::Active,
        graph_relations: CONFIGURATION_RELATIONS,
        producer: "projectatlas-cli/graph-projection",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "recognized configuration files and static environment keys; values are excluded",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::DeploymentInfrastructure,
        state: RelationFamilyState::Active,
        graph_relations: DEPLOYMENT_RELATIONS,
        producer: "projectatlas-cli/graph-projection",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "recognized infrastructure configuration files; resource detail remains partial",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::StaticDataAccess,
        state: RelationFamilyState::Active,
        graph_relations: DATA_ACCESS_RELATIONS,
        producer: "projectatlas-cli/graph-projection",
        persistence: "projectatlas-db/repository-graph",
        invalidation: "projectatlas-cli/graph-projection",
        query_consumer: "projectatlas-service/relations",
        positive_fixture: "graph_projection::accepted_relation_families_publish_and_reopen",
        negative_fixture: "graph_projection::accepted_relation_families_abstain_without_static_evidence",
        coverage: "recognized static read and write calls only; dynamic paths abstain",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::InferredSimilarity,
        state: RelationFamilyState::OptionalDisabled,
        graph_relations: NO_RELATIONS,
        producer: "optional-semantic-lifecycle",
        persistence: "none",
        invalidation: "optional-semantic-lifecycle",
        query_consumer: "explicit-semantic-mode",
        positive_fixture: "deferred-until-quality-gates-pass",
        negative_fixture: "default-core-rejects-unavailable-semantic-mode",
        coverage: "not advertised while optional quality and resource gates are unmet",
    },
    RelationFamilyCapability {
        id: RelationFamilyId::InferredCoChange,
        state: RelationFamilyState::OptionalDisabled,
        graph_relations: NO_RELATIONS,
        producer: "optional-vcs-analysis",
        persistence: "none",
        invalidation: "call-scoped-vcs-identity",
        query_consumer: "explicit-vcs-analysis-mode",
        positive_fixture: "deferred-until-quality-gates-pass",
        negative_fixture: "default-analysis-does-not-infer-co-change",
        coverage: "not advertised while optional quality and freshness gates are unmet",
    },
];

/// Content-free settings projection of the accepted inventory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelationFamilyInventoryReport {
    /// Inventory schema version.
    pub version: u32,
    /// Digest over every accepted owner and coverage field.
    pub digest: String,
    /// Number of active direct families.
    pub active_families: u32,
    /// Number of separately gated inferred families.
    pub optional_disabled_families: u32,
}

/// Validate the accepted inventory and return its stable digest.
///
/// # Errors
///
/// Returns an error when a row is incomplete, duplicated, or overlaps another
/// accepted family's persisted relation kinds.
pub fn validate_relation_family_inventory() -> Result<String, &'static str> {
    let mut ids = BTreeSet::new();
    let mut relation_kinds = BTreeSet::new();
    for row in RELATION_FAMILY_CAPABILITIES {
        if !ids.insert(row.id)
            || row.producer.is_empty()
            || row.persistence.is_empty()
            || row.invalidation.is_empty()
            || row.query_consumer.is_empty()
            || row.positive_fixture.is_empty()
            || row.negative_fixture.is_empty()
            || row.coverage.is_empty()
        {
            return Err("relation-family inventory contains an incomplete or duplicate row");
        }
        match row.state {
            RelationFamilyState::Active if row.graph_relations.is_empty() => {
                return Err("active relation-family row has no persisted relation kind");
            }
            RelationFamilyState::OptionalDisabled if !row.graph_relations.is_empty() => {
                return Err("disabled inferred relation-family row advertises persisted facts");
            }
            RelationFamilyState::Active | RelationFamilyState::OptionalDisabled => {}
        }
        for relation in row.graph_relations {
            if !relation_kinds.insert(relation.as_str()) {
                return Err("persisted relation kind is owned by more than one family");
            }
        }
    }
    Ok(relation_family_inventory_digest())
}

/// Return the stable accepted-inventory digest.
#[must_use]
pub fn relation_family_inventory_digest() -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"projectatlas-relation-family-inventory-v1\0");
    for row in RELATION_FAMILY_CAPABILITIES {
        for value in [
            row.id.as_str(),
            row.state.as_str(),
            row.producer,
            row.persistence,
            row.invalidation,
            row.query_consumer,
            row.positive_fixture,
            row.negative_fixture,
            row.coverage,
        ] {
            hasher.update(value.as_bytes());
            hasher.update(&[0]);
        }
        for relation in row.graph_relations {
            hasher.update(relation.as_str().as_bytes());
            hasher.update(&[0]);
        }
        hasher.update(&[0xff]);
    }
    hasher.finalize().to_hex().to_string()
}

/// Return the content-free settings report.
///
#[must_use]
pub fn relation_family_inventory_report() -> RelationFamilyInventoryReport {
    let active_families = RELATION_FAMILY_CAPABILITIES
        .iter()
        .filter(|row| row.state == RelationFamilyState::Active)
        .count();
    let optional_disabled_families = RELATION_FAMILY_CAPABILITIES
        .iter()
        .filter(|row| row.state == RelationFamilyState::OptionalDisabled)
        .count();
    RelationFamilyInventoryReport {
        version: ACCEPTED_RELATION_FAMILY_INVENTORY_VERSION,
        digest: relation_family_inventory_digest(),
        active_families: u32::try_from(active_families).unwrap_or(u32::MAX),
        optional_disabled_families: u32::try_from(optional_disabled_families).unwrap_or(u32::MAX),
    }
}

/// Render the accepted inventory as its generated Markdown authority.
///
/// # Errors
///
/// Returns formatting errors from the in-memory Markdown writer.
pub fn render_relation_support_markdown() -> Result<String, fmt::Error> {
    let report = relation_family_inventory_report();
    let mut output = String::new();
    writeln!(output, "# Relation Family Support\n")?;
    writeln!(
        output,
        "Generated from accepted relation-family inventory v{} (`{}`).\n",
        report.version, report.digest
    )?;
    writeln!(
        output,
        "| Family | State | Persisted graph relations | Coverage |"
    )?;
    writeln!(output, "| --- | --- | --- | --- |")?;
    for row in RELATION_FAMILY_CAPABILITIES {
        let relations = if row.graph_relations.is_empty() {
            "—".to_owned()
        } else {
            row.graph_relations
                .iter()
                .map(|relation| format!("`{}`", relation.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        writeln!(
            output,
            "| `{}` | `{}` | {} | {} |",
            row.id.as_str(),
            row.state.as_str(),
            relations,
            row.coverage
        )?;
    }
    writeln!(
        output,
        "\nActive rows are persisted through the normalized SQLite graph and consumed by the existing bounded relation and analysis calls. Optional inferred rows remain unavailable until their independent quality, determinism, freshness, package, memory, and platform gates pass."
    )?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_relation_family_inventory_is_complete_and_frozen()
    -> Result<(), Box<dyn std::error::Error>> {
        let digest = validate_relation_family_inventory()?;
        if digest != ACCEPTED_RELATION_FAMILY_INVENTORY_V1_DIGEST {
            return Err("accepted relation-family digest changed".into());
        }
        Ok(())
    }

    #[test]
    fn generated_relation_support_document_is_current() -> Result<(), Box<dyn std::error::Error>> {
        if render_relation_support_markdown()? != include_str!("../../../docs/relation-support.md")
        {
            return Err("generated relation support document is stale".into());
        }
        Ok(())
    }
}
