//! Typed hard-budget contract for default-core indexing and query work.

use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Maximum viable targets retained for one ambiguous resolution.
pub const MAX_RESOLUTION_CANDIDATES: usize = 64;

/// Default-core resources governed by one hard ceiling and enforcement status.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultCoreBudgetKind {
    /// Maximum source file bytes accepted by structural parsing.
    SourceFileBytes,
    /// Maximum abstract-syntax-tree depth.
    AstDepth,
    /// Maximum symbols emitted for one file.
    SymbolsPerFile,
    /// Maximum relations emitted for one file.
    RelationsPerFile,
    /// Maximum viable targets retained for one ambiguous resolution.
    ResolutionCandidates,
    /// Maximum structural worker count.
    WorkerCount,
    /// Maximum duration of one structural stage.
    StageTime,
    /// Maximum working memory.
    WorkingMemory,
    /// Maximum bounded graph query depth.
    QueryDepth,
    /// Maximum nodes visited by one bounded query.
    VisitedNodes,
    /// Maximum edges expanded by one bounded query.
    ExpandedEdges,
    /// Maximum rows returned by one bounded query.
    ReturnedRows,
    /// Maximum serialized response bytes.
    ResponseBytes,
    /// Maximum cancellation polling interval.
    CancellationPoll,
    /// Maximum cancellation grace period.
    CancellationGrace,
}

impl DefaultCoreBudgetKind {
    /// Complete fixed-order default-core budget inventory.
    pub const ALL: [Self; 15] = [
        Self::SourceFileBytes,
        Self::AstDepth,
        Self::SymbolsPerFile,
        Self::RelationsPerFile,
        Self::ResolutionCandidates,
        Self::WorkerCount,
        Self::StageTime,
        Self::WorkingMemory,
        Self::QueryDepth,
        Self::VisitedNodes,
        Self::ExpandedEdges,
        Self::ReturnedRows,
        Self::ResponseBytes,
        Self::CancellationPoll,
        Self::CancellationGrace,
    ];

    /// Return the stable machine-contract identifier for this resource.
    #[must_use]
    pub const fn contract_id(self) -> &'static str {
        match self {
            Self::SourceFileBytes => "source_file_bytes",
            Self::AstDepth => "ast_depth",
            Self::SymbolsPerFile => "symbols_per_file",
            Self::RelationsPerFile => "relations_per_file",
            Self::ResolutionCandidates => "resolution_candidates",
            Self::WorkerCount => "worker_count",
            Self::StageTime => "stage_time",
            Self::WorkingMemory => "working_memory",
            Self::QueryDepth => "query_depth",
            Self::VisitedNodes => "visited_nodes",
            Self::ExpandedEdges => "expanded_edges",
            Self::ReturnedRows => "returned_rows",
            Self::ResponseBytes => "response_bytes",
            Self::CancellationPoll => "cancellation_poll",
            Self::CancellationGrace => "cancellation_grace",
        }
    }

    /// Return this resource's index in [`Self::ALL`].
    const fn index(self) -> usize {
        match self {
            Self::SourceFileBytes => 0,
            Self::AstDepth => 1,
            Self::SymbolsPerFile => 2,
            Self::RelationsPerFile => 3,
            Self::ResolutionCandidates => 4,
            Self::WorkerCount => 5,
            Self::StageTime => 6,
            Self::WorkingMemory => 7,
            Self::QueryDepth => 8,
            Self::VisitedNodes => 9,
            Self::ExpandedEdges => 10,
            Self::ReturnedRows => 11,
            Self::ResponseBytes => 12,
            Self::CancellationPoll => 13,
            Self::CancellationGrace => 14,
        }
    }

    /// Return the pre-registered hard ceiling for this resource.
    #[must_use]
    pub const fn hard_ceiling(self) -> u64 {
        match self {
            Self::SourceFileBytes => 2_000_000,
            Self::AstDepth => 256,
            Self::SymbolsPerFile | Self::VisitedNodes => 50_000,
            Self::RelationsPerFile => 250_000,
            Self::ResolutionCandidates => MAX_RESOLUTION_CANDIDATES as u64,
            Self::WorkerCount => 16,
            Self::StageTime => 300_000,
            Self::WorkingMemory => 536_870_912,
            Self::QueryDepth => 3,
            Self::ExpandedEdges => 200_000,
            Self::ReturnedRows | Self::CancellationGrace => 1_000,
            Self::ResponseBytes => 1_048_576,
            Self::CancellationPoll => 25,
        }
    }

    /// Return the physical or logical unit used by this resource.
    #[must_use]
    pub const fn unit(self) -> BudgetUnit {
        match self {
            Self::SourceFileBytes | Self::WorkingMemory | Self::ResponseBytes => BudgetUnit::Bytes,
            Self::AstDepth | Self::VisitedNodes => BudgetUnit::Nodes,
            Self::SymbolsPerFile => BudgetUnit::Symbols,
            Self::RelationsPerFile => BudgetUnit::Relations,
            Self::ResolutionCandidates => BudgetUnit::Candidates,
            Self::WorkerCount => BudgetUnit::Workers,
            Self::StageTime | Self::CancellationPoll | Self::CancellationGrace => {
                BudgetUnit::Milliseconds
            }
            Self::QueryDepth => BudgetUnit::Hops,
            Self::ExpandedEdges => BudgetUnit::Edges,
            Self::ReturnedRows => BudgetUnit::Rows,
        }
    }

    /// Return the truthful current default-core enforcement status.
    #[must_use]
    pub const fn default_enforcement(self) -> BudgetEnforcement {
        match self {
            Self::ResolutionCandidates => BudgetEnforcement::RuntimeEnforced,
            Self::SourceFileBytes
            | Self::AstDepth
            | Self::SymbolsPerFile
            | Self::RelationsPerFile
            | Self::WorkerCount
            | Self::StageTime
            | Self::WorkingMemory
            | Self::QueryDepth
            | Self::VisitedNodes
            | Self::ExpandedEdges
            | Self::ReturnedRows
            | Self::ResponseBytes
            | Self::CancellationPoll
            | Self::CancellationGrace => BudgetEnforcement::Advisory,
        }
    }

    /// Build this resource's default hard-budget record.
    #[must_use]
    pub const fn default_budget(self) -> DefaultCoreBudget {
        DefaultCoreBudget {
            kind: self,
            value: self.hard_ceiling(),
            enforcement: self.default_enforcement(),
        }
    }
}

impl fmt::Display for DefaultCoreBudgetKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.contract_id())
    }
}

/// Units used by default-core hard budgets.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetUnit {
    /// Bytes.
    Bytes,
    /// Syntax-tree or visited graph nodes.
    Nodes,
    /// Extracted symbols.
    Symbols,
    /// Extracted relations.
    Relations,
    /// Viable resolution candidates.
    Candidates,
    /// Worker threads.
    Workers,
    /// Milliseconds.
    Milliseconds,
    /// Graph hops.
    Hops,
    /// Expanded graph edges.
    Edges,
    /// Returned rows.
    Rows,
}

impl BudgetUnit {
    /// Return the stable machine-contract unit identifier.
    #[must_use]
    pub const fn contract_id(self) -> &'static str {
        match self {
            Self::Bytes => "bytes",
            Self::Nodes => "nodes",
            Self::Symbols => "symbols",
            Self::Relations => "relations",
            Self::Candidates => "candidates",
            Self::Workers => "workers",
            Self::Milliseconds => "milliseconds",
            Self::Hops => "hops",
            Self::Edges => "edges",
            Self::Rows => "rows",
        }
    }
}

/// Whether a hard ceiling is enforced by its runtime owner or remains advisory.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum BudgetEnforcement {
    /// Runtime code enforces the ceiling and records coverage when reached.
    #[serde(rename = "runtime_enforced")]
    RuntimeEnforced,
    /// The ceiling is reported but its owning runtime path is not yet wired.
    #[serde(rename = "advisory_until_implemented")]
    Advisory,
}

impl BudgetEnforcement {
    /// Return the stable machine-contract enforcement identifier.
    #[must_use]
    pub const fn contract_id(self) -> &'static str {
        match self {
            Self::RuntimeEnforced => "runtime_enforced",
            Self::Advisory => "advisory_until_implemented",
        }
    }
}

/// Validation failures for configured default-core budgets.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BudgetContractError {
    /// A configured limit is zero or exceeds its hard ceiling.
    #[error("invalid {kind} default-core budget {requested}; value must be in 1..={maximum}")]
    InvalidValue {
        /// Controlled default-core resource.
        kind: DefaultCoreBudgetKind,
        /// Rejected configured value.
        requested: u64,
        /// Maximum accepted value.
        maximum: u64,
    },
}

/// One validated configured default-core budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DefaultCoreBudget {
    /// Controlled default-core resource.
    kind: DefaultCoreBudgetKind,
    /// Configured nonzero value at or below the hard ceiling.
    value: u64,
    /// Current enforcement status.
    enforcement: BudgetEnforcement,
}

impl DefaultCoreBudget {
    /// Construct a validated configured default-core budget.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is zero or exceeds the resource's hard
    /// ceiling.
    pub fn new(kind: DefaultCoreBudgetKind, value: u64) -> Result<Self, BudgetContractError> {
        let maximum = kind.hard_ceiling();
        if value == 0 || value > maximum {
            return Err(BudgetContractError::InvalidValue {
                kind,
                requested: value,
                maximum,
            });
        }
        Ok(Self {
            kind,
            value,
            enforcement: kind.default_enforcement(),
        })
    }

    /// Return the controlled default-core resource.
    #[must_use]
    pub const fn kind(self) -> DefaultCoreBudgetKind {
        self.kind
    }

    /// Return the configured nonzero value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.value
    }

    /// Return the resource unit.
    #[must_use]
    pub const fn unit(self) -> BudgetUnit {
        self.kind.unit()
    }

    /// Return the current enforcement status.
    #[must_use]
    pub const fn enforcement(self) -> BudgetEnforcement {
        self.enforcement
    }
}

/// Fixed inventory of every default-core hard budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DefaultCoreBudgets {
    /// One entry for every [`DefaultCoreBudgetKind`].
    budgets: [DefaultCoreBudget; DefaultCoreBudgetKind::ALL.len()],
}

impl DefaultCoreBudgets {
    /// Return the configured budget for one default-core resource.
    #[must_use]
    pub const fn get(&self, kind: DefaultCoreBudgetKind) -> DefaultCoreBudget {
        self.budgets[kind.index()]
    }

    /// Return the complete fixed-order default-core budget inventory.
    #[must_use]
    pub const fn as_slice(&self) -> &[DefaultCoreBudget] {
        &self.budgets
    }

    /// Replace one configured budget while retaining the complete inventory.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` is zero or exceeds the resource's hard
    /// ceiling.
    pub fn with_budget(
        mut self,
        kind: DefaultCoreBudgetKind,
        value: u64,
    ) -> Result<Self, BudgetContractError> {
        let replacement = DefaultCoreBudget::new(kind, value)?;
        self.budgets[kind.index()] = replacement;
        Ok(self)
    }
}

impl Default for DefaultCoreBudgets {
    fn default() -> Self {
        Self {
            budgets: DefaultCoreBudgetKind::ALL.map(DefaultCoreBudgetKind::default_budget),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::io;

    /// Configuration cannot promote an advisory resource to runtime-enforced.
    #[test]
    fn configured_advisory_budget_cannot_claim_runtime_enforcement() -> Result<(), Box<dyn Error>> {
        let worker_budget = DefaultCoreBudget::new(DefaultCoreBudgetKind::WorkerCount, 8)?;
        require(
            worker_budget.enforcement() == BudgetEnforcement::Advisory,
            "configured worker budget claimed runtime enforcement",
        )?;

        let configured =
            DefaultCoreBudgets::default().with_budget(DefaultCoreBudgetKind::WorkerCount, 8)?;
        require(
            configured
                .get(DefaultCoreBudgetKind::WorkerCount)
                .enforcement()
                == BudgetEnforcement::Advisory,
            "budget inventory promoted advisory enforcement",
        )?;
        require(
            configured
                .get(DefaultCoreBudgetKind::ResolutionCandidates)
                .enforcement()
                == BudgetEnforcement::RuntimeEnforced,
            "runtime-owned resolution-candidate enforcement was lost",
        )?;
        Ok(())
    }

    /// Return a test error without introducing a production panic path.
    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message).into())
        }
    }
}
