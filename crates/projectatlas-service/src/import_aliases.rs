//! Import-alias caller resolution over persisted symbol relations.

use projectatlas_core::symbols::{CodeSymbol, RelationKind, SymbolRelation};
use projectatlas_db::AtlasStore;
use std::collections::{HashMap, HashSet};

use crate::{
    ServiceResult, module_aliases_for_path, source_stems_for_path, strip_known_source_extension,
    symbol_summary_key, symbol_target_aliases,
};

/// Import relations inspected per module term for alias-based caller lookup.
const IMPORT_RELATION_LIMIT_PER_TERM: usize = 500;

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
    Ok(store.load_import_relations_matching_targets(&terms, IMPORT_RELATION_LIMIT_PER_TERM)?)
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
    let import_text = import_text.trim();
    if import_text.starts_with("use ") {
        rust_local_import_aliases(import_text)
    } else if import_text.starts_with("import ") && import_text.contains(" from ") {
        typescript_local_import_aliases(import_text)
    } else if import_text.starts_with("from ") || import_text.starts_with("import ") {
        python_local_import_aliases(import_text)
    } else {
        Vec::new()
    }
}

/// Extract local aliases from simple Rust use statements.
fn rust_local_import_aliases(import_text: &str) -> Vec<String> {
    let Some(rest) = import_text.strip_prefix("use ") else {
        return Vec::new();
    };
    let rest = rest.trim().trim_end_matches(';').trim();
    if rest.contains('{') {
        return Vec::new();
    }
    let (import_path, local_alias) = split_alias(rest, " as ");
    let Some(last_segment) = import_path
        .rsplit("::")
        .next()
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    vec![local_alias.unwrap_or(last_segment).to_string()]
}

/// Extract local aliases from TypeScript or JavaScript import statements.
fn typescript_local_import_aliases(import_text: &str) -> Vec<String> {
    if let Some(namespace_alias) = namespace_import_alias(import_text) {
        return vec![namespace_alias];
    }
    braced_import_names(import_text).map_or_else(Vec::new, |names| {
        names
            .into_iter()
            .map(|(imported, alias)| alias.unwrap_or(imported))
            .collect()
    })
}

/// Extract local aliases from Python import statements.
fn python_local_import_aliases(import_text: &str) -> Vec<String> {
    if let Some(rest) = import_text.strip_prefix("from ")
        && let Some((_module, imports)) = rest.split_once(" import ")
    {
        return imports
            .split(',')
            .map(|item| {
                let (imported, alias) = split_alias(item.trim(), " as ");
                alias.unwrap_or_else(|| imported.trim()).to_string()
            })
            .collect();
    }
    let Some(rest) = import_text.strip_prefix("import ") else {
        return Vec::new();
    };
    rest.split(',')
        .map(|item| {
            let (module, alias) = split_alias(item.trim(), " as ");
            alias.unwrap_or(module.trim()).to_string()
        })
        .collect()
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
    let Some(rest) = import_text.strip_prefix("use ") else {
        return Vec::new();
    };
    let rest = rest.trim().trim_end_matches(';').trim();
    if rest.contains('{') {
        return Vec::new();
    }
    let (import_path, local_alias) = split_alias(rest, " as ");
    let import_path = import_path.trim();
    let Some(last_segment) = import_path
        .rsplit("::")
        .next()
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    if last_segment == symbol.name {
        let module_path = import_path
            .rsplit_once("::")
            .map_or("", |(module, _name)| module)
            .trim();
        if module_matches_symbol(module_path, "::", symbol, alias_counts) {
            return vec![local_alias.unwrap_or(last_segment).to_string()];
        }
    } else if module_matches_symbol(import_path, "::", symbol, alias_counts) {
        return vec![format!(
            "{}::{}",
            local_alias.unwrap_or(last_segment),
            symbol.name
        )];
    }
    Vec::new()
}

/// Return TypeScript/JavaScript call targets introduced by import aliases.
fn typescript_import_call_targets(
    caller_path: &str,
    import_text: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    let Some(module_spec) = quoted_module_spec_after_from(import_text) else {
        return Vec::new();
    };
    if !typescript_module_matches_symbol(caller_path, module_spec, symbol, alias_counts) {
        return Vec::new();
    }
    if let Some(namespace_alias) = namespace_import_alias(import_text) {
        return vec![format!("{namespace_alias}.{}", symbol.name)];
    }
    let Some(imported_names) = braced_import_names(import_text) else {
        return Vec::new();
    };
    imported_names
        .into_iter()
        .filter_map(|(imported, alias)| {
            (imported == symbol.name).then(|| alias.unwrap_or(imported))
        })
        .collect()
}

/// Return Python call targets introduced by import aliases.
fn python_import_call_targets(
    import_text: &str,
    symbol: &CodeSymbol,
    alias_counts: &HashMap<String, usize>,
) -> Vec<String> {
    if let Some(rest) = import_text.strip_prefix("from ")
        && let Some((module, imports)) = rest.split_once(" import ")
        && python_module_matches_symbol(module.trim(), symbol, alias_counts)
    {
        return imports
            .split(',')
            .filter_map(|item| {
                let (imported, alias) = split_alias(item.trim(), " as ");
                (imported.trim() == symbol.name)
                    .then(|| alias.unwrap_or_else(|| imported.trim()).to_string())
            })
            .collect();
    }
    let Some(rest) = import_text.strip_prefix("import ") else {
        return Vec::new();
    };
    rest.split(',')
        .filter_map(|item| {
            let (module, alias) = split_alias(item.trim(), " as ");
            let module = module.trim();
            if python_module_matches_symbol(module, symbol, alias_counts) {
                Some(format!("{}.{}", alias.unwrap_or(module), symbol.name))
            } else {
                None
            }
        })
        .collect()
}

/// Split `value` into imported path/name and optional local alias.
fn split_alias<'a>(value: &'a str, marker: &str) -> (&'a str, Option<&'a str>) {
    value
        .split_once(marker)
        .map_or((value, None), |(left, right)| {
            (left.trim(), Some(right.trim()))
        })
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
    if let Some(relative_path) = resolve_relative_module_path(caller_path, module_spec) {
        return source_stems_for_path(&symbol.path)
            .iter()
            .any(|stem| stem == &relative_path)
            && module_symbol_alias_is_unique(
                symbol,
                &module_name_from_path(&relative_path),
                ".",
                alias_counts,
            );
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

/// Extract the quoted module specifier after a TypeScript `from` clause.
fn quoted_module_spec_after_from(import_text: &str) -> Option<&str> {
    let (_left, right) = import_text.split_once(" from ")?;
    quoted_text(right.trim().trim_end_matches(';'))
}

/// Extract `import * as alias from ...` namespace alias text.
fn namespace_import_alias(import_text: &str) -> Option<String> {
    let rest = import_text.strip_prefix("import ")?.trim_start();
    let rest = rest.strip_prefix('*')?.trim_start();
    let rest = rest.strip_prefix("as ")?.trim_start();
    rest.split_whitespace()
        .next()
        .map(ToString::to_string)
        .filter(|alias| !alias.is_empty())
}

/// Extract braced TypeScript import names and aliases.
fn braced_import_names(import_text: &str) -> Option<Vec<(String, Option<String>)>> {
    let start = import_text.find('{')?;
    let end = import_text[start + 1..].find('}')? + start + 1;
    Some(
        import_text[start + 1..end]
            .split(',')
            .filter_map(|item| {
                let item = item.trim();
                if item.is_empty() {
                    return None;
                }
                let (name, alias) = split_alias(item, " as ");
                Some((name.trim().to_string(), alias.map(ToString::to_string)))
            })
            .collect(),
    )
}

/// Extract the first quoted string from text.
fn quoted_text(text: &str) -> Option<&str> {
    for quote in ['"', '\''] {
        let Some(start) = text.find(quote) else {
            continue;
        };
        let rest = &text[start + quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            continue;
        };
        return Some(&rest[..end]);
    }
    None
}

/// Resolve a relative TypeScript module specifier to a repository stem path.
fn resolve_relative_module_path(caller_path: &str, module_spec: &str) -> Option<String> {
    if !(module_spec.starts_with("./") || module_spec.starts_with("../")) {
        return None;
    }
    let mut components = caller_path
        .rsplit_once('/')
        .map_or(Vec::new(), |(parent, _file)| {
            parent
                .split('/')
                .filter(|component| !component.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        });
    for component in module_spec.split('/') {
        match component {
            "." | "" => {}
            ".." => {
                components.pop();
            }
            value => components.push(value.to_string()),
        }
    }
    Some(strip_known_source_extension(&components.join("/")))
}

/// Return a dotted module name from a repository stem path.
fn module_name_from_path(path: &str) -> String {
    path.trim_start_matches("src/").replace('/', ".")
}
