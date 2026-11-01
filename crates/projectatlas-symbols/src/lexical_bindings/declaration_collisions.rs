//! Purpose: Withdraw semantic authority when distinct lexical declarations share a graph selector.

use super::{BindingTarget, LexicalCollector, Scope};
use crate::{SourceExportBinding, SourceExportFact};
use std::collections::BTreeMap;
use std::sync::Arc;

impl LexicalCollector<'_> {
    /// Index each already capped raw declaration once, without per-call graph scans.
    ///
    /// The scratch rows are bounded by the raw graph's symbol admission, like the
    /// existing declaration ordinal index. Parent strings use the graph identity
    /// byte bound and the same owner as graph publication, including compaction.
    pub(super) fn declaration_identity_collisions<E>(
        &self,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<bool>, E> {
        collision_ordinals(self.graph, &self.declaration_scopes, check)
    }
}

/// Compare only raw rows that survive the same field policy as graph admission.
pub(super) fn collision_ordinals<E>(
    graph: &projectatlas_core::symbols::SymbolGraph,
    declaration_scopes: &[Option<usize>],
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Vec<bool>, E> {
    let mut collisions = vec![false; graph.symbols.len()];
    let mut admitted = Vec::with_capacity(graph.symbols.len());
    for (ordinal, symbol) in graph.symbols.iter().enumerate() {
        check()?;
        let valid = |value: &str| {
            crate::declaration_identity::validate_source_symbol_identity(value).is_ok()
        };
        let signature = if symbol.signature.is_empty() {
            &symbol.name
        } else {
            &symbol.signature
        };
        if valid(&symbol.name) && symbol.parent.as_deref().is_none_or(valid) && valid(signature) {
            admitted.push(ordinal);
        } else {
            // Rejected declarations cannot grant targets, but do not erase
            // independent valid declarations before source-fact remapping.
            collisions[ordinal] = true;
        }
    }
    let Ok(parents) =
        crate::declaration_identity::qualified_symbol_parents_selected(graph, admitted)
    else {
        // Invalid derived qualification cannot reach canonical publication.
        collisions.fill(true);
        return Ok(collisions);
    };
    let mut identities = BTreeMap::new();
    for (ordinal, symbol) in graph.symbols.iter().enumerate() {
        check()?;
        if collisions[ordinal] {
            continue;
        }
        let signature = if symbol.signature.trim().is_empty() {
            symbol.name.as_str()
        } else {
            symbol.signature.trim()
        };
        identities
            .entry((
                symbol.name.as_str(),
                symbol.kind.to_string(),
                parents[ordinal].as_ref(),
                signature,
            ))
            .or_insert_with(Vec::new)
            .push(ordinal);
    }
    for ordinals in identities.values() {
        check()?;
        let mut owner = None;
        let mut distinct = false;
        for &ordinal in ordinals {
            check()?;
            if let Some(scope) = declaration_scopes[ordinal] {
                distinct |= owner.is_some_and(|owner| owner != scope);
                owner = Some(scope);
            }
        }
        if distinct {
            for &ordinal in ordinals {
                check()?;
                collisions[ordinal] = true;
            }
        }
    }
    Ok(collisions)
}

/// Withdraw whole winning groups once, before ordinary and import-candidate lookup.
///
/// Group members are visited at their binding owner, never rescanned per call.
pub(super) fn guard_scope_bindings<E>(
    scopes: &mut [Scope],
    collisions: &[bool],
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for scope in scopes {
        for names in [
            &mut scope.bindings,
            &mut scope.methods,
            &mut scope.namespaces,
        ] {
            for bindings in names.values_mut() {
                for binding in &mut bindings.transitions {
                    check()?;
                    let collision = match &binding.target {
                        BindingTarget::Declaration(ordinal) => collisions[*ordinal],
                        BindingTarget::Declarations(ordinals)
                        | BindingTarget::Imports {
                            declarations: ordinals,
                            ..
                        } => {
                            let mut collision = false;
                            for &ordinal in ordinals.iter() {
                                check()?;
                                collision |= collisions[ordinal];
                            }
                            collision
                        }
                        _ => false,
                    };
                    if collision {
                        binding.target = BindingTarget::Unresolved;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Keep raw export owners as blockers so fallback cannot restore an unsafe target.
pub(super) fn guard_exports<E>(
    exports: &mut [SourceExportFact],
    collisions: &[bool],
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for export in exports {
        check()?;
        export.binding = match &export.binding {
            SourceExportBinding::LocalDeclaration(ordinal) if collisions[*ordinal] => {
                SourceExportBinding::UnresolvedDeclarations(Arc::from([*ordinal]))
            }
            SourceExportBinding::AmbiguousDeclarations(ordinals)
                if ordinals.iter().any(|ordinal| collisions[*ordinal]) =>
            {
                SourceExportBinding::UnresolvedDeclarations(Arc::clone(ordinals))
            }
            _ => continue,
        };
    }
    Ok(())
}

/// Rust module and associated-item paths consume their own declaration evidence.
pub(super) fn guard_rust_declarations<E>(
    rust: &mut crate::SourceRustFacts,
    collisions: &[bool],
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    for declaration in &mut rust.declarations {
        check()?;
        declaration.binding = match &declaration.binding {
            crate::SourceRustBinding::Declaration(ordinal) if collisions[*ordinal] => {
                crate::SourceRustBinding::Unresolved
            }
            crate::SourceRustBinding::TypeDeclaration(ordinal) if collisions[*ordinal] => {
                crate::SourceRustBinding::UnresolvedType
            }
            _ => continue,
        };
    }
    for method in &mut rust.associated {
        check()?;
        if method
            .declaration
            .is_some_and(|ordinal| collisions[ordinal])
        {
            method.supported = false;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::collision_ordinals;
    use projectatlas_core::graph::{MAX_GRAPH_IDENTITY_BYTES, QUALIFIED_SYMBOL_SCOPE_PREFIX};

    #[test]
    fn rejected_symbol_fields_do_not_poison_valid_identity_groups() {
        let graph = crate::extract_symbol_graph(
            "neighbors.rs",
            Some("rust"),
            "fn outer(){\n{fn run(){}}\n{fn run(){}}\n}\nfn clean(){}\n",
        );
        let clean = graph
            .symbols
            .iter()
            .position(|symbol| symbol.name == "clean");
        assert!(clean.is_some(), "{graph:?}");
        let Some(clean) = clean else { return };
        for (field, invalid) in [
            (0, "bad\0name".to_string()),
            (0, format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}bad")),
            (1, "bad\0parent".to_string()),
            (1, format!("{QUALIFIED_SYMBOL_SCOPE_PREFIX}bad")),
            (2, "bad\0signature".to_string()),
            (2, "x".repeat(MAX_GRAPH_IDENTITY_BYTES + 1)),
            (2, format!(" {}", graph.symbols[clean].signature)),
        ] {
            let mut graph = graph.clone();
            let mut neighbor = graph.symbols[clean].clone();
            match field {
                0 => neighbor.name = invalid,
                1 => neighbor.parent = Some(invalid),
                _ => neighbor.signature = invalid,
            }
            let mut scopes = graph
                .symbols
                .iter()
                .enumerate()
                .map(|(ordinal, symbol)| Some(if symbol.name == "run" { ordinal + 1 } else { 0 }))
                .collect::<Vec<_>>();
            graph.symbols.push(neighbor);
            scopes.push(Some(99));
            let result = collision_ordinals(&graph, &scopes, &mut || Ok::<(), ()>(()));
            assert!(result.is_ok(), "{result:?}");
            let Ok(collisions) = result else { return };
            for (ordinal, symbol) in graph.symbols.iter().enumerate() {
                assert_eq!(
                    collisions[ordinal],
                    symbol.name == "run" || ordinal + 1 == graph.symbols.len(),
                    "field{field}: {graph:?}"
                );
            }
            assert!(!collisions[clean]);
        }
    }

    #[test]
    fn rejected_containers_do_not_enter_the_qualified_parent_stack() {
        let mut graph = crate::extract_symbol_graph(
            "neighbors.java",
            Some("java"),
            "class Outer{class Container{void clean(){}}}",
        );
        let container = graph
            .symbols
            .iter()
            .position(|symbol| symbol.name == "Container");
        let clean = graph
            .symbols
            .iter()
            .position(|symbol| symbol.name == "clean");
        assert!(container.is_some() && clean.is_some(), "{graph:?}");
        let (Some(container), Some(clean)) = (container, clean) else {
            return;
        };
        let mut rejected = graph.symbols[container].clone();
        rejected.parent = Some("Poison".to_string());
        rejected.signature = "bad\0signature".to_string();
        graph.symbols.insert(clean, rejected);
        let admitted = (0..graph.symbols.len())
            .filter(|ordinal| *ordinal != clean)
            .collect();
        let result =
            crate::declaration_identity::qualified_symbol_parents_selected(&graph, admitted);
        assert!(result.is_ok(), "{result:?}");
        let Ok(parents) = result else { return };
        assert_eq!(
            parents[clean + 1]
                .as_ref()
                .map(projectatlas_core::graph::GraphIdentityText::as_str),
            Some("Outer::Container")
        );
        assert!(parents[clean].is_none());
        // The unfiltered clone would poison the last active Container entry.
        let unfiltered = crate::declaration_identity::qualified_symbol_parents(&graph);
        assert!(unfiltered.is_ok(), "{unfiltered:?}");
        let Ok(unfiltered) = unfiltered else { return };
        assert_eq!(
            unfiltered[clean + 1]
                .as_ref()
                .map(projectatlas_core::graph::GraphIdentityText::as_str),
            Some("Poison::Container")
        );
        // With no admitted Container at all, publication retains only the
        // child's direct parent spelling, never its rejected ancestor's scope.
        let admitted = (0..graph.symbols.len())
            .filter(|ordinal| *ordinal != clean && *ordinal != container)
            .collect();
        let result =
            crate::declaration_identity::qualified_symbol_parents_selected(&graph, admitted);
        assert!(result.is_ok(), "{result:?}");
        let Ok(parents) = result else { return };
        assert_eq!(
            parents[clean + 1]
                .as_ref()
                .map(projectatlas_core::graph::GraphIdentityText::as_str),
            Some("Container")
        );
    }
}
