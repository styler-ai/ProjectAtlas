//! Cargo package and dependency semantics.

use projectatlas_core::symbols::SymbolKind;

/// Return whether a Cargo fact exports one package identity.
pub(super) const fn is_export_candidate(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Package)
}
