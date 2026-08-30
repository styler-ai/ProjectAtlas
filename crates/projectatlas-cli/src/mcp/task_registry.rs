//! Purpose: Own the bounded session-local MCP task registry and task lifecycle values.

use super::{
    MCP_TASK_CONTRACT_ID, MCP_TASK_PROGRESS_CONTRACT_MESSAGE, MCP_TASK_REGISTRY_CAPACITY,
    MCP_TOOL_ATLAS_TASK_STATUS, mcp_unix_time_ms,
};
use projectatlas_core::IndexWorkControl;
use serde::Serialize;
use std::collections::VecDeque;

/// Bounded in-memory registry for MCP task-progress records.
#[derive(Debug, Clone)]
pub(super) struct McpTaskRegistry {
    /// Session-local task records.
    records: VecDeque<McpTaskRecord>,
}

impl McpTaskRegistry {
    /// Create a registry with the built-in task-progress contract record.
    pub(super) fn new() -> Self {
        let now = mcp_unix_time_ms();
        let mut registry = Self {
            records: VecDeque::new(),
        };
        registry.insert(McpTaskRecord {
            task_id: MCP_TASK_CONTRACT_ID.to_string(),
            operation: McpTaskOperation::Contract,
            state: McpTaskState::Complete,
            created_at_ms: now,
            updated_at_ms: now,
            progress: Some(McpTaskProgress {
                current: Some(1),
                total: Some(1),
                message: Some(MCP_TASK_PROGRESS_CONTRACT_MESSAGE.to_string()),
            }),
            error: None,
            result_ref: Some(MCP_TOOL_ATLAS_TASK_STATUS.to_string()),
            cancelable: false,
            control: None,
        });
        registry
    }

    /// Return the number of admitted tasks that have not reached a terminal state.
    pub(super) fn active_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| !record.is_terminal_state())
            .count()
    }

    /// Return the number of retained task records.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.records.len()
    }

    /// Return the newest retained task id for an operation.
    #[cfg(test)]
    pub(super) fn latest_task_id(&self, operation: &McpTaskOperation) -> Option<String> {
        self.records
            .iter()
            .rev()
            .find(|record| &record.operation == operation)
            .map(|record| record.task_id.clone())
    }

    /// Insert or replace one task record while preserving the fixed registry capacity.
    pub(super) fn insert(&mut self, record: McpTaskRecord) {
        if let Some(existing_index) = self
            .records
            .iter()
            .position(|current| current.task_id == record.task_id)
        {
            let _removed = self.records.remove(existing_index);
        }
        while self.records.len() >= MCP_TASK_REGISTRY_CAPACITY {
            if let Some(finished_index) = self
                .records
                .iter()
                .position(McpTaskRecord::is_terminal_state)
            {
                let _evicted = self.records.remove(finished_index);
            } else {
                let _evicted = self.records.pop_front();
            }
        }
        self.records.push_back(record);
    }

    /// Return a task record by id.
    pub(super) fn get(&self, task_id: &str) -> Option<McpTaskRecord> {
        self.records
            .iter()
            .find(|record| record.task_id == task_id)
            .cloned()
    }

    /// Update a matching task through a bounded mutable pass.
    pub(super) fn update<F>(&mut self, task_id: &str, update: F) -> Option<McpTaskRecord>
    where
        F: FnOnce(&mut McpTaskRecord),
    {
        let record = self
            .records
            .iter_mut()
            .find(|record| record.task_id == task_id)?;
        update(record);
        Some(record.clone())
    }
}

/// One MCP task-progress record.
#[derive(Debug, Clone, Serialize)]
pub(super) struct McpTaskRecord {
    /// Opaque session-local task id.
    pub(super) task_id: String,
    /// Operation family.
    pub(super) operation: McpTaskOperation,
    /// Current task state.
    pub(super) state: McpTaskState,
    /// Creation timestamp in Unix milliseconds.
    pub(super) created_at_ms: u128,
    /// Last update timestamp in Unix milliseconds.
    pub(super) updated_at_ms: u128,
    /// Optional progress counters/message.
    pub(super) progress: Option<McpTaskProgress>,
    /// Concise failure diagnostic when present.
    pub(super) error: Option<String>,
    /// Result reference or follow-up tool when present.
    pub(super) result_ref: Option<String>,
    /// Whether this task can be canceled by the current server.
    pub(super) cancelable: bool,
    /// Shared cooperative cancellation boundary for active indexing work.
    #[serde(skip)]
    pub(super) control: Option<IndexWorkControl>,
}

impl McpTaskRecord {
    /// Return whether this record is in a terminal state and can be evicted first.
    pub(super) fn is_terminal_state(&self) -> bool {
        matches!(
            self.state,
            McpTaskState::Complete | McpTaskState::Failed | McpTaskState::Canceled
        )
    }
}

/// MCP task operation kind.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum McpTaskOperation {
    /// Contract/schema marker task.
    Contract,
    /// Repository scan and index operation.
    Scan,
    /// One-shot watch refresh operation.
    WatchOnce,
    /// Symbol projection rebuild operation.
    SymbolsBuild,
    /// Future search operation.
    Search,
}

/// MCP task lifecycle state.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum McpTaskState {
    /// Task has not started.
    Pending,
    /// Task is running.
    Running,
    /// Task completed successfully.
    Complete,
    /// Task failed.
    Failed,
    /// Task was canceled.
    Canceled,
}

/// Optional task progress fields.
#[derive(Debug, Clone, Serialize)]
pub(super) struct McpTaskProgress {
    /// Completed unit count when known.
    pub(super) current: Option<u64>,
    /// Total unit count when known.
    pub(super) total: Option<u64>,
    /// Concise progress message.
    pub(super) message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evictions_prefer_old_terminal_records() {
        let mut registry = McpTaskRegistry {
            records: VecDeque::new(),
        };
        registry.insert(McpTaskRecord {
            task_id: "running-0".to_string(),
            operation: McpTaskOperation::Search,
            state: McpTaskState::Running,
            created_at_ms: 0,
            updated_at_ms: 0,
            progress: None,
            error: None,
            result_ref: None,
            cancelable: true,
            control: None,
        });
        for index in 1..MCP_TASK_REGISTRY_CAPACITY {
            registry.insert(McpTaskRecord {
                task_id: format!("complete-{index}"),
                operation: McpTaskOperation::Search,
                state: McpTaskState::Complete,
                created_at_ms: index as u128,
                updated_at_ms: index as u128,
                progress: None,
                error: None,
                result_ref: None,
                cancelable: false,
                control: None,
            });
        }

        registry.insert(McpTaskRecord {
            task_id: "new-complete".to_string(),
            operation: McpTaskOperation::Search,
            state: McpTaskState::Complete,
            created_at_ms: 100,
            updated_at_ms: 100,
            progress: None,
            error: None,
            result_ref: Some(MCP_TOOL_ATLAS_TASK_STATUS.to_string()),
            cancelable: false,
            control: None,
        });

        assert_eq!(registry.len(), MCP_TASK_REGISTRY_CAPACITY);
        assert!(registry.get("running-0").is_some());
        assert!(registry.get("complete-1").is_none());
        assert!(registry.get("new-complete").is_some());
    }
}
