## Context

The shared assumption that ignore's convenience configuration covers all existing policy inputs is false. Its global-config regex cannot parse a quoted pathname containing spaces. Its worktree common-dir selection needs the Git file type, but scanner require_git(false) prevents collecting that type. The latter setting intentionally supports .gitignore without a Git repository and cannot simply be removed globally. RootScanPolicy and policy witness already know shared worktree metadata; full traversal does not consistently consume it.

## Goals / Non-Goals

One effective selection contract across full traversal, targeted refresh, absent-document classification and observed policy inputs. Preserve local ignore precedence/negation, no-Git and linked-worktree workflows, explicit trusted operator paths, cancellation and normal source navigation. No custom config grammar, Git subprocess, public route/schema or authored-state migration.

## Decisions

Keep interpretation at the filesystem policy owner. Reuse the existing root/structural common-Git discovery, ignore glob matching and canonical bounded config-value parsing. Select actual core.excludesFile values by section/key and existing precedence rather than matching arbitrary text; retain correctly quoted/escaped path contents and the established default fallback. Resolve configuration inputs once per policy construction where possible, preserve missing selected paths for observation, and do not repeatedly parse config for every walked entry.

Configure traversal so the same external global/common-Git matchers apply at their established precedence below local .ignore/.gitignore rules. Where WalkBuilder options are used, preserve no-Git behavior and explicitly prove linked control files, canonical/operator root aliases, rooted and nested patterns and whitelist overrides. Do not add external files as ordinary project nodes or make an external config choice a source-root escape. Reuse existing trusted configuration capabilities rather than a generalized filesystem jail.

Use canonical Git-config parsing with bounded input reads and no automatic unbounded includes/network/process execution. The implementation selects pinned `gix-config` 0.62.2 with default features disabled and its required `sha1` feature; that feature satisfies the dependency's hash-type contract and does not change ProjectAtlas content hashing. The parser has a transitive dependency graph, not a standalone tiny syntax-only footprint. Project code owns direct source precedence, cancellation and input inventory. Review the exact locked graph, MSRV, licenses and measured construction/scan overhead before acceptance. A home-grown parser or normal Git command invocation is the rejected alternative.

Pinned `ignore` 0.4.34 repairs Git-control type discovery when `git_exclude` is enabled even with `require_git(false)`. Keep native common-dir and nested Git behavior rather than installing a second hand-written common matcher. Configure one `WalkBuilder` with native .ignore/.gitignore/info-exclude, disable the incomplete built-in global-config selector, and add the canonically selected global excludes at the engine's lowest precedence, anchored at the selected root. Reuse `build_matchers`/`IncrementalIgnore` for targeted checks so inherited parents, subroot selection and ignored-parent pruning agree with traversal. Reuse compiled external policy within the operation/batch; do not retain a stale, unbounded incremental matcher across unrelated source epochs.

The precedence contract is local .ignore, repository .gitignore, common Git
info/exclude, then the selected global excludes file. In particular a global
whitelist cannot undo a common exclusion; a higher-priority local whitelist can.
Preserve the existing ignore engine's per-directory matching and nested rules.

The supported config-source boundary is direct global/system files, not a full
Git repository configuration loader. Read sources in increasing precedence:
existing Unix system default /etc/gitconfig (or explicit GIT_CONFIG_SYSTEM on supported platforms), XDG_CONFIG_HOME/
git/config (HOME/.config/git/config when XDG is unset/empty), then HOME/.gitconfig.
GIT_CONFIG_GLOBAL replaces both ordinary global files; an explicitly empty
GIT_CONFIG_GLOBAL or GIT_CONFIG_SYSTEM disables that corresponding source.
Honor GIT_CONFIG_NOSYSTEM. Within the case-insensitive core/excludesFile key,
last value wins, including repeated sections/keys and higher-priority files;
other sections/subsections cannot authorize this key. An explicit empty value
disables the global excludes file. With no configured value, select the existing
XDG git/ignore or HOME/.config/git/ignore fallback. Observe missing selected
sources too. Quoting, escapes, comments and continuation belong to the canonical
syntax parser. Preserve native absolute paths, resolve relative paths against the
captured selected working root, and expand a leading ~/ using the existing home
context without substituting HOME in the process environment.

As in the existing global-policy convenience boundary, include/includeIf files,
repository-local config overrides, Git -c/runtime config pairs, ~user and Git
installation/prefix discovery are not loaded or claimed as supported here.
Parse include declarations without following them; never infer values from
unread included files. Document this boundary and add no-follow controls rather
than silently claiming complete Git configuration semantics or adding unbounded
include discovery. If investigation establishes an existing supported regression
beyond this boundary, reopen the scope before calling the repair complete.
Bound config reads to1MiB per selected file and4MiB aggregate under cooperative
control before parsing; report a typed resource refusal rather than truncating.
Keep parsed policy per construction, not per entry, and measure the added work.

Source observation records selected configuration and excludes paths, including supported environment-selected sources, shared common-dir pointers and absent inputs. Continuous CLI and MCP observation must register the relevant external parents in the existing watcher, route a selected policy change to reconciliation before ordinary outside-path normalization drops it, and update coverage after a changed config selects another file. Observe absent-parent creation and supported symlink/ancestor replacement using bounded registrations; callbacks remain bounded queue/sequence work without policy I/O. Construct global policy once per drained batch/operation rather than once per event path. Never convert these watched external policy paths into source nodes. Keep existing continuity, cancellation, cleanup and Windows watcher acknowledgement guarantees.

Cold native coverage explicitly rebinds selected nonrecursive external parents before exact source sampling, so a directory replaced at the same pathname does not inherit a dead native registration. This is not warm-read work. Prefer bounded cold re-registration over a new retained-handle/platform-identity subsystem; the old epoch is invalidated before the gap, and source sampling plus final event draining follows installation. Re-discover selected aliases/configuration after installing coverage and retry at most three times under the original caller budget until the selected namespace agrees. If selection cannot stabilize or native coverage fails, retain existing exact/polling fallback rather than admitting a partially covered epoch. Preserve actual acknowledged registrations across partial failure for retry and Windows cleanup.

Native callback filtering drops only proven unrelated immediate children of selected external parents before the existing bounded queue. It performs no filesystem or policy I/O, preserves source-root, selected/control, unknown/Unicode-case/alias and rescan uncertainty, and does not special-case SQLite filenames. Relevant overflow still invalidates continuity. Preparation cleanup is serialized with reconciliation; failed/cancelled request cleanup may invalidate only its own sampled epoch, never a successor. These are causal consequences of extending external observation, not permission to weaken existing concurrency assertions or increase queues to conceal loss.

Policy-only proof includes absent-document classification with an unchanged source inventory: no source-node delta is not evidence that document admission/reasons are unchanged. Reproduce any mismatch through real CLI/MCP before choosing a repair, then reuse the existing bounded reconciliation/projection owner rather than hashing every external edit into an unconditional all-source reparse. Preserve last-valid state on failed observation/reconciliation.

Selected external configuration and excludes must be repeatable file inputs. Reject unsupported non-regular inputs promptly under existing typed failure semantics while retaining regular files and symlinks to regular files. Unix config opens avoid waiting on a FIFO replaced between metadata and open; do not claim hard kernel-I/O preemption or universal adversarial race freedom from cooperative controls. Cover failure and positive operator workflows before declaring the guard complete.

The native ignore parser also opens .ignore, .gitignore and common info/exclude paths. Static FIFO reproductions in installed0.5.0 block those reads. Validate root/ancestor policy inputs before matcher construction, and admitted nested-directory inputs in the traversal admission filter before its worker opens local rules; propagating an error after workers finish must never return a partial successful scan. Retain only the first parallel guard failure and stop further policy I/O/queued work. For targeted matching, first match each parent with its already validated ancestors and preserve ignored-parent pruning before inspecting rules inside that parent. Legacy Git-only helpers validate only their enabled family. Missing/non-directory prospective ancestors and regular-file symlinks retain compatibility. This adds no preflight whole-tree traversal or replacement ignore grammar and does not claim immunity to every replacement race between metadata checks and library open.

Policy-only document freshness uses indexed exclusive keyset pages256 of minimal unresolved-document inputs, the same target normalizer/classifier and an uncached probe. Existing source-inventory maps remain budgeted; target strings are retained only for one page. A first mismatch establishes policy drift, not a complete affected closure or fabricated changed-path totals, and the captured source forces the existing atomic full graph projection while reusing unchanged symbol parses. Source generation/project fencing and staged target-state revalidation remain mandatory. Unchanged/no-document cases remain no-op, and newly indexed target state uses the ordinary source delta. There is no new schema, authored metadata authority, extraction-fingerprint policy-byte hash or query-side mutation.

Advance the extraction derivation contract after combining independently accepted fixes; it must be strictly newer than every exercised private candidate (currently #731 derivation 11 / semantic 11), and retain the accepted semantic contract. Repair omitted/excess prior source facts, not only document reasons. No query-time mutation or alternate database.

## Risks / Trade-offs

Disabling require_git(false) everywhere breaks no-Git projects. Adding excludes at arbitrary priority breaks local negations. Re-reading config for each file or unbounded include expansion harms resource limits. Treating configured external paths as hostile breaks legitimate workflows. Use policy-owned cached bounded inputs, canonical config syntax, existing precedence, causal positive/negative controls and fresh measured costs. Dependency fan-out must be justified; no whole Git runtime is needed for value parsing.

## Migration Plan

Ordinary compatible refresh repairs unchanged old indexes under the existing atomic owner. Preserve root/project identity, all approved-purpose text/status, last-valid publication on failure, current-contract no-op and compatible rollback/re-upgrade. Coordinate the final contract and inventory union with #731/#738; #708 measures final combined source and #715 exercises exact RC packages on four platforms.

## Architecture Disposition

N/A: Existing filesystem selection and runtime publication owners remain authoritative. This repairs input interpretation and consistency within those owners without adding a service, data model or communication boundary.

## Dependencies / Cross-Issue Impact

#739 has no direct implementation blocker, belongs directly to #715 and blocks RC1. #738 retains its separate derived-document prefix guard; this issue restores the legitimate external policy inputs exercised by that positive workflow. #731's semantic contract is retained. #708/#715 require final combined source and exact installed selector coverage. Stable publication remains outside this goal.

## Open Questions

None.
