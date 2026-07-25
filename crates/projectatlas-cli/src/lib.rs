//! Shared process-owning boundaries used by `ProjectAtlas` CLI adapters.

#[cfg(feature = "optional-parser-supervisor")]
pub mod optional_parser_lifecycle;
#[cfg(feature = "optional-parser-supervisor")]
pub mod parser_supervisor;
