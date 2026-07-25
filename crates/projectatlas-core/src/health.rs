//! Purpose: Detect structural health issues in `ProjectAtlas` indexes.

use crate::{IndexedNode, NodeKind, PurposeStatus, is_high_impact_file_path, normalized_parent};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt, str::FromStr};

/// Health category for paths without purpose metadata.
pub const CATEGORY_MISSING_PURPOSE: &str = "missing-purpose";
/// Health category for generated purpose suggestions awaiting review.
pub const CATEGORY_SUGGESTED_PURPOSE_REVIEW: &str = "suggested-purpose-review";
/// Health category for legacy or explicitly flagged accepted-purpose records.
pub const CATEGORY_STALE_PURPOSE: &str = "stale-purpose";
/// Health category for approved purposes that still need agent review.
pub const CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED: &str = "purpose-agent-review-required";
/// Health category for duplicated purpose text.
pub const CATEGORY_DUPLICATE_PURPOSE: &str = "duplicate-purpose";
/// Health category for repeated temporary/generated-output folders.
pub const CATEGORY_REPEATED_TEMPORARY_FOLDER: &str = "repeated-temporary-folder";
/// Structural categories that are not simple purpose lifecycle states.
pub const STRUCTURAL_HEALTH_CATEGORIES: [&str; 3] = [
    CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
    CATEGORY_DUPLICATE_PURPOSE,
    CATEGORY_REPEATED_TEMPORARY_FOLDER,
];
/// Folder names treated as repeated temporary/generated-output buckets.
pub const TEMP_FOLDER_BUCKETS: [&str; 6] = ["tmp", "temp", "cache", "generated", "out", "output"];

/// Finding message for missing-purpose rows.
pub const MESSAGE_MISSING_PURPOSE: &str = "Path is indexed but has no approved purpose.";
/// Finding recommendation for missing-purpose rows.
pub const RECOMMENDATION_MISSING_PURPOSE: &str =
    "Set or approve a one-line purpose in the ProjectAtlas index.";
/// Queue recommendation for missing-purpose rows.
pub const RECOMMENDATION_MISSING_PURPOSE_QUEUE: &str =
    "Set an agent-reviewed one-line purpose in the ProjectAtlas index.";
/// Finding message for suggested-purpose-review rows.
pub const MESSAGE_SUGGESTED_PURPOSE_REVIEW: &str =
    "Path has a generated purpose suggestion but no agent-approved purpose.";
/// Finding recommendation for suggested-purpose-review rows.
pub const RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW: &str =
    "Inspect the folder/file summary and approve or correct the purpose in SQLite.";
/// Queue recommendation for suggested-purpose-review rows.
pub const RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW_QUEUE: &str =
    "Inspect enough context and approve or correct the purpose in SQLite.";
/// Finding message for stale-purpose rows.
pub const MESSAGE_STALE_PURPOSE: &str =
    "Accepted purpose is in a legacy or explicitly flagged review state.";
/// Finding recommendation for stale-purpose rows.
pub const RECOMMENDATION_STALE_PURPOSE: &str =
    "Explicitly approve the existing responsibility or correct it if inconsistent.";
/// Finding message for purpose-agent-review-required rows.
pub const MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED: &str =
    "Purpose is approved but has not been reviewed by an agent.";
/// Finding recommendation for purpose-agent-review-required rows.
pub const RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED: &str =
    "Inspect current context and approve or correct the purpose with purpose set.";
/// Finding recommendation for duplicate-purpose rows.
pub const RECOMMENDATION_DUPLICATE_PURPOSE: &str =
    "Review whether these paths duplicate responsibility or need clearer purposes.";
/// Finding recommendation for repeated-temporary-folder rows.
pub const RECOMMENDATION_REPEATED_TEMPORARY_FOLDER: &str =
    "Consolidate temporary/generated output roots or add an allowlist rationale.";

/// Health finding severity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational finding.
    Info,
    /// Warning finding.
    Warning,
    /// Error finding.
    Error,
}

impl Severity {
    /// Return the stable lowercase database and payload value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Error returned when a health severity string is unknown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseSeverityError;

impl fmt::Display for ParseSeverityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid health severity")
    }
}

impl std::error::Error for ParseSeverityError {}

impl FromStr for Severity {
    type Err = ParseSeverityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            value if value == Self::Info.as_str() => Ok(Self::Info),
            value if value == Self::Warning.as_str() => Ok(Self::Warning),
            value if value == Self::Error.as_str() => Ok(Self::Error),
            _ => Err(ParseSeverityError),
        }
    }
}

/// Health finding emitted by `ProjectAtlas`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthFinding {
    /// Stable finding id derived from category and affected paths.
    pub id: String,
    /// Finding severity.
    pub severity: Severity,
    /// Finding category.
    pub category: String,
    /// Primary path.
    pub path: String,
    /// Related path when applicable.
    pub related_path: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Recommended cleanup or review action.
    pub recommendation: String,
}

/// Run initial structural health checks.
#[must_use]
pub fn health_check(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    let mut findings = Vec::new();
    findings.extend(missing_purpose_findings(nodes));
    findings.extend(suggested_purpose_findings(nodes));
    findings.extend(stale_purpose_findings(nodes));
    findings.extend(agent_review_required_findings(nodes));
    findings.extend(duplicate_purpose_findings(nodes));
    findings.extend(temp_folder_findings(nodes));
    findings
}

/// Return findings that have not been marked resolved.
#[must_use]
pub fn unresolved_health_findings(
    findings: Vec<HealthFinding>,
    resolved_ids: &[String],
) -> Vec<HealthFinding> {
    findings
        .into_iter()
        .filter(|finding| !resolved_ids.iter().any(|id| id == &finding.id))
        .collect()
}

/// Build a stable finding id from category and affected paths.
///
/// The database layer uses the same id contract when it builds health
/// findings through SQL instead of materializing the complete node list.
#[must_use]
pub fn finding_id(category: &str, path: &str, related_path: Option<&str>) -> String {
    let related_path = related_path.unwrap_or("");
    format!("{category}:{path}:{related_path}")
}

/// Find indexed paths without purpose metadata.
fn missing_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Missing)
        .map(|node| HealthFinding {
            id: finding_id(CATEGORY_MISSING_PURPOSE, &node.node.path, None),
            severity: Severity::Warning,
            category: CATEGORY_MISSING_PURPOSE.to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: MESSAGE_MISSING_PURPOSE.to_string(),
            recommendation: RECOMMENDATION_MISSING_PURPOSE.to_string(),
        })
        .collect()
}

/// Find indexed paths with generated purpose suggestions that need agent review.
fn suggested_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Suggested)
        .map(|node| HealthFinding {
            id: finding_id(CATEGORY_SUGGESTED_PURPOSE_REVIEW, &node.node.path, None),
            severity: Severity::Warning,
            category: CATEGORY_SUGGESTED_PURPOSE_REVIEW.to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: MESSAGE_SUGGESTED_PURPOSE_REVIEW.to_string(),
            recommendation: RECOMMENDATION_SUGGESTED_PURPOSE_REVIEW.to_string(),
        })
        .collect()
}

/// Find legacy or explicitly flagged accepted purposes awaiting explicit review.
fn stale_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Stale)
        .map(|node| HealthFinding {
            id: finding_id(CATEGORY_STALE_PURPOSE, &node.node.path, None),
            severity: Severity::Warning,
            category: CATEGORY_STALE_PURPOSE.to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: MESSAGE_STALE_PURPOSE.to_string(),
            recommendation: RECOMMENDATION_STALE_PURPOSE.to_string(),
        })
        .collect()
}

/// Find navigation-critical approved purposes that still need agent review.
fn agent_review_required_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Approved)
        .filter(|node| !node.purpose.agent_reviewed())
        .filter(|node| {
            node.node.kind == NodeKind::Folder
                || (node.node.kind == NodeKind::File && is_high_impact_file_path(&node.node.path))
        })
        .map(|node| HealthFinding {
            id: finding_id(
                CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
                &node.node.path,
                None,
            ),
            severity: Severity::Warning,
            category: CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: MESSAGE_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
            recommendation: RECOMMENDATION_PURPOSE_AGENT_REVIEW_REQUIRED.to_string(),
        })
        .collect()
}

/// Find paths that share the same purpose text.
fn duplicate_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    let mut by_purpose: HashMap<(NodeKind, String, String), Vec<&IndexedNode>> = HashMap::new();
    for node in nodes {
        if node.purpose.status != PurposeStatus::Approved {
            continue;
        }
        let Some(purpose) = &node.purpose.purpose else {
            continue;
        };
        by_purpose
            .entry((
                node.node.kind,
                purpose.to_lowercase(),
                duplicate_context_key(node),
            ))
            .or_default()
            .push(node);
    }
    let mut findings = Vec::new();
    for ((kind, _, _), matches) in by_purpose {
        if matches.len() < 2 {
            continue;
        }
        let first = matches[0];
        for duplicate in matches.iter().skip(1) {
            findings.push(HealthFinding {
                id: finding_id(
                    CATEGORY_DUPLICATE_PURPOSE,
                    &duplicate.node.path,
                    Some(&first.node.path),
                ),
                severity: Severity::Warning,
                category: CATEGORY_DUPLICATE_PURPOSE.to_string(),
                path: duplicate.node.path.clone(),
                related_path: Some(first.node.path.clone()),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation: RECOMMENDATION_DUPLICATE_PURPOSE.to_string(),
            });
        }
    }
    findings
}

/// Return the duplicate-purpose comparison context for a node.
fn duplicate_context_key(node: &IndexedNode) -> String {
    if node.node.kind == NodeKind::Folder {
        normalized_parent(&node.node.path).unwrap_or_default()
    } else {
        String::new()
    }
}

/// Find repeated temporary or generated-output folders.
fn temp_folder_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    let mut buckets: HashMap<&str, Vec<&IndexedNode>> = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.node.kind == NodeKind::Folder)
    {
        let Some(name) = node.node.path.rsplit('/').next() else {
            continue;
        };
        let normalized = name.to_lowercase();
        if let Some(bucket) = TEMP_FOLDER_BUCKETS
            .iter()
            .find(|candidate| **candidate == normalized)
        {
            buckets.entry(bucket).or_default().push(node);
        }
    }
    let mut findings = Vec::new();
    for (bucket, matches) in buckets {
        if matches.len() < 2 {
            continue;
        }
        let first = matches[0];
        for duplicate in matches.iter().skip(1) {
            findings.push(HealthFinding {
                id: finding_id(
                    CATEGORY_REPEATED_TEMPORARY_FOLDER,
                    &duplicate.node.path,
                    Some(&first.node.path),
                ),
                severity: Severity::Warning,
                category: CATEGORY_REPEATED_TEMPORARY_FOLDER.to_string(),
                path: duplicate.node.path.clone(),
                related_path: Some(first.node.path.clone()),
                message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                recommendation: RECOMMENDATION_REPEATED_TEMPORARY_FOLDER.to_string(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IndexedNode, Node, Purpose, PurposeSource};
    use std::error::Error;

    #[test]
    fn suggested_purpose_requires_review_and_is_not_duplicate_signal() -> Result<(), Box<dyn Error>>
    {
        let nodes = vec![
            test_node(
                "src/a.rs",
                NodeKind::File,
                Some("Generated file purpose"),
                PurposeStatus::Suggested,
            ),
            test_node(
                "src/b.rs",
                NodeKind::File,
                Some("Generated file purpose"),
                PurposeStatus::Suggested,
            ),
        ];

        let findings = health_check(&nodes);
        require_category(&findings, CATEGORY_SUGGESTED_PURPOSE_REVIEW)?;
        reject_category(&findings, CATEGORY_DUPLICATE_PURPOSE)?;
        Ok(())
    }

    #[test]
    fn approved_navigation_purposes_require_agent_review() -> Result<(), Box<dyn Error>> {
        let nodes = vec![
            test_node_with_source(
                ".",
                NodeKind::Folder,
                Some("Imported repository root"),
                PurposeStatus::Approved,
                PurposeSource::Imported,
            ),
            test_node_with_source(
                "Cargo.toml",
                NodeKind::File,
                Some("Imported Rust manifest"),
                PurposeStatus::Approved,
                PurposeSource::Imported,
            ),
            test_node_with_source(
                "src/detail.rs",
                NodeKind::File,
                Some("Imported implementation detail"),
                PurposeStatus::Approved,
                PurposeSource::Imported,
            ),
        ];

        let findings = health_check(&nodes);
        require_category(&findings, CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED)?;
        require_path(&findings, CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED, ".")?;
        require_path(
            &findings,
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            "Cargo.toml",
        )?;
        reject_path(
            &findings,
            CATEGORY_PURPOSE_AGENT_REVIEW_REQUIRED,
            "src/detail.rs",
        )?;
        Ok(())
    }

    #[test]
    fn duplicate_purpose_uses_approved_purposes_only() -> Result<(), Box<dyn Error>> {
        let nodes = vec![
            test_node(
                "src/a.rs",
                NodeKind::File,
                Some("Approved duplicate purpose"),
                PurposeStatus::Approved,
            ),
            test_node(
                "src/b.rs",
                NodeKind::File,
                Some("Approved duplicate purpose"),
                PurposeStatus::Approved,
            ),
        ];

        let findings = health_check(&nodes);
        require_category(&findings, CATEGORY_DUPLICATE_PURPOSE)?;
        reject_category(&findings, CATEGORY_SUGGESTED_PURPOSE_REVIEW)?;
        Ok(())
    }

    #[test]
    fn duplicate_folder_purpose_is_scoped_by_parent_context() -> Result<(), Box<dyn Error>> {
        let nodes = vec![
            test_node(
                "customers/service",
                NodeKind::Folder,
                Some("Service layer"),
                PurposeStatus::Approved,
            ),
            test_node(
                "settings/service",
                NodeKind::Folder,
                Some("Service layer"),
                PurposeStatus::Approved,
            ),
        ];

        let findings = health_check(&nodes);
        reject_category(&findings, CATEGORY_DUPLICATE_PURPOSE)?;
        Ok(())
    }

    /// Build a health-check test node.
    fn test_node(
        path: &str,
        kind: NodeKind,
        purpose: Option<&str>,
        status: PurposeStatus,
    ) -> IndexedNode {
        let source = if status == PurposeStatus::Suggested {
            PurposeSource::Generated
        } else {
            PurposeSource::Agent
        };
        test_node_with_source(path, kind, purpose, status, source)
    }

    /// Build a health-check test node with an explicit purpose source.
    fn test_node_with_source(
        path: &str,
        kind: NodeKind,
        purpose: Option<&str>,
        status: PurposeStatus,
        source: PurposeSource,
    ) -> IndexedNode {
        IndexedNode {
            node: Node {
                path: path.to_string(),
                kind,
                parent_path: normalized_parent(path),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(10),
                mtime_ns: Some(0),
                content_hash: Some("hash".to_string()),
            },
            purpose: Purpose {
                path: path.to_string(),
                purpose: purpose.map(str::to_string),
                source,
                status,
            },
            summary: Some("rust source summary".to_string()),
        }
    }

    /// Require a health finding category to be present.
    fn require_category(findings: &[HealthFinding], category: &str) -> Result<(), Box<dyn Error>> {
        if findings.iter().any(|finding| finding.category == category) {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("missing category {category}")).into())
        }
    }

    /// Require a health finding category to be absent.
    fn reject_category(findings: &[HealthFinding], category: &str) -> Result<(), Box<dyn Error>> {
        if findings.iter().any(|finding| finding.category == category) {
            Err(std::io::Error::other(format!("unexpected category {category}")).into())
        } else {
            Ok(())
        }
    }

    /// Require a health finding category/path pair to be present.
    fn require_path(
        findings: &[HealthFinding],
        category: &str,
        path: &str,
    ) -> Result<(), Box<dyn Error>> {
        if findings
            .iter()
            .any(|finding| finding.category == category && finding.path == path)
        {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("missing category {category} path {path}")).into())
        }
    }

    /// Require a health finding category/path pair to be absent.
    fn reject_path(
        findings: &[HealthFinding],
        category: &str,
        path: &str,
    ) -> Result<(), Box<dyn Error>> {
        if findings
            .iter()
            .any(|finding| finding.category == category && finding.path == path)
        {
            Err(std::io::Error::other(format!("unexpected category {category} path {path}")).into())
        } else {
            Ok(())
        }
    }
}
