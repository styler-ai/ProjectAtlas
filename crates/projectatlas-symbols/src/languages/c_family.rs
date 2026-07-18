//! C and C++ language augmentation boundary.

use projectatlas_core::symbols::SymbolGraph;

/// Keep a dedicated boundary for future C/C++ parser corrections.
pub(super) fn augment(_graph: &mut SymbolGraph, _content: &str) {}
