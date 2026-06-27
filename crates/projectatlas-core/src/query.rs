//! Purpose: Rank `ProjectAtlas` index nodes for progressive context queries.

use crate::{IndexedNode, NodeKind};

/// Rank nodes by simple path and purpose term matching.
#[must_use]
pub fn rank_nodes(nodes: &[IndexedNode], query: &str, kind: Option<NodeKind>) -> Vec<IndexedNode> {
    let terms = normalize_terms(query);
    let mut scored = Vec::new();
    for node in nodes {
        if let Some(required_kind) = kind
            && node.node.kind != required_kind
        {
            continue;
        }
        let score = score_node(node, &terms);
        if score > 0 || terms.is_empty() {
            scored.push((score, node.clone()));
        }
    }
    scored.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.node.path.cmp(&right.node.path))
    });
    scored.into_iter().map(|(_, node)| node).collect()
}

/// Split a query into lowercase search terms.
fn normalize_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Score a node against normalized query terms.
fn score_node(node: &IndexedNode, terms: &[String]) -> usize {
    if terms.is_empty() {
        return 1;
    }
    let mut haystack = node.node.path.to_lowercase();
    if let Some(purpose) = &node.purpose.purpose {
        haystack.push(' ');
        haystack.push_str(&purpose.to_lowercase());
    }
    if let Some(summary) = &node.summary {
        haystack.push(' ');
        haystack.push_str(&summary.to_lowercase());
    }
    terms
        .iter()
        .map(|term| {
            if haystack.contains(term) {
                10
            } else if fuzzy_contains(&haystack, term) {
                2
            } else {
                0
            }
        })
        .sum()
}

/// Check whether all needle characters appear in order.
fn fuzzy_contains(haystack: &str, needle: &str) -> bool {
    let mut chars = needle.chars();
    let Some(mut current) = chars.next() else {
        return true;
    };
    for character in haystack.chars() {
        if character == current {
            match chars.next() {
                Some(next) => current = next,
                None => return true,
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Node, Purpose, PurposeSource, PurposeStatus};

    #[test]
    /// Ranking should find semantic purpose matches.
    fn rank_prefers_purpose_match() {
        let nodes = vec![IndexedNode {
            node: Node {
                path: "src/auth/login.rs".to_string(),
                kind: NodeKind::File,
                parent_path: Some("src/auth".to_string()),
                extension: Some(".rs".to_string()),
                language: Some("rust".to_string()),
                size_bytes: Some(10),
                mtime_ns: Some(1),
                content_hash: Some("hash".to_string()),
            },
            purpose: Purpose {
                path: "src/auth/login.rs".to_string(),
                purpose: Some("Handle user authentication".to_string()),
                source: PurposeSource::Generated,
                status: PurposeStatus::Suggested,
            },
            summary: Some("Defines login flow entrypoint".to_string()),
        }];

        let ranked = rank_nodes(&nodes, "authentication", Some(NodeKind::File));
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].node.path, "src/auth/login.rs");
    }
}
