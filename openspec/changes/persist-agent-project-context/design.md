## Context

ProjectAtlas already solves source orientation with a purpose-led funnel and an indexed local-source view. It does not yet preserve the small set of project-wide facts an agent needs to understand why the project exists, how the system is shaped, which decisions govern it, which skills apply, and where a long-running initiative currently stands.

The Cline Memory Bank demonstrates the value of separating project brief, product context, active context, system patterns, technical context, and progress. Its Markdown hierarchy is intentionally simple, but it asks the agent to reread every file and can grow or drift without transactional bounds. The Memory Atlas keeps the useful responsibility separation while adapting it to ProjectAtlas: typed SQLite records, stable keys, bounded recovery, exact local-root ownership, and direct links back into purpose/graph/summary/slice navigation.

Current Codex capabilities narrow the gap but do not remove it. Codex can resume sessions, compact conversation history, persist a task goal, optionally generate personal memories, load `AGENTS.md`, discover skills progressively, connect MCP servers, and run trusted lifecycle hooks for startup, resume, clear, compact, and subagent start. These surfaces have different owners:

- transcript resume and compaction preserve recent conversational continuity;
- native goals preserve a current execution target;
- memories preserve useful host/user context and are not a project database contract;
- `AGENTS.md` preserves durable rules;
- skills and plugins preserve reusable workflows and integrations;
- ProjectAtlas preserves the selected project's current local-source atlas.

The Memory Atlas fills only the remaining project-local bird's-eye role. It must work for hosts with weaker native memory/goal support, but it must not become a second transcript, rule system, task tracker, or private host-memory writer.

Implementation is dependency-gated behind #308. The storage migration, authored/derived-state boundary, fresh read snapshot, selected-root rules, session brief, and streamlined MCP inventory must be stable before #314 lands.

## Goals / Non-Goals

**Goals:**

- Recover the overarching product goal, project scope, architecture, accepted decisions, system patterns, operating workflows, relevant skills/plugins, active checkpoint, blockers, and next action in one small deterministic view.
- Keep retained project context bounded after every successful mutation and force agents to replace, retire, or compact obsolete state instead of appending session history.
- Make each write an explicit reflection point: the caller submits current replacements plus obsolete/superseded identities, while deterministic lifecycle cleanup removes expired volatile state in the same transaction.
- Preserve durable authored facts across source and graph publication while keeping them independent from structural generations.
- Integrate recovery into startup, resume, post-compaction, and supported subagent entry without adding noisy success messages or hidden background mutation.
- Link orientation records to exact reusable folder, file, symbol, issue, OpenSpec, documentation, skill, and plugin selectors.
- Return reviewed/current attribution, lifecycle, trust, and selector-resolution state so project-authored context guides navigation without becoming higher-priority agent instruction.
- Keep MCP small: one bounded read tool, one atomic update tool, and additive recovery in the existing session brief.

**Non-Goals:**

- Storing transcripts, chain-of-thought, tool output, arbitrary notes, secrets, credentials, user profiles, or host-private memory.
- Replacing host goals, host memories, `AGENTS.md`, skills, plugins, GitHub issues, OpenSpec, or task/todo systems.
- Automatic semantic summarization inside the Rust runtime, network access, embeddings, vector search, or a cleanup daemon.
- A generic memory provider trait, event-sourced history, cross-project global memory, or one MCP tool per CRUD operation.
- Repeating #308 lane, migration, publication, root, or MCP infrastructure inside #314; #314 consumes those stabilized contracts.

## Decisions

### Model a bird's-eye atlas, not a transcript bank

Use a closed `MemoryAtlasKind` with responsibility-owned records:

- `project_goal`: the durable overarching outcome, distinct from a host's current execution goal;
- `project_scope`: product purpose, users, boundaries, and non-goals;
- `architecture`: component and ownership shape;
- `system_pattern`: durable patterns and critical paths;
- `decision`: accepted decisions and supersession links;
- `workflow`: operating and release workflows;
- `skill_route`: logical skill identifiers and applicability for the overarching project goal or active issue/checkpoint;
- `plugin_route`: logical plugin/capability identifiers and applicability;
- `checkpoint`: the current initiative/issue checkpoint, blockers, and next action.

Each record has a stable key, short summary, optional bounded detail, lifecycle class, attribution, reviewed/current state, timestamps, and typed references/selectors. Stable facts replace by `(kind, stable_key)`. Volatile checkpoints use a stable initiative key and replace in place. Decisions may explicitly supersede earlier keys; old decisions are removed or marked superseded rather than retained as an unbounded narrative. Expiry is valid only for explicitly volatile records; durable goal, scope, architecture, pattern, decision, workflow, skill-route, and plugin-route records never expire automatically.

Source selectors are portable and project-relative: folder, file, optional symbol identity/span, issue, OpenSpec change/task, or public documentation reference. Machine-specific absolute paths are rejected. A skill route uses a closed `project_goal` or `active_issue` scope and records required versus recommended status, deterministic read order, a concise applicability rationale, a portable logical host-resolvable selector, current/stale/unavailable resolution state, and an optional fingerprint or capability identity. It stores no copied skill body, install command, executable path, or machine-local path. Plugin routes follow the same portable-reference boundary.

Recovery deduplicates a skill required by both scopes while retaining both applicability reasons and the strongest requirement. Project-goal routes remain visible and ordered ahead of issue-only routes; issue-scoped routes cannot hide them. When the active checkpoint or issue changes, its routes are replaced or retired rather than accumulating, while project-goal routes remain until explicitly replaced or removed. Memory Atlas routing stays below system, developer, user, repository-instruction, and current-skill authority; the harness resolves every returned route through its trusted registry and reads the complete current instructions before implementation.

This preserves the useful Cline responsibilities without copying its file hierarchy:

| Memory Bank responsibility | Memory Atlas projection |
| --- | --- |
| project brief and product context | `project_goal` plus `project_scope` |
| system and technical patterns | `architecture`, `system_pattern`, `decision`, `workflow` |
| active context | one replaceable `checkpoint` per active initiative |
| progress | compact checkpoint/blocker/next-action state plus tracker references |
| rules and tools needed to continue | `skill_route` and `plugin_route` references |

Public documentation remains authoritative for full rationale. GitHub/OpenSpec remain authoritative for issue/task status. Memory Atlas rows are compact orientation and routing facts with references back to those authorities.

Alternative rejected: Markdown files. They are portable, but they cannot atomically enforce root binding, byte/row budgets, conditional updates, lifecycle cleanup, authored-state preservation, or coherent concurrent recovery. Export can be considered later only if an agent workflow proves it necessary.

### Use one context revision and atomic reflection updates

The database owns one nonnegative context revision. Read results return it as an opaque decimal string. `atlas_memory_update` carries the revision observed by the caller and one atomic batch:

- `upsert`: complete replacement records;
- `remove`: exact stable identities no longer useful;
- optional `supersedes` links and expiry for volatile records.

Validation, expected-revision comparison, replacement/removal, deterministic cleanup, budget reconciliation, and revision advance occur in one transaction. A successful state-changing batch advances once. An exact no-op does not alter revision, timestamps, or cleanup metadata. A stale writer, malformed record, wrong root, busy database, exhausted revision, or irreducible pressure changes nothing.

The host guidance makes each meaningful checkpoint a reflection boundary. Before calling the update tool, the agent must compare the proposed bird's-eye state with returned stable identities, remove facts that became obsolete, replace facts that changed, and leave unrelated durable facts untouched. The Rust runtime performs deterministic lifecycle cleanup; it does not pretend to understand semantic obsolescence without an agent decision.

Alternative rejected: an append-only event log. The product needs current orientation, not transcript archaeology. Stable replacement plus authoritative external references is smaller and bounded.

### Enforce hard budgets during every write

Configuration defines per-record, total retained UTF-8 byte, row, checkpoint, recovery-row, and recovery-output budgets below compiled hard maxima. Defaults are selected from representative ProjectAtlas fixtures during implementation, not guessed or duplicated across code and docs.

Every successful update ends within all budgets. Cleanup order is deterministic:

1. expired checkpoints;
2. records explicitly superseded or removed by the batch;
3. older volatile checkpoints beyond the configured allowance;
4. other lifecycle-expired volatile records using stable identity as the final tie-breaker.

Current `project_goal`, `project_scope`, `architecture`, active decisions/patterns/workflows, and active skill/plugin routes are protected from implicit deletion. If protected records prevent the new state fitting, the transaction rolls back and returns a content-free pressure report with sizes, identities, and suggested candidates. Explicit CLI compaction uses the same policy in dry-run/apply form.

This provides the user's required forcing function: memory cannot silently run over, and every write either includes an honest cleanup reflection, triggers deterministic expiry cleanup, or fails until the atlas is made smaller.

Alternative rejected: periodic cleanup. It permits persistent overflow after crashes and adds a daemon without improving agent orientation.

### Reuse existing authored-state and root ownership

Memory Atlas tables are authored state. Full scans, incremental graph publication, source deletion, watcher refresh, and derived-slot cleanup never replace them. Supported schema migration, repair, backup/restore, and rollback preserve them under the #308 database contracts.

All reads and writes use the selected project's canonical/physical root and per-call `project_path` rules. Missing, wrong-root, incompatible, or refresh-required databases fail explicitly; memory operations never initialize, scan, migrate, switch active roots, or fall back to another repository.

The context revision is independent from the structural generation. Recovery returns both when structural selectors are included, and cursors bind each stamp independently.

### Expose two Memory Atlas tools and enrich session brief

The agent MCP surface gains only:

- `atlas_memory`: bounded status/list/detail retrieval with kind/key/task filters, pressure, stable identities, and revision-bound pagination;
- `atlas_memory_update`: atomic conditional upsert/remove/reflection batches.

`atlas_session_brief` gains an optional recovery mode that composes the highest-value Memory Atlas rows with existing project identity, freshness, and next-call orientation. `atlas_settings` reports capability, effective budgets, pressure, revision, and counts without content. CLI provides the same read/update behavior plus validation and explicit compaction administration.

There is no separate Memory Atlas goal tool. The overarching project goal is a protected `project_goal` record; a host-native goal remains the current execution mechanism. This avoids the dual-goal system in the earlier draft and keeps tracker/task ownership clear.

TOON remains the default for uniform rows. The recovery projection may use a small typed topology when references form a path. Exact source remains available only through normal slice calls.

### Make recovery a small ordered bird's-eye view

Recovery mode returns, in order:

1. selected project identity, freshness, context revision, and pressure;
2. overarching project goal and scope;
3. architecture and system-pattern digest;
4. current initiative checkpoint, blockers, and next action;
5. applicable decisions and workflows;
6. deduplicated project-goal and active-issue skill routes in deterministic required-read order, followed by applicable plugin routes;
7. reusable folder/file/symbol/tracker selectors and the best next atlas call.

Rows are deterministically ranked and bounded. Repeated reads are byte-stable for the same database state, request, and effective lifecycle evaluation instant. The service captures one concrete evaluation instant per operation so expiry cannot change midway through a projection; tests inject a fixed value without introducing a clock trait or dependency. Truncation includes returned/omitted counts and a revision-bound cursor. The default view never dumps all memory.

This output rejoins the normal navigation sieve:

```text
Memory Atlas bird's-eye context
→ folder purpose plus graph role
→ file purpose plus relevant connections
→ summary plus trust/coverage
→ exact slice
```

The Memory Atlas does not decide which source is current. #308 freshness remains authoritative; stale structural state is reported separately from stale memory context.

### Complement documented host capabilities

Host integration is capability-driven and truthful:

- Codex: use `SessionStart` for `startup`, `resume`, `clear`, and `compact` plus `SubagentStart` when the plugin hook is trusted and enabled. The hook injects a fixed instruction to request read-only recovery; it does not write memory or goals. Recovery directs the agent to resolve and completely read the required project-goal skills followed by active-issue skills before implementation. Stable native goals may carry the current execution target. Experimental memories remain host-owned and optional. `AGENTS.md`, skills, and plugins remain their own instruction/workflow surfaces.
- Claude Code and OpenCode: use only documented skill/plugin/MCP/task lifecycle capabilities. Where automatic startup recovery is unavailable, packaged guidance makes the manual `atlas_session_brief` recovery explicit.
- Generic MCP: use the two Memory Atlas tools and session brief without assuming any native memory or goal API.

Successful recovery and routine checkpoint updates stay out of user-facing narration unless they change the plan, reveal pressure/conflict, or fail. No host integration reads transcript paths, personal memory directories, global config, or private caches.

Memory updates occur at meaningful recovery, architecture-decision, issue/task-transition, and final-verification boundaries—not after every file edit. This is frequent enough to remain useful and sparse enough to avoid noise and churn.

When a harness uses a quiet background maintainer, it supplies only the bounded current brief and the explicit task outcome/checkpoint. The maintainer uses the normal `atlas_memory_update` revision precondition, exact stable identities, and reflection rules. An exact no-op remains fully silent and changes neither revision nor timestamps. A stale background writer loses without retrying over newer accepted facts; conflicts, protected pressure, invalid roots, rejected content, and decisions requiring an owner are surfaced, while recovery and source navigation never wait for maintenance.

Alternative rejected: automatic PreCompact/PostCompact mutation. Those events do not provide a reliable semantic checkpoint transaction, and hidden writes could race or capture low-quality state. Session-start recovery is automatic when supported; authored updates remain explicit agent actions.

### Treat Memory Atlas as reviewed project data, not agent authority

Recovery labels every record's attribution, lifecycle/review state, and stale or unresolved references. Memory Atlas content cannot override system, developer, user, repository `AGENTS.md`, or currently loaded skill instructions. A route recommends only a logical installed capability and must be resolved through the harness's trusted registry and policy before its complete current instructions are read.

No stored field is interpolated into shell commands, SQL identifiers, filesystem roots, or plugin installation inputs. Validation failures and diagnostics never echo rejected content. This keeps useful project prose from becoming an execution or prompt-priority boundary.

### Rust pattern fit and crate ownership

- `projectatlas-core`: closed kinds, identities, references, lifecycle, revision, pressure, recovery, and request/response types.
- `projectatlas-db`: migration, constraints, coherent snapshots, conditional transaction, retained-byte accounting, and authored-state preservation.
- `projectatlas-service`: validation, task ranking, cleanup plan, recovery projection, and conflict/pressure policy.
- `projectatlas-cli`: CLI/MCP/settings adapters, host capability rendering, and plugin guidance.

Concrete structs/enums and existing service boundaries are sufficient. No trait objects, actors, background tasks, new dependency, or eighth crate are justified.

## Risks / Trade-offs

- **[Facts become stale]** → Store attribution, review time, references, and optional fingerprints; surface stale/unresolved selectors and require replacement rather than silently trusting them.
- **[Agents append low-value notes]** → Closed kinds, short field limits, stable keys, protected/volatile lifecycle, write-time reflection, and hard total budgets reject diary-like growth.
- **[Cleanup removes valuable context]** → Automatic cleanup targets only expired/superseded volatile rows; protected current facts require explicit revision/removal and pressure is visible before failure.
- **[Memory duplicates host or tracker state]** → Store the overarching project goal and compact checkpoint/reference only; hosts own live goals/memories and trackers own task status.
- **[Recovery becomes a context dump]** → Fixed priority, row/byte limits, typed truncation, and bounded follow-up reads.
- **[Private data leaks]** → Explicit bounded input only, no implicit ingestion, no content in ordinary settings/telemetry/logs/errors, and hostile sentinel tests. The runtime does not claim universal secret detection for caller-authored prose.
- **[Lifecycle hooks are unavailable or untrusted]** → Capability/status reporting and packaged manual recovery remain correct; no automatic behavior is claimed when the host cannot run it.
- **[Schema work races #308]** → Do not implement until #308's storage/session/compatibility surface is merged and frozen for this consumer.

## Migration Plan

1. Land the lean OpenSpec and synchronized issue without Memory Atlas runtime code.
2. After #308 stabilizes, add typed contracts, measured budgets, and migration/storage in one behavior slice.
3. Add shared service lifecycle, reflection batches, bounded reads, and recovery projection with shared tests.
4. Add CLI/MCP/settings and replay every existing request/default.
5. Add host guidance/hooks and ProjectAtlas dogfooding through the public tools.
6. Run focused tests after each significant slice; run the complete workspace, line-coverage, and mutation campaign once after all #314 behavior is finished.
7. Merge only when OpenSpec/GitHub tasks are synchronized and the combined issue is reviewed.

Before release, rollback removes the unshipped migration and additive surfaces together. After release, supported rollback uses the existing verified database backup/restore path; older runtimes reject the newer schema rather than downgrading it.

## Open Questions

No architecture choice blocks the specification. Exact default and hard budgets must be selected from representative recovery fixtures during implementation and then exposed through settings and documentation.
