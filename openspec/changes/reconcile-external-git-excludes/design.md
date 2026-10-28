## Context

The shared assumption that ignore's convenience configuration covers all existing policy inputs is false. Its global-config regex cannot parse a quoted pathname containing spaces. Its worktree common-dir selection needs the Git file type, but scanner require_git(false) prevents collecting that type. The latter setting intentionally supports .gitignore without a Git repository and cannot simply be removed globally. RootScanPolicy and policy witness already know shared worktree metadata; full traversal does not consistently consume it.

## Goals / Non-Goals

One effective selection contract across full traversal, targeted refresh, absent-document classification and observed policy inputs. Preserve local ignore precedence/negation, no-Git and linked-worktree workflows, explicit trusted operator paths, cancellation and normal source navigation. No custom config grammar, Git subprocess, public route/schema or authored-state migration.

## Decisions

Keep interpretation at the filesystem policy owner. Reuse the existing root/structural common-Git discovery, ignore glob matching and canonical bounded config-value parsing. Select actual core.excludesFile values by section/key and existing precedence rather than matching arbitrary text; retain correctly quoted/escaped path contents and the established default fallback. Resolve configuration inputs once per policy construction where possible, preserve missing selected paths for observation, and do not repeatedly parse config for every walked entry.

Configure traversal so the same external global/common-Git matchers apply at their established precedence below local .ignore/.gitignore rules. Where WalkBuilder options are used, preserve no-Git behavior and explicitly prove linked control files, canonical/operator root aliases, rooted and nested patterns and whitelist overrides. Do not add external files as ordinary project nodes or make an external config choice a source-root escape. Reuse existing trusted configuration capabilities rather than a generalized filesystem jail.

Use a minimal canonical Git-config crate with default features disabled when supported, bounded input reads and no automatic unbounded includes/network/process execution. Evaluate existing selection and include semantics during the broader audit before adding or excluding behavior; record verified boundaries. The selected parser handles config syntax; project code owns source precedence, cancellation and input inventory. Review the exact pinned version, dependency graph, MSRV, licenses and measured construction/scan overhead before acceptance. A home-grown parser or normal Git command invocation is the rejected alternative.

The precedence contract is local .ignore, repository .gitignore, common Git
info/exclude, then the selected global excludes file. In particular a global
whitelist cannot undo a common exclusion; a higher-priority local whitelist can.
Preserve the existing ignore engine's per-directory matching and nested rules.

The supported config-source boundary is direct global/system files, not a full
Git repository configuration loader. Read sources in increasing precedence:
existing system default /etc/gitconfig (or GIT_CONFIG_SYSTEM), XDG_CONFIG_HOME/
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

Source observation records selected configuration and excludes paths, including supported environment-selected sources, shared common-dir pointers and absent inputs. A changed policy triggers existing reconciliation; unrelated external content is not indexed. Keep the source-policy and publication ownership/cancellation model.

Advance the extraction derivation contract after combining independently accepted fixes; it must be strictly newer than every exercised private candidate (currently #731 contract10), and retain the accepted semantic contract. Repair omitted/excess prior source facts, not only document reasons. No query-time mutation or alternate database.

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
