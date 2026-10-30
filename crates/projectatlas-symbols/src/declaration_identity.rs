//! Purpose: Share canonical declaration scope identities with graph consumers.

use projectatlas_core::graph::{
    GraphContractError, GraphIdentityText, MAX_GRAPH_IDENTITY_BYTES, QUALIFIED_SYMBOL_SCOPE_PREFIX,
};
use projectatlas_core::symbols::{SymbolGraph, SymbolKind};
use std::collections::BTreeMap;

/// Qualify graph-only parent identity from the parser's existing containment rows.
///
/// Sorting and active-scope lookup are `O(n log n)`. Identity work is
/// `O(n * MAX_GRAPH_IDENTITY_BYTES)`: every retained parent is bounded, and an
/// overflowing candidate is hashed without materializing more than one bounded
/// parent plus one admitted component.
///
/// # Errors
/// Returns the graph contract error for an invalid or reserved source identity.
pub fn qualified_symbol_parents(
    graph: &SymbolGraph,
) -> Result<Vec<Option<GraphIdentityText>>, GraphContractError> {
    qualified_symbol_parents_selected(graph, (0..graph.symbols.len()).collect())
}

/// Derive only admitted raw ordinals, without retaining rejected scope ancestors.
pub(crate) fn qualified_symbol_parents_selected(
    graph: &SymbolGraph,
    mut order: Vec<usize>,
) -> Result<Vec<Option<GraphIdentityText>>, GraphContractError> {
    let is_php = graph
        .language
        .as_deref()
        .is_some_and(|language| language.eq_ignore_ascii_case("php"));
    order.sort_by_key(|&index| (graph.symbols[index].line_start, index));
    let mut active_by_name = BTreeMap::<&str, Vec<usize>>::new();
    let mut qualified_names = vec![None::<GraphIdentityText>; graph.symbols.len()];
    let mut parents = vec![None; graph.symbols.len()];
    for index in order {
        let symbol = &graph.symbols[index];
        let name = source_symbol_identity(symbol.name.clone())?;
        let mut parent = symbol
            .parent
            .clone()
            .map(source_symbol_identity)
            .transpose()?;
        if let Some(immediate_parent) = parent.as_ref()
            && let Some(candidates) = active_by_name.get_mut(immediate_parent.as_str())
        {
            while candidates.last().is_some_and(|&candidate_index| {
                let candidate = &graph.symbols[candidate_index];
                // Namespace headers may precede their body; concrete PHP scopes
                // have exact ends even when the next declaration shares a line.
                if is_php
                    && candidate.kind != SymbolKind::Module
                    && let Some((outer, inner)) =
                        candidate.source_selector.zip(symbol.source_selector)
                {
                    outer.byte_end < inner.byte_end
                } else {
                    candidate.line_end < symbol.line_end
                }
            }) {
                candidates.pop();
            }
            if let Some(qualified_parent) = candidates
                .last()
                .and_then(|&candidate_index| qualified_names[candidate_index].clone())
            {
                parent = Some(qualified_parent);
            }
        }
        qualified_names[index] = Some(match parent.as_ref() {
            Some(parent) => qualified_symbol_identity(parent, &name)?,
            None => name,
        });
        parents[index] = parent;
        active_by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(index);
    }
    Ok(parents)
}

/// Validate borrowed parser identity text using the publication admission policy.
///
/// # Errors
/// Returns the graph contract error for invalid text or the reserved scope prefix.
pub fn validate_source_symbol_identity(value: &str) -> Result<(), GraphContractError> {
    GraphIdentityText::validate(value)?;
    validate_source_scope_namespace(value)
}

/// Reject the private derived-scope namespace without changing source spelling.
fn validate_source_scope_namespace(value: &str) -> Result<(), GraphContractError> {
    if value.starts_with(QUALIFIED_SYMBOL_SCOPE_PREFIX) {
        return Err(GraphContractError::InvalidIdentityText {
            reason: "source symbol identity uses the reserved derived-scope namespace",
        });
    }
    Ok(())
}

/// Admit one parser-owned symbol identity outside the derived-scope namespace.
///
/// # Errors
/// Returns the graph contract error for invalid text or the reserved scope prefix.
pub fn source_symbol_identity(value: String) -> Result<GraphIdentityText, GraphContractError> {
    let identity = GraphIdentityText::new(value)?;
    validate_source_scope_namespace(identity.as_str())?;
    Ok(identity)
}

/// Derive one exact or compact qualified scope from validated components.
///
/// # Errors
/// Returns the graph contract error when the compact identity cannot retain its name.
pub fn qualified_symbol_identity(
    parent: &GraphIdentityText,
    name: &GraphIdentityText,
) -> Result<GraphIdentityText, GraphContractError> {
    const SEPARATOR: &str = "::";
    const DIGEST_DOMAIN: &str = "projectatlas.graph.qualified-symbol-scope.v1";
    let qualified_len = parent.as_str().len() + SEPARATOR.len() + name.as_str().len();
    if qualified_len <= MAX_GRAPH_IDENTITY_BYTES {
        let mut qualified = String::with_capacity(qualified_len);
        qualified.push_str(parent.as_str());
        qualified.push_str(SEPARATOR);
        qualified.push_str(name.as_str());
        return GraphIdentityText::new(qualified);
    }

    let compact_len =
        QUALIFIED_SYMBOL_SCOPE_PREFIX.len() + 64 + SEPARATOR.len() + name.as_str().len();
    if compact_len > MAX_GRAPH_IDENTITY_BYTES {
        return Err(GraphContractError::InvalidIdentityText {
            reason: "derived scope cannot retain its nearest admitted symbol name",
        });
    }
    let mut hasher = blake3::Hasher::new_derive_key(DIGEST_DOMAIN);
    hasher.update(parent.as_str().as_bytes());
    hasher.update(SEPARATOR.as_bytes());
    hasher.update(name.as_str().as_bytes());
    let digest = hasher.finalize().to_hex();
    let mut compact = String::with_capacity(compact_len);
    compact.push_str(QUALIFIED_SYMBOL_SCOPE_PREFIX);
    compact.push_str(digest.as_str());
    compact.push_str(SEPARATOR);
    compact.push_str(name.as_str());
    GraphIdentityText::new(compact)
}
