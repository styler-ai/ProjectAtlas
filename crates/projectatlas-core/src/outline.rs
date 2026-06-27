//! Purpose: Build compressed file outlines for agent context.

use serde::{Deserialize, Serialize};

/// Compact file outline returned before exact source is requested.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FileOutline {
    /// Repository-relative path.
    pub path: String,
    /// Optional language or file family.
    pub language: Option<String>,
    /// Total line count.
    pub line_count: usize,
    /// First non-empty lines capped for token efficiency.
    pub preview_lines: Vec<String>,
    /// Estimated tokens for the full file.
    pub estimated_full_tokens: usize,
    /// Estimated tokens for this outline.
    pub estimated_outline_tokens: usize,
}

/// Build a compact outline from file content.
#[must_use]
pub fn build_outline(
    path: &str,
    language: Option<String>,
    content: &str,
    limit: usize,
) -> FileOutline {
    let mut preview_lines = Vec::new();
    for line in content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        preview_lines.push(line.to_string());
        if preview_lines.len() >= limit {
            break;
        }
    }
    let estimated_full_tokens = estimate_tokens(content);
    let outline_payload = preview_lines.join("\n");
    let estimated_outline_tokens = estimate_tokens(&outline_payload);
    FileOutline {
        path: path.to_string(),
        language,
        line_count: content.lines().count(),
        preview_lines,
        estimated_full_tokens,
        estimated_outline_tokens,
    }
}

/// Estimate tokens from text using a stable lightweight heuristic.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 { 0 } else { chars.div_ceil(4) }
}
