//! Cargo package and dependency semantics.

use projectatlas_core::symbols::SymbolKind;

/// Return whether a Cargo fact exports one package identity.
pub(super) const fn is_export_candidate(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_packages_are_export_candidates() {
        assert!(is_export_candidate(SymbolKind::Package));
        assert!(!is_export_candidate(SymbolKind::Dependency));
    }
}
