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
Ordinary Win32 lookup normalizes a parent component before opening the file;
an eliminated `intermediate/..` sibling is therefore not a policy dependency.
Prove the selected rules remain readable after removing that sibling and that
routing agrees with the normalized spelling on Windows. Retain the Unix
structural-component assertion and actual missing-parent creation controls;
do not broaden callback filtering to satisfy a Unix-only expectation.

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

A nonrecursive watch on the nearest external policy parent does not observe an
ordinary higher ancestor being renamed. For each external selected spelling or
resolved alias target, retain immediate-child namespace coverage at every existing
directory ancestor under the existing 256 unique-directory bound. Preserve the
nearest binding used by controlled drains and native alias/Win32 parent semantics.
Ordinary in-root inputs continue to use recursive source coverage; a lexical
root/../external selection is recognized by its nearest resolved parent. Existing
source-policy inputs already cover the source-root ancestor ignore chain. This
repair closes disconnected external branches without watching whole outside
trees. Unrelated immediate-child filtering remains callback-only lexical work;
failed or over-capacity installation retains the established exact/polling fallback.
Prove native rename/recreate at more than one ancestor depth, present/missing
leaves and intermediate parents, external and source-contained aliases, then a
distinct new leaf after cold rebinding so delayed earlier events cannot pass.

Native policy callback filtering drops only proven unrelated immediate children of selected external parents before the existing bounded queue. It performs no filesystem or policy I/O and preserves source-root, selected/control, unknown/Unicode-case/alias and rescan uncertainty. Runtime namespace routing remains with the CLI/source-observation adapters described below, rather than adding database rules to policy selection. Relevant overflow still invalidates continuity. Preparation cleanup is serialized with reconciliation; failed/cancelled request cleanup may invalidate only its own sampled epoch, never a successor. These are causal consequences of extending external observation, not permission to weaken existing concurrency assertions or increase queues to conceal loss.

Unknown arbitrary Unicode names cannot be compared using the fixed ASCII managed
name helpers or blanket Unix byte equality: supported Linux casefold and Apple
normalization/case rules differ. Classify known-parent sibling ingress as ignored,
relevant or requiring policy recheck. One atomic pending marker admits the first
uncertain event as a wake; later uncertain siblings share it without filling the
queue. Known selected/root/control and unscoped/error/rescan events retain normal
admission. Callbacks perform no native lookup or policy read.

The controlled drain consumes the marker before fresh inspection, so a later
callback can arm and queue another wake. MCP publication and consumption share
the receiver fence. Compare fresh selected bindings and the existing source
contract/policy witness with the current verified epoch; absence of an epoch
invalidates the in-flight exact candidate even if transient policy bytes reverted.
An unchanged verified epoch can acknowledge the coalesced sibling storm. Continuous
CLI uses the immutable installed selection's bounded content witness and native
bindings; changed policy invokes the existing rebind/reload/reconciliation owner.
Reuse the existing bounded policy-file hashing implementation for both witnesses,
not a second grammar, filename table or per-sibling state. Failed/cancelled
inspection preserves pending uncertainty and never acknowledges consumed changes.

OR structural uncertainty into the coalesced state even when a data event already
armed its wake. Unchanged canonical path strings and bytes do not prove that a
watch still follows a replaced directory object. Such uncertainty retains the
existing cold/full rebind owner; unrelated Unicode directory structure can still
require conservative reconciliation, but cannot fill the queue with one entry
per sibling. No universal hostile-local-filesystem availability guarantee is added.
For continuous CLI refresh, advance a recheck generation on every uncertain
callback, including those sharing an armed wake. Fence the actual initial and
incremental/full refresh calls with that generation. If uncertain ingress overlaps
a refresh, reload/rebind and reconcile again under the same original control
before counting a completed watch cycle; a change-and-revert cannot leave the
transient candidate as the final accepted result. No new timeout or retry budget
is introduced. A causal fixture exercises real publication while policy changes
and reverts inside this production fence.

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

A Unicode selected child can make even an ASCII sibling require policy recheck.
Therefore exact captured database and sidecar event spellings must reach ordinary
bounded runtime ingress without first arming policy uncertainty. Otherwise SQLite's
own WAL/SHM creation invalidates every cold MCP candidate before the drain can
apply its established native runtime filter. Use lexical equality against the
already captured selected/target namespace and three sidecars, with no callback
filesystem I/O or native-name folding. This routes events; it never suppresses
them or authorizes a stale target. Structural main-file, mixed, unknown spelling
and rescan events keep ordinary policy observation. The controlled MCP filter
still revalidates the selected target; a routed event retained by that filter
preserves full reconciliation rather than disappearing as an outside source path.
Coalesce exact runtime-only writes into one separate pending runtime wake, rather
than filling the source queue with SQLite's own WAL updates. Clear it before the
controlled drain (under MCP's existing receiver fence) and before exact-verification
queue reset. Main-file replacement bypasses this pending bit and retains native
rebind uncertainty, including in an ASCII policy parent that would otherwise
classify the database filename as an unrelated sibling. A later sidecar event can
arm a fresh wake after consumption. No new worker, unbounded event collection,
policy marker reuse or callback filesystem probe is introduced.
Prove real CLI/MCP reads and edits with a Unicode root and explicit external DB,
alongside runtime-alias retarget, distinct source alias and real policy controls.

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

Native watch discovery must distinguish a genuinely absent parent chain from a
confirmed non-directory intermediate component. Windows can return `NotFound`
where Unix returns `NotADirectory` for a child beneath a regular file. Reject
the confirmed file at `PolicyWatchInputs::add_path` using the existing I/O error
boundary, before registering the impossible selection. Preserve regular-file
leaves, symlink expansion and genuinely absent parents. Stop this check once a
closer ancestor resolves as a directory: ordinary Win32 parent normalization can
cancel earlier lexical file components. Prove both blocker depths, regular and
symlink leaves, missing-parent creation coverage and native ordinary `file/..`
compatibility alongside observer startup cleanup. Synthetic Windows events that
model notify must use the exact registered root; temporary-directory DOS short
aliases are a separate input spelling, not the backend's event producer.

Ordinary Win32 policy spellings must use native lexical normalization before
watch-scope capture, including each resolved alias target queued for expansion.
Reuse std::path::absolute under cfg(windows): it does not read the filesystem or
resolve symlinks, and leaves verbatim spellings unchanged. This removes canceled
intermediate/.. components from watch scopes without changing source-policy opens,
Unix traversal semantics, supported aliases, missing leaves or verbatim refusals.
The existing native control proves the rules file remains readable after deleting
the canceled intermediate, and that events on it cannot change policy selection.
Ordinary versus verbatim prefix spelling must not invent an external namespace
for an in-root file. Windows derives this from the already-resolved nearest
parent; a real outside junction/control parent still enables external ancestor
coverage even when the target is inside the source root. Keep Unix lexical
containment because its intermediate parent traversal can remain meaningful.
A lexical root/../external alias that resolves back inside the root is another
manifestation: the alias parent is an independently replaceable external control
on Unix too. Its real outside parent must enable higher ancestor coverage even
when the nearest target directory is in-root; retain a causal native selection
control for this case rather than relying on the target path alone.

Windows ancestor-move acceptance must distinguish a rejected mutation from a
missed event. The direct-layout native failure was the rename of control/current,
which contained the watched nested directory, before any alias case was reached.
Microsoft's FILE_RENAME_INFORMATION remarks describe restrictions involving open
handles below a renamed directory:
https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information.
Do not infer that FILE_SHARE_DELETE alone permits that ancestor move, or turn a
PermissionDenied return into a passing replacement assertion. Require an isolated
native control matrix for no handles, the target handle alone, a descendant
handle with notify's exact access/share/flags, and real notify watches. Expected
descendant denial must leave the tree unchanged; closing its handle must allow
the actual move. Notify teardown is asynchronous, so only that already observed
native denial may be retried under a bounded teardown acknowledgment; timeout
or any other error remains failure. Never register the same path twice in the
control, since the locked backend can replace its tracked state without stopping
the earlier request.

Keep all nine higher-ancestor layouts on Linux/macOS. Windows native positive
coverage instead exercises legal nearest-directory replacement, missing-parent
creation and external/source-contained directory aliases, retaining the same
selection/install/rebind/distinct-later-leaf owner. Static higher-ancestor routing
coverage remains shared. The separate Windows stop/move/restart control constructs
a fresh selection and watcher, installs through the real cold discovery owner,
and requires a distinct later policy event. This explicitly documents a native
backend lifecycle limit rather than claiming automatic higher-ancestor recovery
from tests of another operation. The isolated Windows matrix and restart control
must establish the limit natively before this acceptance is complete; no product
backend, source witness, quota, indexing deadline or security boundary changes.

Native platform jobs use the same limited Cargo dev/test debug metadata as the
Rust proof job. Hosted Linux exhausted disk while copying an independent runtime
for the real-host reader after both debug and release builds; leaving platform
jobs at full debug metadata missed the existing Rust-job resource control.
CARGO_PROFILE_DEV_DEBUG=1 and CARGO_PROFILE_TEST_DEBUG=1 retain unoptimized tests,
debug assertions and limited diagnostic information. Apply them to the platform
job from its first compilation, preserving independent executable copies, all
feature/target/test selectors, release-profile measurements and time budgets.
Do not hide copy failures, hard-link mutable runtime fixtures or delete unrelated
host SDKs to make this resource proof pass.

Native source-policy CI remains required under its existing ten-minute cap.
Place its unchanged commands after the existing twenty-minute PHP regression,
which owns cold symbol/CLI dependency compilation in this job. Full-mode Intel
macOS otherwise reaches the early unit step without a test build and can exhaust
its budget compiling shared dependencies before any CLI unit executes. This
ordering reuses the existing cold-build owner without relaxing a test deadline,
product indexing budget, feature matrix or selector. All-feature-only compilation
can still consume the retained native-step cap and needs actual hosted proof.

Windows native traces show repeated `Modify(Any)` notifications on the containing
Temp directory, not source edits: all 77 recursive-rebind traces at public
68cb2fd named that strict ancestor. Namespace spelling alone must not turn an
ordinary ancestor modification into lost recursive coverage. Keep these events
queued, but set callback rebind markers only for structural uncertainty, empty
events, errors or rescan. Source-root notifications still reach full verification
through the normal drain. Both continuous CLI and observed MCP drains use the
same selected-input classification.

Exact selected files and alias controls retain all relevant event kinds. Record
symlink/junction ancestors as control entries as well as resolving their selected
leaves. Exact Windows matching compares parent identity and native entry name,
without following the control leaf into its target directory; structural proper
ancestor matching retains the existing native-path comparison. Do not confuse a
target directory's metadata with a modification to its alias entry.

Retain each admitted input/control's already-resolved canonical target or absence
in the immutable watch selection. For nonstructural ancestor events, the drain
checks only affected bindings under its original work control, with checkpoints
between candidates and affected entries. Index canonical target and entry scopes
at selection time; resolve each event namespace once and use ordered prefix
lookup to find related selected spellings. Deduplicate binding checks across
overlapping event paths. Ordinary source-file events perform constant native
namespace work, independent of selected-input count and path depth; a directory
event performs at most one resolution per affected binding plus one per event
path. This is proportional to actual affected selection, not the 256-directory
watch limit: the resource regression includes 1,024 absent inputs sharing one
external directory. Windows exact entry matching also normalizes the candidate
once rather than resolving every selected entry. Suppress the event only when the
ancestor is still an ordinary directory and those namespace outcomes are
unchanged. A new link/reparse point, non-directory, missing ancestor, changed
binding or inspection failure retains uncertainty. Only `NotFound` means absence;
other resolution failures propagate. If a directory observed present disappears
during resolution, or its subsequent no-follow metadata shows a link or another
entry kind, retain uncertainty instead of looking up a raw or replaced target
scope. Likewise preserve disagreement when a present entry loses its containing
parent. A deterministic real-filesystem inter-call transition test covers removal
and native symlink/junction replacement through a lexical event spelling. This
is not a filesystem-snapshot guarantee. This preserves alias retargets even when
leaves remain absent, and Windows case-mode changes that invalidate a selected
spelling. Callbacks do no filesystem I/O. No file-object identity, content hash,
new watcher or durable state is introduced; space/work scales with selected
policy paths within the existing coverage and cooperative bounds.

This is a namespace check, not proof of which metadata flag changed:
[notify 8.2's Windows backend](https://docs.rs/notify/8.2.0/src/notify/windows.rs.html)
collapses native modified actions into `Modify(Any)`, including security events.
Retain explicitly typed access/ownership/unknown metadata notifications and the
existing source/policy witnesses, read failures and rescan behavior. Do not claim
flag-level discrimination, a filesystem snapshot or universal race immunity.
[Rust 1.98's native filesystem implementation](https://github.com/rust-lang/rust/blob/1.98.0/library/std/src/sys/fs/windows.rs)
recognizes name-surrogate reparse points and reads mount-point targets; use those
standard APIs for junction controls. Windows permits
[per-directory case changes](https://devblogs.microsoft.com/commandline/per-directory-case-sensitivity-and-wsl/)
and [reparse-point updates on empty directories](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fsa/4aeefef8-92c3-4abc-af7a-a610caf8a165),
so ordinary-directory type alone cannot prove selected bindings stayed valid.

Run the same filesystem/policy/observer unit commands immediately after native
target compilation to surface owning failures before installed-host/installer
work. The early unit step permits a ten-minute cold test build; product indexing
budgets are unchanged. Keep all three real CLI/MCP external-policy/document/root
replacement E2Es in their existing five-minute late step and retain every other
platform, feature flag and required gate.

Policy-only proof includes absent-document classification with an unchanged source inventory: no source-node delta is not evidence that document admission/reasons are unchanged. Reproduce any mismatch through real CLI/MCP before choosing a repair, then reuse the existing bounded reconciliation/projection owner rather than hashing every external edit into an unconditional all-source reparse. Preserve last-valid state on failed observation/reconciliation.

Full-refresh no-op admission must retain the distinction between a captured
document check and no captured check. An oversized document-only watch batch can
select full staging without a captured inventory; identical source nodes then
do not prove that its newly staged document reasons are unchanged. A known
captured document change can still bypass no-op admission; a false or absent
captured check is not a fresh policy snapshot. At the existing staged no-op
owner, first reject changed contracts, generations, authored imports and source
nodes; for every source-equal candidate, reuse the existing bounded keyset
document-policy probe over the staged source inventory before returning a no-op.
Recheck the publication generation after the paged reads and propagate the
existing generation-change refusal if it differs. Keep one pass-local policy
matcher, the original work control and quota/error propagation. Do not rescan
source, materialize another graph or open a publication transaction for this
check. Captured full-refresh source deltas exit at the source comparison without
a repeated probe. Prove oversized source/document batches and direct full-refresh
callers, unchanged-source reason transitions, no-op, unchanged parse reuse,
identity/purpose preservation and refusal without publication.

Selected external configuration and excludes must be repeatable file inputs. Reject unsupported non-regular inputs promptly under existing typed failure semantics while retaining regular files and symlinks to regular files. Unix config opens avoid waiting on a FIFO replaced between metadata and open; do not claim hard kernel-I/O preemption or universal adversarial race freedom from cooperative controls. Cover failure and positive operator workflows before declaring the guard complete.

The native ignore parser also opens .ignore, .gitignore and common info/exclude paths. Static FIFO reproductions in installed0.5.0 block those reads. Validate root/ancestor policy inputs before matcher construction, and admitted nested-directory inputs in the traversal admission filter before its worker opens local rules; propagating an error after workers finish must never return a partial successful scan. Retain only the first parallel guard failure and stop further policy I/O/queued work. For targeted matching, first match each parent with its already validated ancestors and preserve ignored-parent pruning before inspecting rules inside that parent. Legacy Git-only helpers validate only their enabled family. Missing/non-directory prospective ancestors and regular-file symlinks retain compatibility. This adds no preflight whole-tree traversal or replacement ignore grammar and does not claim immunity to every replacement race between metadata checks and library open.

Policy-only document freshness uses indexed exclusive keyset pages256 of minimal unresolved-document inputs, the same target normalizer/classifier and an uncached probe. Existing source-inventory maps remain budgeted; target strings are retained only for one page. A first mismatch establishes policy drift, not a complete affected closure or fabricated changed-path totals, and the captured source forces the existing atomic full graph projection while reusing unchanged symbol parses. Source generation/project fencing and staged target-state revalidation remain mandatory. Unchanged/no-document cases remain no-op, and newly indexed target state uses the ordinary source delta. There is no new schema, authored metadata authority, extraction-fingerprint policy-byte hash or query-side mutation.

The uncached probe is another consumer of the document-policy reader covered by #738. Put its accepted native-prefix admission at `DocumentResolutionIndex::inspect_absent_reason`, before `RootScanPolicy::excludes_path`, so cached projection, staged revalidation and the new uncached freshness probe share the same guard. Check every existing component against the captured canonical root, including escape-then-reentry, and borrow the existing operation control for construction, cached/uncached admission and publication revalidation. Preserve ordinary missing/ignored/folder/case/link outcomes and legitimate explicit global/shared-worktree policy; no blanket operator-path restriction. Causal tests must exercise both cached and uncached calls. This shared owning-boundary prerequisite does not claim #738's complete platform/upgrade/installed acceptance or close that separate issue.

Publication revalidation must discover current policy from the original plan's configured `ScanOptions`, not reuse the staged compiled matcher or feed its repository-expanded exclusions back into discovery. A selected external edit can change only an absent target's reason while the complete source inventory stays identical. Pass the original options and existing control into the publication owner, discover policy once when retained document states need validation, and refuse changed outcomes before acquiring the publication transaction. No new policy-byte contract hash, retained options clone or filesystem witness registry is required. A deterministic stage-to-publication control must flip an external exclusion without changing source nodes, prove last-valid state survives refusal, and prove retry/no-op convergence.

Advance the extraction derivation contract after combining independently accepted fixes; it must be strictly newer than every exercised predecessor candidate and retain the accepted semantic contract. Accepted main includes #731 and #741 at derivation 24 / semantic 23. The initial external-policy repair used derivation 25. Its required dependency gate then reported reachable RUSTSEC-2026-0433: Markdown extraction uses pulldown-cmark Options::all(), including wikilinks, so a small parser event count does not bound the vulnerable nested-image work between checkpoints. Upgrade only the locked pulldown-cmark 0.13.5 to patched 0.13.6; retain the supported options and ordinary/wiki references. The upstream deepest-embed correction changes some heading facts, so the final candidate uses derivation 26 and retains semantic 23. Repair omitted/excess prior source and heading facts, not only document reasons. No query-time mutation or alternate database. Prove the old heading outcome causally and include derivation-25 upgrade, no-op and compatible rollback/re-upgrade in final native proof. Later combined release acceptance must reconcile any subsequent independent derivation changes.

The cold unresolved-document probe pages rows but previously rebuilt a lazy
hierarchical matcher for each target. Pagination bounds row memory, not repeated
policy parsing. A bounded CLI reproduction at head 634cf78 has 1,024 unresolved
targets and 50,890 bytes of root policy; three cold summaries take a median
4.37718 seconds without changing publication. This establishes amplification,
not a demonstrated thirty-minute denial or a general latency acceptance result.

Own reuse in an additive filesystem `RootScanMatcher` borrowed from
`RootScanPolicy` and the original `IndexWorkControl`. Preserve the existing
`RootScanPolicy::excludes_path` signature and fresh single-path semantics. Reuse
pinned ignore 0.4.34 `IncrementalIgnore`, not a second grammar. Each
`DocumentResolutionIndex` owns one matcher through its existing single-threaded
interior-mutability boundary, including all pages of the uncached cold probe.
Keep native-prefix refusal before policy access and retain selected-root alias,
relative/native path, directory, inherited rule, no-Git and linked-worktree
semantics. No global cache, threads, synchronization service or schema change.

Admit root/ancestor local policy once before the first native parse, then warm
the root seed using the target's first normal component as a file. That loads no
child policy. Reuse the warmed seed when evicting child contexts. For each target,
match each parent as a directory, stop immediately if ignored, then validate and
reserve that parent's enabled local policy inputs before a subsequent native
match can load them. Share the existing validation/input enumeration; do not
preflight pruned descendants or bypass non-regular-input refusal. The selected
external excludes are already compiled once by policy discovery and are reused;
these new quotas cover local hierarchical parse work, not a new restriction on
trusted operator-selected global paths.

Default cumulative limits are 1,000,000 directory-context loads (aligned with
the default scan entry bound) and 128 MiB of local policy input admitted for
parsing **per matcher/pass**. Every reload after eviction is charged again.
Expose explicit limits for callers and deterministic tests. Bound retained child
contexts with soft limits of 256 contexts and a 4 MiB estimate covering local
policy inputs and retained native path/cache keys, including wrapper keys.
Evict between targets when a new context is needed and either soft threshold
would be exceeded; do not
reparse policy for repeated targets in an already cached directory. One target's
ancestor chain may temporarily exceed the soft cache threshold, but remains
subject to the cumulative limits. This estimates retained input/key cost rather
than promising exact allocator RSS. Root/ancestor seed input is separately
bounded by the cumulative parse quota and is not repeatedly recompiled.

Use an additive `PolicyMatchError` and typed policy resource/limit fields; do
not extend the existing public closed `FsError` or `IndexWorkResource` enums.
Preserve cancellation/deadline as the original `IndexWorkFailure`. At the CLI
document owner map policy quota exhaustion to the existing
`VerificationIncomplete` / `PolicyUnavailable` full-scope refusal, including
resource, limit and observed work. It must not masquerade as incremental
Entries/SourceBytes work that triggers a fresh full-scan retry. Staging and fresh
publication revalidation deliberately construct separate matchers and each has
its own pass quota; both retain the same cooperative whole-operation deadline.
No hard unresolved-row cap or partial successful publication is introduced.
A failed matcher pass is terminal, including a partial native parser failure;
retry requires a fresh matcher. Keep an internal, non-serialized typed policy
resource on quota verification refusals so the observed MCP watch owner also
propagates them before its general incomplete-verification refresh fallback.
Unmarked verification refusals retain that existing recovery. Prove both paths
through the actual owner dispatch with a real pending source change and assert
that the marker does not alter the serialized CLI/MCP envelope.

Protect reuse, eviction/reload charging, before-parse refusal, cancellation and
deadline with causal filesystem tests and low-limit tests through the real
document-resolution/probe owner. Prove a loaded pass is stable and a new pass
observes policy edits, final revalidation remains fresh, and refusals preserve
last-valid publication, project identity and approved purposes. Re-run the
bounded multi-page real CLI fixture and MCP positive/failure workflows. Existing
FIFO/pruning, native-prefix, aliases, local/global precedence and linked/no-Git
controls remain required. No claim of immunity to hostile replacement races or
uninterruptible kernel I/O is added.

## Risks / Trade-offs

Disabling require_git(false) everywhere breaks no-Git projects. Adding excludes at arbitrary priority breaks local negations. Re-reading config for each file or unbounded include expansion harms resource limits. Treating configured external paths as hostile breaks legitimate workflows. Use policy-owned cached bounded inputs, canonical config syntax, existing precedence, causal positive/negative controls and fresh measured costs. Dependency fan-out must be justified; no whole Git runtime is needed for value parsing.

## Migration Plan

Ordinary compatible refresh repairs unchanged old indexes under the existing atomic owner. Preserve root/project identity, all approved-purpose text/status, last-valid publication on failure, current-contract no-op and compatible rollback/re-upgrade. Coordinate the final contract and inventory union with #731/#738; #708 measures final combined source and #715 exercises exact RC packages on four platforms.

### Fresh bounded document-prefix admission

The separate security review found that cold paged document verification still
canonicalized every component of every missing target. The hierarchical matcher
quota did not charge those calls or the nearest-ancestor fallback. A bounded
512-link/64-directory CLI fixture reproduces a 2.600s cold summary; syscall
tracing records 1,300,153 readlink calls, because each canonicalization walks its
whole prefix. This is a shared admission mechanism, including cached projection,
uncached verification and fresh staged-publication checks.

Keep prefix admission fresh for each distinct target. Canonicalize the selected
root and compare it with the captured canonical root, then inspect normal path
components with no-follow metadata. Canonicalize symlink components and every
Windows reparse component immediately, refusing the first escape even if a later
component could reenter. Stop on the first missing prefix; preserve unsupported
and permission uncertainty. The missing-ancestor fallback uses the same fresh,
controlled walk. There is no cross-target allow-cache or new snapshot guarantee.

Charge each target-specific metadata and canonicalization call before execution
under the original control, cumulatively across every target and SQL page owned
by one DocumentResolutionIndex. The default four-million-probe allowance admits
one million flat missing targets at four probes per target; deeper targets spend
the same allowance on their extra component checks. A native probe is an
accounting unit, not a constant-time syscall or latency promise. Root capture
and policy discovery remain separately bounded setup; the existing matcher owns
its context and input-byte quotas. Staging and fresh publication revalidation
create separate indexes under the same operation cancellation/deadline.

Quota exhaustion retains existing VerificationIncomplete/PolicyUnavailable wire
shapes and a CLI-private, non-serialized typed work-owner marker. Both policy
quota and document-prefix quota are terminal for automatic incremental/observed
MCP fallback; neither can restart a fresh allowance. Failure preserves identity,
approved purposes and last-valid publication. No public core/filesystem error
enum, SQL/schema, graph identity, CLI command or MCP tool changes.

Causal coverage distinguishes expensive canonicalization counts from total native
probe accounting, and covers cumulative refusal across pages, retry prevention,
fresh alias/root retargets, outside-then-reentry, in-root aliases, missing and
regular suffixes, cancellation and native Windows junction/reparse behavior.
The existing no-hostile-replacement-race limitation remains; it does not justify
caching a positive admission across targets before a later policy read.

## Architecture Disposition

N/A: Existing filesystem selection and runtime publication owners remain authoritative. This repairs input interpretation and consistency within those owners without adding a service, data model or communication boundary.

## Dependencies / Cross-Issue Impact

#739 has no direct implementation blocker, belongs directly to #715 and blocks RC1. #738 retains its separate derived-document prefix guard; this issue restores the legitimate external policy inputs exercised by that positive workflow. #731's semantic contract is retained. #708/#715 require final combined source and exact installed selector coverage. Stable publication remains outside this goal.

## Open Questions

None.
