//! Gradle DSL symbol graph augmentation.

use projectatlas_core::symbols::{SymbolGraph, SymbolKind};
use regex::Regex;

use crate::push_symbol;

/// Add Gradle Kotlin DSL task registrations from `.gradle.kts` files.
pub(super) fn augment_kotlin(graph: &mut SymbolGraph, content: &str) {
    if !path_has_ascii_suffix(&graph.path, ".gradle.kts") {
        return;
    }
    if let Some(patterns) = GradleTaskPatterns::kotlin() {
        augment_tasks(graph, content, &patterns, "gradle-kotlin-dsl-task");
    }
}

/// Add Gradle Groovy DSL task registrations from `.gradle` files.
pub(super) fn augment_groovy(graph: &mut SymbolGraph, content: &str) {
    if !path_has_ascii_suffix(&graph.path, ".gradle") {
        return;
    }
    if let Some(patterns) = GradleTaskPatterns::groovy() {
        augment_tasks(graph, content, &patterns, "gradle-groovy-dsl-task");
    }
}

/// Regexes for one Gradle DSL task declaration style.
struct GradleTaskPatterns {
    /// Task declaration calls that are valid outside a `tasks {}` block.
    top_level_call_regexes: Vec<Regex>,
    /// `register`, `create`, or `named` call inside `tasks {}`.
    block_call_regex: Regex,
    /// Start of a multiline top-level task call.
    pending_regex: Regex,
    /// Start of a multiline task call inside `tasks {}`.
    block_pending_regex: Regex,
    /// String first argument on the following line of a multiline task call.
    string_arg_regex: Regex,
    /// Start of a `tasks {}` block.
    tasks_block_regex: Regex,
}

impl GradleTaskPatterns {
    /// Build regexes for Gradle Kotlin DSL scripts.
    fn kotlin() -> Option<Self> {
        Some(Self {
            top_level_call_regexes: vec![
                Regex::new(
                    r#"\btasks\.(?:register|create|named)\s*(?:<[^>]+>)?\s*\(\s*["']([^"']+)["']"#,
                )
                .ok()?,
                Regex::new(r#"\btask\s*(?:<[^>]+>)?\s*\(\s*["']([^"']+)["']"#).ok()?,
                Regex::new(
                    r"^\s*(?:val|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s+by\s+tasks\.(?:registering|creating|existing)",
                )
                .ok()?,
            ],
            block_call_regex: Regex::new(
                r#"\b(?:register|create|named)\s*(?:<[^>]+>)?\s*\(\s*["']([^"']+)["']"#,
            )
            .ok()?,
            pending_regex: Regex::new(
                r"\b(?:tasks\.(?:register|create|named)|task)\s*(?:<[^>]+>)?\s*\(\s*$",
            )
            .ok()?,
            block_pending_regex: Regex::new(
                r"\b(?:register|create|named)\s*(?:<[^>]+>)?\s*\(\s*$",
            )
            .ok()?,
            string_arg_regex: Regex::new(r#"^\s*["']([^"']+)["']"#).ok()?,
            tasks_block_regex: Regex::new(r"^\s*tasks\s*\{").ok()?,
        })
    }

    /// Build regexes for Gradle Groovy DSL scripts.
    fn groovy() -> Option<Self> {
        Some(Self {
            top_level_call_regexes: vec![
                Regex::new(
                    r#"\btasks\.(?:register|create|named)\s*\(\s*(?:name\s*:\s*)?["']([^"']+)["']"#,
                )
                .ok()?,
                Regex::new(r#"\btask\s*\(\s*(?:name\s*:\s*)?["']([^"']+)["']"#).ok()?,
                Regex::new(r"^task\s+([A-Za-z_][A-Za-z0-9_-]*)\b").ok()?,
            ],
            block_call_regex: Regex::new(
                r#"\b(?:register|create|named)\s*\(\s*(?:name\s*:\s*)?["']([^"']+)["']"#,
            )
            .ok()?,
            pending_regex: Regex::new(r"\b(?:tasks\.(?:register|create|named)|task)\s*\(\s*$")
                .ok()?,
            block_pending_regex: Regex::new(r"\b(?:register|create|named)\s*\(\s*$").ok()?,
            string_arg_regex: Regex::new(r#"^\s*(?:name\s*:\s*)?["']([^"']+)["']"#).ok()?,
            tasks_block_regex: Regex::new(r"^\s*tasks\s*\{").ok()?,
        })
    }
}

/// Add task symbols from one Gradle DSL source.
fn augment_tasks(
    graph: &mut SymbolGraph,
    content: &str,
    patterns: &GradleTaskPatterns,
    detail: &str,
) {
    let mut in_tasks_block = false;
    let mut tasks_block_depth = 0_i32;
    let mut pending_gradle_task_line: Option<usize> = None;
    for (line_index, line) in content.lines().enumerate() {
        let line_number = line_index + 1;
        let trimmed = line.trim();
        let tasks_block_starts = patterns.tasks_block_regex.is_match(trimmed);
        let in_tasks_context = in_tasks_block || tasks_block_starts;
        if let Some(name) = capture_first(trimmed, &patterns.top_level_call_regexes) {
            push_gradle_task_symbol(graph, name, line_number, line_number, detail, trimmed);
            pending_gradle_task_line = None;
        } else if in_tasks_context
            && let Some(name) = patterns
                .block_call_regex
                .captures(trimmed)
                .and_then(|capture| capture.get(1))
                .map(|value| value.as_str())
        {
            push_gradle_task_symbol(graph, name, line_number, line_number, detail, trimmed);
            pending_gradle_task_line = None;
        } else if let Some(start_line) = pending_gradle_task_line {
            if let Some(capture) = patterns.string_arg_regex.captures(trimmed)
                && let Some(name) = capture.get(1).map(|value| value.as_str())
            {
                push_gradle_task_symbol(graph, name, start_line, line_number, detail, trimmed);
                pending_gradle_task_line = None;
            } else if trimmed.contains(')') {
                pending_gradle_task_line = None;
            }
        } else if patterns.pending_regex.is_match(trimmed)
            || (in_tasks_context && patterns.block_pending_regex.is_match(trimmed))
        {
            pending_gradle_task_line = Some(line_number);
        }
        if tasks_block_starts && !in_tasks_block {
            in_tasks_block = true;
            tasks_block_depth = 0;
        }
        if in_tasks_block {
            tasks_block_depth += trimmed.matches('{').count() as i32;
            tasks_block_depth -= trimmed.matches('}').count() as i32;
            if tasks_block_depth <= 0 {
                in_tasks_block = false;
                tasks_block_depth = 0;
            }
        }
    }
}

/// Return the first captured identifier from a set of regexes.
fn capture_first<'a>(line: &'a str, patterns: &[Regex]) -> Option<&'a str> {
    patterns.iter().find_map(|regex| {
        regex
            .captures(line)
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str())
    })
}

/// Emit a Gradle DSL task as a primary symbol for ranking and summaries.
fn push_gradle_task_symbol(
    graph: &mut SymbolGraph,
    name: &str,
    line_start: usize,
    line_end: usize,
    detail: &str,
    signature: &str,
) {
    if graph
        .symbols
        .iter()
        .any(|symbol| symbol.name == name && symbol.detail.as_deref() == Some(detail))
    {
        return;
    }
    push_symbol(
        graph,
        name,
        SymbolKind::Function,
        line_start,
        line_end,
        None,
        Some(detail),
        signature,
    );
}

/// Case-insensitive ASCII suffix check for repository paths.
fn path_has_ascii_suffix(path: &str, suffix: &str) -> bool {
    let path = path.as_bytes();
    let suffix = suffix.as_bytes();
    path.len() >= suffix.len() && path[path.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}
