//! Purpose: Detect structural health issues in `ProjectAtlas` indexes.

use crate::{IndexedNode, NodeKind, PurposeStatus, normalized_parent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
            id: finding_id("missing-purpose", &node.node.path, None),
            severity: Severity::Warning,
            category: "missing-purpose".to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: "Path is indexed but has no approved purpose.".to_string(),
            recommendation: "Set or approve a one-line purpose in the ProjectAtlas index."
                .to_string(),
        })
        .collect()
}

/// Find indexed paths with generated purpose suggestions that need agent review.
fn suggested_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Suggested)
        .map(|node| HealthFinding {
            id: finding_id("suggested-purpose-review", &node.node.path, None),
            severity: Severity::Warning,
            category: "suggested-purpose-review".to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: "Path has a generated purpose suggestion but no agent-approved purpose."
                .to_string(),
            recommendation:
                "Inspect the folder/file summary and approve or correct the purpose in SQLite."
                    .to_string(),
        })
        .collect()
}

/// Find approved purposes whose indexed content changed and needs review.
fn stale_purpose_findings(nodes: &[IndexedNode]) -> Vec<HealthFinding> {
    nodes
        .iter()
        .filter(|node| node.purpose.status == PurposeStatus::Stale)
        .map(|node| HealthFinding {
            id: finding_id("stale-purpose", &node.node.path, None),
            severity: Severity::Warning,
            category: "stale-purpose".to_string(),
            path: node.node.path.clone(),
            related_path: None,
            message: "Path changed after its purpose was approved.".to_string(),
            recommendation:
                "Inspect the current summary and approve or correct the one-line purpose."
                    .to_string(),
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
                    "duplicate-purpose",
                    &duplicate.node.path,
                    Some(&first.node.path),
                ),
                severity: Severity::Warning,
                category: "duplicate-purpose".to_string(),
                path: duplicate.node.path.clone(),
                related_path: Some(first.node.path.clone()),
                message: format!("Multiple {kind} nodes share the same purpose."),
                recommendation:
                    "Review whether these paths duplicate responsibility or need clearer purposes."
                        .to_string(),
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
    let suspicious = ["tmp", "temp", "cache", "generated", "out", "output"];
    let mut buckets: HashMap<&str, Vec<&IndexedNode>> = HashMap::new();
    for node in nodes
        .iter()
        .filter(|node| node.node.kind == NodeKind::Folder)
    {
        let Some(name) = node.node.path.rsplit('/').next() else {
            continue;
        };
        let normalized = name.to_lowercase();
        if let Some(bucket) = suspicious
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
                    "repeated-temporary-folder",
                    &duplicate.node.path,
                    Some(&first.node.path),
                ),
                severity: Severity::Warning,
                category: "repeated-temporary-folder".to_string(),
                path: duplicate.node.path.clone(),
                related_path: Some(first.node.path.clone()),
                message: format!("Repeated temporary/generated folder name `{bucket}` found."),
                recommendation:
                    "Consolidate temporary/generated output roots or add an allowlist rationale."
                        .to_string(),
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
        require_category(&findings, "suggested-purpose-review")?;
        reject_category(&findings, "duplicate-purpose")?;
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
        require_category(&findings, "duplicate-purpose")?;
        reject_category(&findings, "suggested-purpose-review")?;
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
        reject_category(&findings, "duplicate-purpose")?;
        Ok(())
    }

    /// Build a health-check test node.
    fn test_node(
        path: &str,
        kind: NodeKind,
        purpose: Option<&str>,
        status: PurposeStatus,
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
                source: if status == PurposeStatus::Suggested {
                    PurposeSource::Generated
                } else {
                    PurposeSource::Agent
                },
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
}
