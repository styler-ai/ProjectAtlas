//! Import-alias caller resolution over persisted symbol relations.

use projectatlas_core::symbols::{CodeSymbol, RelationKind, SymbolRelation};
use projectatlas_db::AtlasStore;
use projectatlas_symbols::{ImportSyntax, parse_import_references, resolve_relative_import_path};
use projectatlas_symbols::{module_aliases_for_path, source_stems_for_path};
use std::collections::{HashMap, HashSet};

use crate::{ServiceResult, symbol_summary_key, symbol_target_aliases};

/// Import relations inspected per module term for alias-based caller lookup.
const IMPORT_RELATION_LIMIT_PER_TERM: usize = 500;
/// Relations inspected per caller file after a target import is found.
const IMPORT_RELATION_LIMIT_PER_CALLER: usize = 1_000;

/// Import-derived call target for one caller file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImportCallAlias {
    /// Caller path that owns the import alias.
    pub(crate) caller_path: String,
    /// Call target emitted by parser relations inside that caller.
    pub(crate) target_name: String,
}

/// Per-symbol import alias lookup keyed by `symbol_summary_key`.
pub(crate) type ImportAliasMap = HashMap<String, Vec<ImportCallAlias>>;

/// Build deterministic import-alias call targets for displayed symbols.
pub(crate) fn load_import_alias_map(
    store: &AtlasStore,
    symbols: &[CodeSymbol],
    alias_counts: &HashMap<String, usize>,
) -> ServiceResult<ImportAliasMap> {
    let import_relations = load_import_relations_for_symbols(store, symbols)?;
    Ok(import_alias_map(symbols, &import_relations, alias_counts))
}

/// Load persisted import relations likely to mention displayed symbols.
fn load_import_relations_for_symbols(
    store: &AtlasStore,
    symbols: &[CodeSymbol],
) -> ServiceResult<Vec<SymbolRelation>> {
    let mut terms = symbols
        .iter()
        .flat_map(|symbol| module_aliases_for_path(&symbol.path))
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    let mut relations =
        store.load_import_relations_matching_targets(&terms, IMPORT_RELATION_LIMIT_PER_TERM)?;
    let mut caller_paths = relations
        .iter()
        .map(|relation| relation.path.clone())
        .collect::<Vec<_>>();
    caller_paths.sort();
    caller_paths.dedup();
    for caller_path in caller_paths {
        relations.extend(
            store
                .load_symbol_relations(Some(&caller_path), None, IMPORT_RELATION_LIMIT_PER_CALLER)?
                .into_iter()
                .filter(|relation| relation.kind == RelationKind::Imports),
        );
    }
    relations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.source_name.cmp(&right.source_name))
            .then_with(|| left.target_name.cmp(&right.target_name))
    });
    relations.dedup_by(|left, right| {
        left.path == right.path
            && left.source_name == right.source_name
            && left.target_name == right.target_name
            && left.kind == right.kind
            && left.line == right.line
    });
    Ok(relations)
}

/// Build deterministic import-alias call targets from already loaded imports.
fn import_alias_map(
    symbols: &[CodeSymbol],
    import_relations: &[SymbolRelation],
    alias_counts: &HashMap<String, usize>,
) -> ImportAliasMap {
    let local_alias_counts = import_local_alias_counts(import_relations);
    let mut candidates: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for relation in import_relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::Imports)
    {
        for symbol in symbols {
            for target_name in import_call_targets_for_symbol(relation, symbol, alias_counts) {
                if import_local_alias_is_ambiguous(&local_alias_counts, relation, &target_name) {
                    continue;
                }
                candidates
                    .entry((relation.path.clone(), target_name))
                    .or_default()
                    .insert(symbol_summary_key(symbol));
            }
        }
    }
    let mut aliases: ImportAliasMap = HashMap::new();
    for ((caller_path, target_name), symbol_keys) in candidates {
        if symbol_keys.len() != 1 {
            continue;
        }
        let Some(symbol_key) = symbol_keys.into_iter().next() else {
            continue;
        };
        aliases
            .entry(symbol_key)
            .or_default()
            .push(ImportCallAlias {
                caller_path,
                target_name,
            });
    }
    for rows in aliases.values_mut() {
        rows.sort_by(|left, right| {
            left.caller_path
                .cmp(&right.caller_path)
                .then_with(|| left.target_name.cmp(&right.target_name))
        });
        rows.dedup();
    }
    aliases
}

/// Count local import aliases per caller file.
fn import_local_alias_counts(
    import_relations: &[SymbolRelation],
) -> HashMap<(String, String), usize> {
    let mut counts = HashMap::new();
    for relation in import_relations
        .iter()
        .filter(|relation| relation.kind == RelationKind::Imports)
    {
        for alias in local_aliases_from_import(&relation.target_name) {
            *counts.entry((relation.path.clone(), alias)).or_insert(0) += 1;
        }
    }
    counts
}

/// Return whether a resolved call target uses a duplicated local import alias.
fn import_local_alias_is_ambiguous(
    counts: &HashMap<(String, String), usize>,
    relation: &SymbolRelation,
    target_name: &str,
) -> bool {
    local_alias_candidates(target_name).iter().any(|alias| {
        counts
            .get(&(relation.path.clone(), alias.clone()))
            .copied()
            .unwrap_or(0)
            > 1
    })
}

/// Return all local alias fragments that a call target depends on.
fn local_alias_candidates(target_name: &str) -> Vec<String> {
    let mut aliases = vec![target_name.to_string()];
    if let Some((prefix, _rest)) = target_name.split_once("::") {
        aliases.push(prefix.to_string());
    }
    if let Some((prefix, _rest)) = target_name.split_once('.') {
        aliases.push(prefix.to_string());
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

/// Extract caller-local aliases declared by one import relation.
fn local_aliases_from_import(import_text: &str) -> Vec<String> {
    let mut aliases = parse_import_references(import_text)
        .into_iter()
        .map(|reference| reference.local().to_string())
        .collect::<Vec<_>>();
    aliases.sort();
    aliases.dedup();
    aliases
}

/// Return caller-local call targets that an import relation maps to a symbol.
fn import_call_targets_for_symbol(
    relation: &SymbolRelation,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    let import_text = relation.target_name.trim();
    if import_text.starts_with("use ") {
        rust_import_call_targets(import_text, symbol, alias_counts)
    } else if import_text.starts_with("import ") && import_text.contains(" from ") {
        typescript_import_call_targets(&relation.path, import_text, symbol, alias_counts)
    } else if import_text.starts_with("from ") || import_text.starts_with("import ") {
        python_import_call_targets(import_text, symbol, alias_counts)
    } else {
        Vec::new()
    }
}

/// Return Rust call targets introduced by simple `use` aliases.
fn rust_import_call_targets(
    import_text: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    parse_import_references(import_text)
        .into_iter()
        .filter(|reference| reference.syntax() == ImportSyntax::Rust)
        .filter_map(|reference| match reference.imported() {
            Some(imported)
                if imported == symbol.name
                    && module_matches_symbol(reference.module(), "::", symbol, alias_counts) =>
            {
                Some(reference.local().to_string())
            }
            Some(imported)
                if module_matches_symbol(
                    &format!("{}::{imported}", reference.module()),
                    "::",
                    symbol,
                    alias_counts,
                ) =>
            {
                Some(format!("{}::{}", reference.local(), symbol.name))
            }
            None if module_matches_symbol(reference.module(), "::", symbol, alias_counts) => {
                Some(format!("{}::{}", reference.local(), symbol.name))
            }
            _ => None,
        })
        .collect()
}

/// Return TypeScript/JavaScript call targets introduced by import aliases.
fn typescript_import_call_targets(
    caller_path: &str,
    import_text: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    parse_import_references(import_text)
        .into_iter()
        .filter(|reference| reference.syntax() == ImportSyntax::EcmaScript)
        .filter_map(|reference| {
            if !typescript_module_matches_symbol(
                caller_path,
                reference.module(),
                symbol,
                alias_counts,
            ) {
                return None;
            }
            match reference.imported() {
                Some(imported) if imported == symbol.name => Some(reference.local().to_string()),
                None => Some(format!("{}.{}", reference.local(), symbol.name)),
                Some(_) => None,
            }
        })
        .collect()
}

/// Return Python call targets introduced by import aliases.
fn python_import_call_targets(
    import_text: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    parse_import_references(import_text)
        .into_iter()
        .filter(|reference| reference.syntax() == ImportSyntax::Python)
        .filter_map(|reference| {
            if !python_module_matches_symbol(reference.module(), symbol, alias_counts) {
                return None;
            }
            match reference.imported() {
                Some(imported) if imported == symbol.name => Some(reference.local().to_string()),
                None => Some(format!("{}.{}", reference.local(), symbol.name)),
                Some(_) => None,
            }
        })
        .collect()
}

/// Return whether a Rust/Python module path can uniquely identify a symbol file.
fn module_matches_symbol(
    module_path: &str,
    separator: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> bool {
    let normalized = module_path
        .trim()
        .trim_start_matches("crate::")
        .trim_start_matches("self::")
        .trim_start_matches("super::")
        .trim_start_matches("crate.")
        .trim_start_matches("self.")
        .trim_start_matches("super.");
    module_aliases_for_path(&symbol.path).iter().any(|alias| {
        let alias_in_separator = if separator == "::" {
            alias.replace('.', "::")
        } else {
            alias.replace("::", ".")
        };
        alias_in_separator == normalized
            && module_symbol_alias_is_unique(symbol, &alias_in_separator, separator, alias_counts)
    })
}

/// Return whether a TypeScript module specifier can uniquely identify a symbol file.
fn typescript_module_matches_symbol(
    caller_path: &str,
    module_spec: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> bool {
    if let Some(relative_path) = resolve_relative_import_path(caller_path, module_spec) {
        return source_stems_for_path(&symbol.path)
            .iter()
            .any(|stem| stem == &relative_path)
            && module_aliases_for_path(&relative_path)
                .iter()
                .any(|module_alias| {
                    module_symbol_alias_is_unique(symbol, module_alias, ".", alias_counts)
                });
    }
    module_matches_symbol(module_spec, ".", symbol, alias_counts)
}

/// Return whether a Python module path can uniquely identify a symbol file.
fn python_module_matches_symbol(
    module_path: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> bool {
    module_matches_symbol(module_path, ".", symbol, alias_counts)
}

/// Return whether a module-qualified symbol alias is globally unique.
fn module_symbol_alias_is_unique(
    symbol: &CodeSymbol,
    module_alias: &str,
    separator: &str,
    alias_counts: &HashMap<String, usize>,
) -> bool {
    let candidate = format!("{module_alias}{separator}{}", symbol.name);
    symbol_target_aliases(symbol)
        .iter()
        .any(|alias| alias == &candidate && alias_counts.get(alias).copied().unwrap_or(0) <= 1)
}
