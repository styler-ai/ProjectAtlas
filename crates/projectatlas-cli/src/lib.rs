//! Shared process-owning boundaries used by `ProjectAtlas` CLI adapters.

#[cfg(feature = "optional-parser-supervisor")]
pub mod optional_parser_lifecycle;
#[cfg(all(
    feature = "optional-parser-supervisor",
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(crate) mod parser_linux_authority;
#[cfg(feature = "optional-parser-supervisor")]
pub mod parser_supervisor;
