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
        let mut collisions = vec![false; self.graph.symbols.len()];
        check()?;
        let Ok(parents) = crate::declaration_identity::qualified_symbol_parents(self.graph) else {
            // Publication rejects invalid identities; they cannot grant proof here.
            collisions.fill(true);
            return Ok(collisions);
        };
        let mut identities = BTreeMap::new();
        for (ordinal, symbol) in self.graph.symbols.iter().enumerate() {
            check()?;
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
                if let Some(scope) = self.declaration_scopes[ordinal] {
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
