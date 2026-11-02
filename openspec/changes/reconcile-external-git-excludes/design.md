## Context

The shared assumption that ignore's convenience configuration covers all existing policy inputs is false. Its global-config regex cannot parse a quoted pathname containing spaces. Its worktree common-dir selection needs the Git file type, but scanner require_git(false) prevents collecting that type. The latter setting intentionally supports .gitignore without a Git repository and cannot simply be removed globally. RootScanPolicy and policy witness already know shared worktree metadata; full traversal does not consistently consume it.

## Goals / Non-Goals

One effective selection contract across full traversal, targeted refresh, absent-document classification and observed policy inputs. Preserve local ignore precedence/negation, no-Git and linked-worktree workflows, explicit trusted operator paths, cancellation and normal source navigation. No custom config grammar, Git subprocess, public route/schema or authored-state migration.

## Decisions

Keep interpretation at the filesystem policy owner. Reuse the existing root/structural common-Git discovery, ignore glob matching and canonical bounded config-value parsing. Select actual core.excludesFile values by section/key and existing precedence rather than matching arbitrary text; retain correctly quoted/escaped path contents and the established default fallback. Resolve configuration inputs once per policy construction where possible, preserve missing selected paths for observation, and do not repeatedly parse config for every walked entry.

Configure traversal so the same external global/common-Git matchers apply at their established precedence below local .ignore/.gitignore rules. Where WalkBuilder options are used, preserve no-Git behavior and explicitly prove linked control files, canonical/operator root aliases, rooted and nested patterns and whitelist overrides. Do not add external files as ordinary project nodes or make an external config choice a source-root escape. Reuse existing trusted configuration capabilities rather than a generalized filesystem jail.

Capture the selected root's absolute lexical spelling at policy discovery only
when it differs from its canonical root. Anchor a relative selection to one
captured working directory before canonicalizing that same selection. Rebase
absolute targets beneath that exact component prefix onto the canonical root
before both Atlas exclusions and native ignore matching; relative targets stay
canonical-root-relative and unrelated absolute paths remain untouched. Do not
canonicalize absent targets or add target I/O. This prevents root aliases such
as macOS's temporary-directory spelling from being mistaken for excluded outside
paths before nested-policy validation. Canonical runtime plans and persisted
source rows remain unchanged, so this API correction does not require another
derivation increment. Prove canonical/selected/relative spelling, admitted and
ignored/missing targets, non-regular inputs, pruning and unchanged full-scan rows.
The native-byte worktree fixture must distinguish a filesystem's direct EILSEQ
filename refusal from a product defect: preserve raw-byte round trips where
representable, always retain typed lossless-UTF-8 rejection, and run real Unicode
Git/worktree lifecycle controls on the refusing filesystem rather than skipping
the platform or coercing invalid bytes into another path.

Synthetic source-observer events must model the registered native namespace.
The pinned notify 8.2.0 FSEvents backend stores a canonical registration and
filters delivered callback paths by that root prefix; Apple does not promise a
universal canonical spelling for all callback paths. A pre-canonical temporary
root spelling injected directly into the receiver is not evidence of a product
normalization defect. Derive synthetic source paths from the captured scan-plan
root, retain explicit database spelling/alias controls, and audit every fixture
consumer rather than changing production filtering to accommodate an impossible
producer path. Force the alias case on Unix to reproduce the fixture fault, then
prove real native events for writes through both selected and canonical root
spellings using distinct leaves so a delayed earlier event cannot satisfy the
next observation. Preserve the existing stale-read/mutation invalidation assertions.
The database-alias retarget fixture follows the same producer rule: force a
selected-root symlink on Unix, retain raw operator DB/root selections for I/O,
and derive structural/retarget event paths from the captured binding root. A
selected spelling is not a valid injected FSEvents callback merely because it
names the same object. Keep native target-sidecar delivery and genuine config/
ignore controls; do not add a production normalization exception for this fixture.

Windows parent-component fixtures must use ordinary Win32 spelling: a raw
verbatim `\\?\` path containing `..` is an invalid native filename, not proof
that callback filtering lost a valid selected path. Keep the normalized verbatim
positive control, assert an ordinary absolute Disk/UNC base before raw parent
construction, and read the parser-selected rules to prove native use. Do not add
a production normalization exception for unsupported verbatim parent syntax.
Limit display-spelling conversion to Windows so valid non-UTF-8 Unix roots remain
native paths. Keep relative-root coverage under captured CWD; use a separate
ordinary temporary base for the Windows alias case so a long CWD or another
volume cannot silently omit it. Compare each selection with its own canonical
root and complete node vector, retaining all three cases.

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

Replacing the selected source directory at the same pathname can also leave its recursive registration on the retired object, especially with an external database that survives the replacement. Retain structural uncertainty until the cold owner rebinds recursive coverage under the original work control, before exact source sampling; never add an unconditional warm-read rewatch. In the source-observation adapter, publish that uncertainty under the same receiver fence as the event and ingress sequence so final acceptance cannot acknowledge an empty queue while a rebind marker has already become visible. Ordinary access events without rescan retain their existing ignored behavior before any marker mutation; notify errors and genuine rescan/overflow still invalidate continuity. Add a deterministic callback/fence negative control and permanent CLI/MCP replacement proof with external database/config, unchanged project identity and all approved purposes, updated source results and no accidental conventional database creation. Existing root/config and registered-worktree lifecycle admission remain authoritative; this does not authorize bypassing an identity refusal or adopting a different root.

Continuous CLI batch classification must not require the captured root to exist
before the existing debounce window settles a supported replacement. An affecting
child or root event can arrive while the old directory has been renamed and the
prepared replacement has not yet taken its pathname. Treat only an exact-root
RootNotDirectory or exact-root canonicalization NotFound/NotADirectory as full-scan
uncertainty and retain recursive-rebind state; unrelated policy, parser and I/O
failures still propagate. Once full reconciliation is required, keep draining
native results under the original debounce, preserving notification errors,
disconnection and overflow, without classifying redundant path/document sets.
The existing plan reload, native rebind and fresh reconciliation remain authoritative
after settling: a persistently absent/file root still fails without publication.
Do not add a retry budget, suppress initial or one-shot root admission, or weaken
root/lifecycle/DB binding. Exercise the actual batch helper causally, including
child-before-root ordering, ordinary/non-regular policy and native-error controls;
the standalone test helper's fallback must not mask the production failure.

Moving continuous notify reconciliation to a caller-controlled entry point must
retain the existing incremental-limit fallback at that production owner. A
resource refusal for incremental entry/source-byte work or dependency closure
requires complete discovery and one atomic publication under the SAME original
control, not termination or a new polling budget. Keep raw captured/read-only
incremental consumers and their existing typed admission/fallback owners distinct.
Cancellation, deadline and unrelated failures still propagate; full reconciliation
retains its own resource/error controls. The oversized-batch regression must call
the actual controlled production entry, cover source and document-only batches,
discover an addition absent from the events, preserve identity/purposes and publish
one generation, with stopped controls retaining the last valid publication.

Runtime-event suppression must identify an admitted filename namespace, not any
source path that resolves to the same object. An explicit operator DB symlink
(or Windows managed 8.3 input) can name SQLite while its real main/WAL/SHM/journal
entries use the resolved target spelling. Capture that target alongside the
unchanged selected database path in the process-local binding; include it in the
observer key so alias retargeting cannot reuse an old binding. Filter the bounded
selected and target namespaces by parent/native entry name. Preserve distinct
source symlink/hardlink names and aliases to the metadata directory, structural
main-file changes, target uncertainty and genuine configuration/policy events.
Do not retarget actual SQLite opens or blanket-ignore metadata descendants.
Reuse the existing Windows literal-entry lookup, and require actual directory
case behavior before folding a missing sidecar suffix: an 8.3 alias alone is not
proof of case insensitivity. With no captured target, keep uncertainty observable.

This adds one optional path to each of at most sixteen process-local observer
bindings, one target lookup at binding construction on each observed request
(including warm requests), and constant bounded work
per relevant event (at most two namespace bases and three sidecars). Native
callbacks still perform no policy/filesystem I/O; ordinary warm drains with no
events add no filter work. Existing cooperative controls and source/publication
owners remain authoritative. No schema, extraction-fact contract, retry budget,
filesystem-jail or universal hostile-path-race guarantee is added. Prove the
actual native target-sidecar notification, CLI/MCP reads and purpose writes
through an external DB symlink, real Windows short-name admission, stale document
invalidation through a distinct DB-object alias, removed sidecars, retargeting,
and selected config/ignore-file positive controls.

Observed epochs consume the complete shared change set, including document-only
paths omitted from indexed source by repository/global policy. An ignored target
can become an outside-root symlink without changing any source node. Use the
existing `WatchChangeSet::has_changes` owner instead of duplicating a narrower
source-only predicate; preserve the ingress fence and invalidate the epoch without
requesting a recursive rebind for an ordinary document target. Reads then return
the existing typed policy-drift guidance without mutating publication; explicit
refresh reconciles the reason with unchanged source parses. Cover both directions
under .ignore, no-Git .gitignore and selected global rules through real persistent
MCP, plus deterministic all-platform receiver/epoch invalidation and existing
runtime-path/access/outside filtering, source and structural uncertainty controls.

Policy-only proof includes absent-document classification with an unchanged source inventory: no source-node delta is not evidence that document admission/reasons are unchanged. Reproduce any mismatch through real CLI/MCP before choosing a repair, then reuse the existing bounded reconciliation/projection owner rather than hashing every external edit into an unconditional all-source reparse. Preserve last-valid state on failed observation/reconciliation.

Selected external configuration and excludes must be repeatable file inputs. Reject unsupported non-regular inputs promptly under existing typed failure semantics while retaining regular files and symlinks to regular files. Unix config opens avoid waiting on a FIFO replaced between metadata and open; do not claim hard kernel-I/O preemption or universal adversarial race freedom from cooperative controls. Cover failure and positive operator workflows before declaring the guard complete.

The native ignore parser also opens .ignore, .gitignore and common info/exclude paths. Static FIFO reproductions in installed0.5.0 block those reads. Validate root/ancestor policy inputs before matcher construction, and admitted nested-directory inputs in the traversal admission filter before its worker opens local rules; propagating an error after workers finish must never return a partial successful scan. Retain only the first parallel guard failure and stop further policy I/O/queued work. For targeted matching, first match each parent with its already validated ancestors and preserve ignored-parent pruning before inspecting rules inside that parent. Legacy Git-only helpers validate only their enabled family. Missing/non-directory prospective ancestors and regular-file symlinks retain compatibility. This adds no preflight whole-tree traversal or replacement ignore grammar and does not claim immunity to every replacement race between metadata checks and library open.

Policy-only document freshness uses indexed exclusive keyset pages256 of minimal unresolved-document inputs, the same target normalizer/classifier and an uncached probe. Existing source-inventory maps remain budgeted; target strings are retained only for one page. A first mismatch establishes policy drift, not a complete affected closure or fabricated changed-path totals, and the captured source forces the existing atomic full graph projection while reusing unchanged symbol parses. Source generation/project fencing and staged target-state revalidation remain mandatory. Unchanged/no-document cases remain no-op, and newly indexed target state uses the ordinary source delta. There is no new schema, authored metadata authority, extraction-fingerprint policy-byte hash or query-side mutation.

The uncached probe is another consumer of the document-policy reader covered by #738. Put its accepted native-prefix admission at `DocumentResolutionIndex::inspect_absent_reason`, before `RootScanPolicy::excludes_path`, so cached projection, staged revalidation and the new uncached freshness probe share the same guard. Check every existing component against the captured canonical root, including escape-then-reentry, and borrow the existing operation control for construction, cached/uncached admission and publication revalidation. Preserve ordinary missing/ignored/folder/case/link outcomes and legitimate explicit global/shared-worktree policy; no blanket operator-path restriction. Causal tests must exercise both cached and uncached calls. This shared owning-boundary prerequisite does not claim #738's complete platform/upgrade/installed acceptance or close that separate issue.

Publication revalidation must discover current policy from the original plan's configured `ScanOptions`, not reuse the staged compiled matcher or feed its repository-expanded exclusions back into discovery. A selected external edit can change only an absent target's reason while the complete source inventory stays identical. Pass the original options and existing control into the publication owner, discover policy once when retained document states need validation, and refuse changed outcomes before acquiring the publication transaction. No new policy-byte contract hash, retained options clone or filesystem witness registry is required. A deterministic stage-to-publication control must flip an external exclusion without changing source nodes, prove last-valid state survives refusal, and prove retry/no-op convergence.

Advance the extraction derivation contract after combining independently accepted fixes; it must be strictly newer than every exercised predecessor candidate and retain the accepted semantic contract. Accepted main now includes #731 and #741 at derivation 24 / semantic 23. The registered private-worktree inventory also has maximum derivation 24 before this change, so this policy repair uses derivation 25 and retains semantic 23. Repair omitted/excess prior source facts, not only document reasons. No query-time mutation or alternate database. Later combined release acceptance must reconcile any subsequent independent derivation changes.

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
