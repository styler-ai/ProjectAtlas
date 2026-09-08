## Context

Each MCP server owns a source-observation registry. Entries own notify watchers for a repository root and, when outside that root, the configuration parent. Pinned notify 8.2.0 on Windows queues Unwatch and Stop asynchronously. Temporary-directory cleanup can overlap an outstanding ReadDirectoryChangesW request.

## Goals / Non-Goals

Complete owned Windows cancellation before entry drop returns, including partially failed startup. Preserve existing identity, observation, root isolation, errors, and non-Windows behavior. No new watcher wrapper, dependency, retries, timeout policy, database changes, or thread pool.

## Decisions

Use the existing entry as a Rust RAII owner before either watch registration. Derive both registration and teardown paths from its binding. On Windows, enqueue unwatch for those paths, then call the existing synchronous configure acknowledgement. The backend processes these actions in order: unwatch cancels and closes the directory request and waits for completion before configure acknowledges. Unwatch alone is insufficient; a new worker/join framework would duplicate the dependency's existing mechanism.

Teardown is constant work for at most two watches and performs no source traversal, publication, or database access. Drop remains non-panicking; disconnected backend errors mean the worker cannot process further requests. The Windows acknowledgement result does not indicate a configuration change and is not used as one.

## Risks / Trade-offs

- Backend ordering is dependency-specific: document the pinned behavior beside the barrier and protect real native directory cleanup in tests.
- A second registration can fail: construct the entry first so its Drop owns the successfully registered root.
- Completion waits for the native backend: preserve its cancellation mechanism rather than adding retries or accepting early cleanup.

## Migration Plan

No data migration. Merge the shared runtime fix, refresh dependent worktrees, and rerun affected proof. Reverting source restores the old lifecycle without changing stored data.

## Dependencies / Cross-Issue Impact

#570 has no implementation prerequisite. It belongs to #492 and blocks #465 validation. Merge the shared baseline repair before refreshing #465; #358 remains queued after document closure.

## Open Questions

None.
