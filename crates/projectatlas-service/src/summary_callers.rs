//! Display caller lists from bounded canonical incoming call adjacency.

use std::collections::HashMap;
use std::path::Path;

use projectatlas_core::graph::{
    EntitySelector, GraphLimits, GraphRelationKind, RepositoryNodePath,
};
use projectatlas_core::symbols::{CodeSymbol, RelationKind};
use projectatlas_db::{AtlasStore, RepositoryGraphDirection};

use crate::{CALLERS_PER_SYMBOL_LIMIT, ServiceResult, symbol_summary_key};

/// Canonical callers plus an explicit omission signal for bounded/ambiguous correspondence.
pub(super) fn load_summary_callers(
    store: &AtlasStore,
    path: &str,
    symbols: &[CodeSymbol],
) -> ServiceResult<(HashMap<String, Vec<String>>, bool)> {
    crate::with_index_read_snapshot(store, || {
        load_summary_callers_in_snapshot(store, path, symbols)
    })
}

/// Keep metadata correspondence and incoming edges in one complete read generation.
fn load_summary_callers_in_snapshot(
    store: &AtlasStore,
    path: &str,
    symbols: &[CodeSymbol],
) -> ServiceResult<(HashMap<String, Vec<String>>, bool)> {
    let mut callers = HashMap::new();
    let Some(project) = store.project_instance_id()? else {
        return Ok((callers, false));
    };
    if store.repository_graph_generation()?.is_none() || symbols.is_empty() {
        return Ok((callers, false));
    }
    let path = RepositoryNodePath::new(Path::new(path)).map_err(projectatlas_db::DbError::from)?;
    let page = store.repository_graph_entities_by_path(project, &path, GraphLimits::MAX_ROWS)?;
    // Incomplete owning metadata cannot establish uniqueness.
    if page.truncated {
        return Ok((callers, true));
    }
    let mut candidates = HashMap::<_, Vec<_>>::new();
    for entity in &page.rows {
        if let EntitySelector::Symbol { symbol } = entity.selector() {
            candidates
                .entry((
                    symbol.name.as_str(),
                    symbol.kind.to_string(),
                    symbol.signature.as_str(),
                ))
                .or_default()
                .push(entity);
        }
    }
    let mut truncated = false;
    let mut by_entity = HashMap::<&str, Vec<String>>::new();
    // Bound correspondence work as well as returned rows on dense duplicate-name files.
    let mut remaining_candidates = (GraphLimits::MAX_ROWS as usize).saturating_mul(4);
    for symbol in symbols {
        let signature = if symbol.signature.trim().is_empty() {
            symbol.name.as_str()
        } else {
            symbol.signature.trim()
        };
        let Some(entities) =
            candidates.get(&(symbol.name.as_str(), symbol.kind.to_string(), signature))
        else {
            continue;
        };
        let mut matched = None;
        let mut ambiguous = false;
        for entity in entities {
            if remaining_candidates == 0 {
                truncated = true;
                ambiguous = true;
                break;
            }
            remaining_candidates -= 1;
            let EntitySelector::Symbol { symbol: selector } = entity.selector() else {
                continue;
            };
            let compatible_parent = match (symbol.parent.as_deref(), selector.parent.as_ref()) {
                (None, None) => true,
                (Some(raw), Some(parent)) => {
                    parent.as_str() == raw
                        || parent
                            .as_str()
                            .strip_suffix(raw)
                            .is_some_and(|prefix| prefix.ends_with("::"))
                }
                _ => false,
            };
            if compatible_parent {
                if matched.is_some() {
                    ambiguous = true;
                    truncated = true;
                    break;
                }
                matched = Some(*entity);
            }
        }
        if ambiguous {
            continue;
        }
        let Some(entity) = matched else {
            continue;
        };
        let references = if let Some(references) = by_entity.get(entity.key().digest()) {
            references.clone()
        } else {
            let adjacency = store.repository_graph_resolved_adjacency_page(
                std::slice::from_ref(entity.key()),
                RepositoryGraphDirection::Inbound,
                GraphRelationKind::Legacy(RelationKind::Calls),
                None,
                CALLERS_PER_SYMBOL_LIMIT as u32,
                None,
            )?;
            truncated |= adjacency.truncated;
            let mut references = adjacency
                .rows
                .into_iter()
                .filter_map(|row| match row.detail.source.selector() {
                    EntitySelector::Symbol { symbol } => Some(format!(
                        "{}::{}",
                        symbol.file.as_str(),
                        symbol.name.as_str()
                    )),
                    EntitySelector::File { path } => Some(format!(
                        "{}::{}",
                        path.as_str(),
                        projectatlas_core::symbols::MODULE_RELATION_SOURCE
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
            references.sort();
            references.dedup();
            by_entity.insert(entity.key().digest(), references.clone());
            references
        };
        callers.insert(symbol_summary_key(symbol), references);
    }
    Ok((callers, truncated))
}
