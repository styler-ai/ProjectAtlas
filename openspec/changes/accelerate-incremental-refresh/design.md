# Design

## Context

See [proposal.md](proposal.md) for scope. #708 is now a 0.5.1 must-have under #715 and covers initial indexing, full reindexing, symbol/graph rebuilding and refreshes. Stable 0.5.0 is the new performance baseline. The implementation remains in the existing runtime, filesystem, parsing and storage owners; the task checklist records remaining acceptance.

The current CLI enters `run_single_watch_refresh_controlled`, then `refresh_index_controlled`. MCP watch-once calls the same refresh owner without using its server's `SourceObservationRegistry`. An unchanged call walks admitted paths, hashes their contents, loads stored nodes and compares the two sets. It avoids parsing and a new generation when nothing changed. A changed one-shot call can stage a full publication and perform two further exact validation scans. Continuous watching already supplies `WatchChangeSet` to an incremental path.

The one-shot cost is therefore tied to all entries and source bytes, with sorting/map construction as well. The full scan pipeline stages complete index state, revalidates publication inputs and publishes atomically. CLI and MCP symbol rebuilds share `run_symbol_build_pipeline_controlled`. These are source findings, not latency measurements. Initial/full indexing and symbol rebuilding still need their own measured phase profiles. Historical #358 timings are fixture context, not a 0.5.0 baseline.

## Goals / Non-Goals

**Goals:** improve initial/full indexing and rebuilding as well as refresh, reuse reliable changes, minimize affected work, preserve complete results and choose bounded default budgets from required work.

**Non-Goals:** a generic change-tracking framework, a new crate/database/daemon by default, Git-only discovery, metadata-only freshness, or speed claims based on unoptimized binaries or disabled normal telemetry.

## Decisions

### Settle the architecture before expanding measurements

Use one validated operation plan in the existing shared indexing owners. Reuse their current no-op, incremental and full-work states rather than adding a parallel framework. Capture root identity, policy, publication generation and trustworthy observation together, then select the required work once.

| Work mode | Architectural guarantee |
| --- | --- |
| Unchanged current-contract source | One exact detection discovery and ordered source-only comparison; no full-node decode, parsing or publication. |
| Known changes | Read and parse changed sources; update the actual affected data and dependency closure, without restaging unrelated project state. |
| Fresh CLI without reliable history | One exact source-discovery/comparison phase, then affected work. Compare lightweight ordered source records with the stored hash cursor; avoid loading complete stored nodes or repeatedly rediscovering the same project. |
| Required initial/full/rebuild work | Capture required source evidence once for building, share valid bytes and parse facts across projections, and publish complete required state. Wider validation is retained only where the source/writer evidence needs it. |

The source comparison is a linear merge of ordered lightweight records; use existing bounded ordering/staging facilities rather than assuming directory traversal is already sorted or allocating another full-node map. Expand the affected set once through the existing dependency/invalidation owner. Keep source reads, compatible parser results and verification stamps reusable within the operation, tied to their exact source/policy/generation.

Keep SQL, prepared/batched operations and snapshot ownership in the database crate. Use only the columns needed for comparison and bound cursor/staging memory. Preserve a consistent captured publication across reads; release readers before slow work where the existing snapshot/guard design allows it so they do not unnecessarily pin WAL checkpoints. Traverse and parse outside the published database's write transaction; retain the existing short writer-validation and atomic commit boundary. Do not add a pool, parallel writers, new indexes or tuning pragmas without an owning workload reason.

The Rust mechanism is existing concrete operation types, closed work states and RAII publication/cancellation guards, with bounded iterators/batches. A second engine, generic strategy layer or independently sized thread pool has no current consumer. The principal risk is stale evidence authorizing publication; source/policy/generation validation and causal failure/concurrency tests protect that boundary.

This is the chosen starting architecture. Code investigation and these work guarantees drive the implementation. Use a small optimized 0.5.0 reference baseline and focused before/after checks to confirm the expected benefit, not an open-ended benchmark search for an architecture. Set the cold-CLI evidence/lifetime contract before code; do not hide an unresolved history gap behind a fast MCP result. Keep broader platform validation at integrated acceptance and reuse unchanged passing proof.

### Keep one refresh and publication owner

Recovery adapters derive their next operation from the same closed refusal reason
as the human diagnostic. A dependency-closure limit requires an explicit complete
scan; blindly returning watch-once can repeat the refusal. Full scope alone does
not require a scan: ordinary source, path and policy drift can be reconciled by
watch-once. Preserve exact project/worktree routing and existing payload schemas.
This repair belongs to task 2.4's retry and exact-fallback behavior; it changes no
publication, closure limit or accepted performance target.

CLI/MCP init, scan and symbol-build adapters keep their existing shared pipeline owners; one-shot and continuous watch keep the shared refresh owner. Improve their common filesystem, parsing, comparison and atomic-publication work rather than introducing separate fast implementations per adapter. Known changes describe additions, modifications, deletions and renames of any size. Include affected text, symbols, summaries, relations and configuration-dependent data; one changed file can affect many graph rows.

Reuse `WatchChangeSet`, the existing source observer, source verification and store transaction boundaries. Do not add parallel adapter implementations or a second source-of-truth index. Keep native project/worktree identity captured throughout the operation.

Cold observer admission skips cached-policy preparation when no verified epoch
exists under its reconciliation lock; exact source and policy verification still
runs. Compute the immutable achieved/accepted language-registry digests once per
process in their existing core owner. Their bytes remain identical to uncached
catalog hashing; content-rule validation can still hash explicit alternate input.
Project-specific scan/text settings and installed optional-parser selection remain
fresh inputs to every publication-contract comparison. No source, policy, root,
observer or publication check is removed, and no persisted cache or schema is added.

### Improve initial indexing and real rebuilds

Profile `run_scan_pipeline_controlled`, symbol rebuilding and their shared staging/publication path before selecting optimizations. Measure directory entries, source bytes, hashing, parser work, stored-source decoding, database statements/writes, graph invalidation, validation passes and peak memory. Use separate initial-index, full-scan and explicit symbol/graph-rebuild fixtures; verify that the required work actually ran.

Select the smallest measured improvement in the owning code: avoid redundant traversal/reading/decoding, use bounded streaming or ordered comparisons where appropriate, batch database work using existing transactions, and reuse valid source verification without losing its race protection. Inspect real SQLite behavior and query plans before proposing an index or storage change. Keep worker, memory, file and output bounds effective across the host.

Preserve selection, ignores, parser provenance, document partial coverage, source/summary/text/symbol/relation completeness and existing force/rebuild semantics. A required full rebuild cannot be replaced by an unrelated cached no-op. Exact cold/full work necessarily inspects the source evidence it needs; the target is efficient bounded work, not an unsupported constant-time promise. Initial/full/rebuild paths must meet improvement targets, not merely avoid a regression.

Full normalized-graph replacement clears dependency/export owner rows and source
occurrences before their parent keys, relations and entities. Reuse one private
storage helper in disposable staging reset, staged full publication, in-memory
full publication and the root-path replacement sentinel. Keep each caller's
existing identity checks and transaction/savepoint, and keep identity-rejection
handling separate. This avoids repeated foreign-key cascade work without changing
schema, read indexes, durability, sparse affected-path replacement or authored
state. Causal SQLite coverage checks remaining child work at parent deletion and
late-publication rollback; the installed product targets remain independent proof.

Full replacement captures the exact definitions of the existing non-unique graph
read indexes, drops them before retiring the old rows, and restores them after
the bulk write inside the same savepoint. Maintaining those indexes during mass
deletion only to drop them afterward adds measured VM and journal work. Unique
and primary-key constraints and foreign keys remain active. The normalized root
path sentinel routes to the existing full replacement owner; ordinary affected
paths retain their indexes and incremental behavior. Disposable staging already
drops these read indexes before loading graph rows. Causal SQL admission observes
the index lifecycle at row retirement; late source-constraint and cancellation
failures restore exact index definitions, prior graph/text/generation state and
reader snapshots. This changes neither the committed schema nor connection or
cache settings; optimized product and installed-platform measurements still own
the speed and resource claims.

### Reuse trustworthy observations; handle missing history honestly

For explicit one-shot refresh, both long-lived MCP and fresh CLI reconcile source exactly. The existing observer may retain operation-local source evidence for the writer, but native callback delivery cannot establish a zero-work freshness witness. A read stamp alone is not permission to publish.

A fresh CLI process cannot infer edits made before it started from an empty event queue. Without trustworthy retained evidence it performs exact initial discovery. Start observation before this baseline where the existing backend supports it, so later source changes can be checked during the same refresh. Lost events, unavailable observation or invalid identities require exact reconciliation. No unchanged result may be inferred solely from equal size/mtime or the absence of newly observed events.

This chooses the existing observer plus exact fallback, rather than introducing an unproved persistent service or silently relaxing freshness. If measurements show that cross-invocation evidence needs an additional component, publish and review that concrete design before adding it; the issue cannot close with only a fast MCP path and an unexplained slow CLI path.

MCP one-shot refresh uses the verified-read boundary with repair disabled and
forces exact source comparison on every explicit refresh. An unchanged current
contract avoids node decoding, parsing and publication, while still discovering
and hashing the selected source. A detected delta transfers its operation-owned
assessment to the existing mutable owner, which fences database identity,
generation, root and contract without repeating detection discovery. Caller
text-policy overrides use the normal refresh owner. Preserve worktree checks,
background task cancellation and final exact publication validation. Operation-local
evidence is reusable within this operation; a retained epoch cannot authorize
zero-source-work admission across requests.

The no-repair exact assessment borrows the immutable `before_plan` already owned
by the observed request instead of rebuilding the same scan plan inside that
assessment. A private shared runtime helper retains validated store/root,
publication and typed contract-mismatch checks. Native-observer fallback has no
such retained plan and builds a fresh one for each exact read. Incremental repair
keeps its existing owner. Independent after-admission and final acceptance plans,
policy witnesses, exact discovery/comparison and callback checks remain fresh.
The fresh after-plan/policy comparison guards both current results and stale-source
assessments before transfer to the refresh owner; changed policy drops the
assessment and retries within the original control. This removes duplicate operation-local setup, not a freshness check or source walk.

### Bind discovery to the policy inputs actually consumed

The endpoint witness alone is insufficient. A causal owning-admission test has
shown that configuration A at both endpoints can bracket a real walk using B,
which excludes an already saved new source. With callback delivery delayed, the
operation incorrectly reports the prior publication as current. The same test
confirms in-place configuration edits, restoration of the original configuration
file object, global excludes, repository `info/exclude`, and self-ignored root and
nested `.gitignore`/`.ignore` inputs. Stable-policy controls detect the new file;
the defective admission does not alter the prior valid publication. This finding
belongs to task 2.4 and must be resolved before its acceptance.

The filesystem owner must retain bounded evidence of the exact policy bytes it
hands to the canonical matchers, including directly parsed Git configuration and
lazy hierarchical ignore inputs. Selection evidence also includes the exact
environment/configuration source selection and the matcher's actual `.git` and
`commondir` input streams. For repository-derived exclusions, retain the exact
excluded-prefix vector produced by the existing validating worktree owner and
recompute it freshly at admission. That vector is the complete effect of the
registration inventory and `gitdir` pointers on scan inclusion; raw registration
bytes are not a second authority. A valid outside-root registration that leaves
the vector unchanged remains admissible, while an in-root registration changing
selection invalidates the evidence. Causal pointer/registration and equal-result
controls protect this distinction. Selected-root aliases
must retain their existing canonical-root and explicit-spelling behavior.
The source assessment must validate that
evidence at its admission boundary, in addition to the fresh endpoint policy,
native identity, callback and SQLite publication checks. A consumed-input mismatch
requires typed refusal or a fresh bounded retry; it must not transfer stale
discovery to publication. Evidence is operation-owned and transient, not a new
persistent cache or authority for zero-source-work admission. Existing file,
aggregate input, entry, worker, cancellation and deadline bounds remain effective.

The pinned canonical `ignore` API privately opens its global and hierarchical
inputs. It provides no policy-load observer or in-memory injection into the
parallel walker. Reusing only a compiled global policy leaves lazy local inputs
unbound. Endpoint file identity/timestamps also fail when the original object is
restored, and local node hashes cannot cover self-ignored inputs. Reimplementing
ignore precedence or adding a second full discovery is not an acceptable shortcut.
Before adopting a dependency patch, review an additive policy-reader/observation
seam that leaves the canonical parser, hierarchy and precedence in place. Its
default path must preserve upstream behavior; the Atlas reader must stream a
digest of the same bytes supplied for parsing, retain absence and errors, and expose
the consumed-input evidence through the scan result for fresh runtime validation.
Do not retain whole policy files or hold the evidence lock during input I/O. Bound
the distinct input inventory by the admitted traversal/registration limits,
charge streamed reads and validation to the original work control, and reuse a
consistent observation when a canonical matcher reuses its cached context.
Multiple inconsistent observations of the same selected input invalidate the
operation. Absence is evidence too, including candidates omitted by directory
entry optimizations. Preserve the canonical default missing-file, parse-error and
symlink behavior. The external policy witness's 16 MiB per-file cap must not be
silently imposed on nested ignore files; specify and test their streaming work
accounting under the existing scan resource/deadline controls before adoption.
Any retained upstream source must have exact provenance, license, a minimal
auditable patch and a removal condition when an equivalent official API exists.
The additive reader/evidence direction has passed design compatibility review.
Its concrete source patch, provenance/license, resource accounting and native
behavior remain subject to implementation review and proof.

Positive and causal controls must cover all confirmed input families, original
object restoration, local negation/precedence and selected-root aliases, persistent
policy changes, unchanged no-op work, correct reconciliation paths, native failure,
bounded reads and cancellation. Native installed CLI/MCP/watch acceptance and
performance targets remain mandatory. This scoped consumed-input contract does
not promise an atomic global filesystem snapshot.

The exact source assessment owns the source certificate and any additional
certificate consumed by nonindexed document resolution. The runtime retains
them through exact admission, the outer fresh endpoint checks, and final query
acceptance inside the existing callback-receiver fence. A mutation retains both
its initial admission and its fresh commit-verification certificate through the
writer's final acceptance. Exact fallback retains its first and post-query
certificates. None is stored in a cached observer epoch or used to authorize a
later request without fresh source work. A rejected final policy check invalidates
that operation's epoch. Causal restoration after inner verification, during the
query, and after fresh mutation verification protects these distinct boundaries;
stable policy remains a valid workflow.

Direct CLI snapshot admission verifies the retained certificate and a freshly
derived plan against the pinned root identity and publication immediately before
returning the store. Retargeting the original selected root alias must produce
the existing typed project mismatch even when both roots have equal scan options.
Federation holds every exact snapshot and certificate privately until
all participants have opened, then performs the same controlled checks for each
participant. It charges the extra identity/publication reads and admission time to that
participant's existing work report, preserves its alias on typed failure, and
closes all snapshots on rejection. The service still receives immutable admitted
SQLite snapshots; certificates do not cross into its types or survive through
the later aggregate query or `finish`. These are sequential per-root admission
checks, not a simultaneous cross-root filesystem snapshot. Positive stable-policy
admission and an independently checked active-reader cleanup oracle protect both
adapters, alongside late matcher and AtlasMap-plan transitions.

Mutable reconciliation preserves its existing database identity, root, contract
and generation fences. If captured policy has changed, the full owner discards
the captured source and discovers it afresh under the original work control;
the incremental attempt selects its normal full fallback. A stale reusable
assessment is not a reason to disable legitimate full reconciliation. Existing
document-only policy changes retain zero parser work and authored state while
publishing the updated graph once.

The whole `.git` directory substitution has also reproduced the defect using two
real initialized repositories. Separate causal controls reproduce linked-worktree
`.git` pointer and `commondir` transitions and registration of a newly added
checkout, whose folder is completely hidden during the walk. Registering an
already indexed folder instead requests full reconciliation; retain that safe
control rather than requiring a narrow changed-path list when the assessment
already requires a full scan. All selector controls must pass through the fixed
admission boundary before accepting the dependency/evidence implementation for RC1.

### Prove the standalone CLI approach before production changes

Task 2.1 settles the exact-discovery/cross-invocation evidence contract and a separate cold-CLI target using code analysis and a focused reference check. The selected initial implementation uses exact discovery plus lightweight ordered comparison and affected updates when no trusted history exists. Task 2.4 proves that CLI approach as well as the warm MCP path. A fast MCP result cannot substitute for a slow CLI result.

If exact cold discovery cannot meet that target, publish and review a concrete trustworthy cross-invocation evidence design before implementing that alternative. Cover how history is obtained and retained, root/identity validation, gaps, restart, fallback, ownership and any required process lifetime or deployment behavior. Do not silently introduce a service. Zero-traversal admission for either route is deferred to unscheduled #725; it is not an RC1 acceptance target.

Recovered source semantics remain live through identity/key admission, native
call/import binding and Cargo-context consumers. At both full and incremental
projection boundaries, consume that evidence before raw row materialization:
retain the same immutable raw graph allocation and parser ordinals through a
separate private ownership type, drop source-cache and package/context owners,
and preserve recovery counts. Existing pre-release peak-budget checks and spill
selection remain conservative; the additional raw-graph reference owner is also
accounted. This shortens coexistence rather than weakening proof, increasing
limits, copying the corpus or adding persistent state.

At the shared raw-graph loader, selected paths with staged `Parsed` or `Clear`
mutations need no previous raw graph. Derive their borrowed path set under the
existing control and path-inventory allowance, load only the remaining paths in
the existing bounded chunks, then apply staged mutations in their original order.
The temporary selection drops before graph loading. Unchanged graphs still receive
persisted-row validation and any required source recovery; incremental old-export
and affected-footprint reads remain at their existing owners. Fully replaced or
cleared raw rows are deliberately not decoded or validated merely to discard
them. This does not change validation of retained rows or promise recovery from
unrelated database corruption. Causal coverage uses malformed old rows to prove
the omission, preserves last-change-wins and unchanged import bindings, and checks
cancellation even when every selected graph is staged. Measure the affected full
and sparse paths before claiming latency or sampled-RSS acceptance.

Schema admission still inspects complete current table definitions, columns,
indexes and foreign keys on its actual connection and snapshot. Reuse only the
five fixed parameterized SQL preparations through rusqlite's existing bounded
connection-local statement cache; bind each table/index name and execute again.
Successful table validation instead streams names and definitions from one
ordered schema-catalog query, avoiding a separate catalog search for each table.
No schema facts, acceptance result, project identity or cross-connection witness
is cached, and no capacity, compatibility rule or connection profile changes.
Real SQLite authorizer counts prove bounded preparation across tables; comparison
with cache-disabled reads and DDL changes protects rebinding and fresh column,
index and foreign-key results. Measure optimized navigation and no-op latency/RSS
before retaining a performance claim.

Compare actual schema metadata one table at a time in the existing storage
validator, rather than retaining an owned copy of every inspected contract.
Rust retains one table's metadata from the ordered cursor rather than a separate
table-name inventory; the immutable expected contract remains derived from
authoritative DDL. SQLite may sort the selected names and definitions, so retain
this query change only after measuring latency and sampled RSS, without claiming
that the engine retains only one definition. Validate exact table names, complete definitions and columns first, then
foreign keys, required and compatible extension indexes, and extension objects.
Retain the released schema-8 named-column rules. The separate disposable-graph
schema-15 validator remains unchanged. Failed table or foreign-key checks may
capture the complete actual contract for the established bounded diagnostic;
successful admission does not. Actual SQL and native conversion errors remain
fresh on the selected connection and snapshot, with no acceptance-result cache,
schema, connection-profile or statement-cache-capacity change. Causal
capture counts, existing predecessor/drift/extension controls and fixed focused
shipping summary/warm measurements must establish this experiment's bounds,
compatibility and benefit before new installed acceptance is requested.

Schema catalogs distinguish SQLite's literal reserved `sqlite_` namespace from
ordinary user-object names. SQL `LIKE` treats an unescaped underscore as a
single-character wildcard; that assumption previously hid lookalike user tables,
views and triggers from admission and could classify a foreign database as fresh.
Use an escaped literal underscore in every owning catalog filter: fresh-state
detection, current streamed table validation, complete predecessor/diagnostic
table capture and extension-object validation. Compatibility and rollback
snapshot readers use the same literal namespace rule independently. This changes
no durable schema, version, connection profile, cached fact or supported extension
policy. Genuine engine statistics and compatible non-unique indexes remain valid.
The correction belongs to tasks 2.3/2.4 alongside the changed metadata admission
owner. Causal foreign/current/released/evolved-schema controls must prove typed
refusal without mutation, with real CLI/MCP refusal and positive navigation before
acceptance; preserve the registered performance targets on the corrected source.

### Apply the delta and avoid repeated whole-project work

Convert discovered or observed changes into the existing incremental path. Use source/dependency invalidation rules to expand the affected set only when necessary. Relevant compiler, ignore, parser or publication-policy changes may legitimately invalidate a wider scope.

Use source-only stored fields for comparison where feasible, and compare ordered source records in a linear pass instead of building unnecessary maps on an unchanged result. These are concrete candidates to measure, not permission to add a speculative index or cache.

The next implementation slice retains the exact discovery's ordered current nodes,
captured database instance identity, publication generation, root and contract as
operation-owned source evidence.
An admitted delta consumes that evidence: select changed nodes from it, query downstream
authored/derived state only for affected paths after the full ordered source-only
comparison, and move the complete current-node vector into the
existing graph/publication plan. Do not hash changed paths again merely to reconstruct
nodes already captured by discovery, or build a second whole-source lookup map.
Full reconciliation selected directly by that assessment also consumes its captured
inventory, rather than retaining it while discovering another complete copy. Recheck
the identity and generation at full staging admission; final exact validation still
rejects edits made after capture. An attempted incremental closure that has already
consumed its evidence may rediscover during full fallback.
Reject a changed database identity, generation, root or contract before reuse. Existing source reads
must still match the captured content hashes, and final exact publication validation
remains until an equivalent writer-safe witness is established. A delayed source edit
or concurrent publication must refuse without exposing a partial generation.

Text staging already holds hash-verified UTF-8 content. Built-in symbol parsing may
borrow that operation-owned plain-text content instead of reopening the file only when
its bytes preserve parser semantics, including BOMs and newlines; do not
clone the corpus or apply this shortcut to extracted PDF/DOCX text, skipped text,
incompatible parser policies or external parser-worker inputs. Keep bounded fallback
reads and existing final source validation. This is evidence reuse within the existing
owners, not a new cache or cross-invocation history claim.

Reuse a valid observed epoch and per-source verification across staging and publication where the existing safety contract permits it. Remove repeated exact scans only after proving their race/failure protection is provided by the retained observation and writer checks. When that proof is unavailable, keep the exact fallback. Optimize initial/full discovery separately where measurement identifies avoidable work.

### Select the adaptive budget once at admission

Automatic budget selection at the existing shared indexing work-control boundary uses reliable available estimates: required discovery, admitted file counts/bytes, known changes, affected data and parser costs. Cover initial/full/rebuild operations as well as refresh. Do not add an unbudgeted whole-tree walk just to calculate a budget. Cold operations include discovery cost; a verified warm no-op does not receive a rebuild estimate merely because the repository is large.

Keep #699's existing 1,800-second ceiling and clamp for larger representable requests. Version 0.5.1 changes only automatic admission when the caller omits a limit. If reliable counts/bytes/work estimates are unavailable, including a new cold initial scan, use the existing 1,800-second budget without a preflight walk. With reliable estimates, choose a bounded default using coefficients and a minimum fixed by task 2.1's measurements; the maximum stays 1,800 seconds.

An explicit limit up to the ceiling wins, even when it equals the former default of 1,800 seconds. Preserve zero-budget refusal, the larger-value clamp and fail-closed unrepresentable deadlines. Do not reset the clock or extend caller limits during retries. Update affected help, schemas, skill references and budget guidance with the automatic-default change before shipping it; retain the closed #699 artifacts as the historical 0.5.0 contract. Preserve cooperative checkpoints, cancellation and memory/file/output/worker bounds; do not claim a hard wall-clock guarantee.

### Measure before choosing further optimizations

Use the actual optimized installed 0.5.0 CLI/MCP as the baseline with normal telemetry. Architecture and deterministic work-count guarantees come first. Use a small representative reference set to fix practical latency/throughput and resource targets before the affected implementation; do not run exhaustive platform/fixture sweeps before a compiling behavior slice exists. Include large file counts and byte totals, dense graphs, supported parser/document coverage, initial/full/rebuild work, small and large deltas, warm/cold starts and caches, no Git, policy changes and required reconciliation in the final supported-platform acceptance. Keep private repository context out of public evidence.

Record distributions rather than one timing. Record visited entries, hashed bytes, parse candidates, decoded nodes, store statements/writes, generation changes, CPU and RSS where supported. Reuse `SourceVerificationWork` and existing harnesses; keep benchmark-only detail out of normal product output unless a real user contract needs it.

The release helpers and comparison harness must work with native Python 3.9 hosts.
Hash archive files, extracted executables and ZIP/TAR runtime streams using bounded
SHA-256 updates rather than the Python 3.11-only `hashlib.file_digest` helper.
Retain the preregistered archive identities, published-checksum checks, regular
archive-member checks and exact runtime binding. These untimed input checks do not
change fixtures, measured operations, telemetry, sample counts or performance targets.

The Python observer owns a short-lived, read-only SQLite connection for each
inspection and closes it on success or failure. A connection transaction context
alone does not close its handle. Backup/restore retains its transaction contexts
inside explicit close scopes, releasing both handles before invoking a runtime.
Preserve the backup API's committed WAL contents and the observer's `mode=ro`;
do not use immutable or read-write inspection to bypass a failure. Reports record
the SQLite library version. On inspection failure, retain the original exception
and report connect-versus-query stage plus database, parent, WAL and SHM file
state. Causal real-SQLite tests cover closure, read-only refusal, missing paths
and committed WAL backup. This repairs a confirmed lifecycle defect; it does not
establish the cause of the observed Apple Silicon baseline query failure, which
requires replay through the corrected native campaign.

The corrected campaign still failed during baseline inspection on Apple Silicon.
Capture owner IDs, the IDs used by access checks, access results and the optional
SQLite error name without replacing the original exception. Run the focused
reader compatibility tests on the existing native CI matrix before compilation:
cover a settled WAL database without sidecars and a live committed WAL, read-only
DML/DDL refusal and unchanged main-database/WAL contents. Coordination sidecar
creation is permitted by SQLite's read-only contract. These tests isolate the
host boundary; they do not waive the failed campaign or establish its cause.

Keep the settled and live WAL cases independent. Native SQLite may retain
sidecars after the last close, so closure alone cannot construct a sidecarless
fixture. For the exclusively owned settled fixture, require a successful
`wal_checkpoint(TRUNCATE)`, close its sole writer, and remove a retained WAL only
after verifying it is empty; then remove its coordination SHM. Verify that the
main file remains in WAL mode before exercising the unchanged read-only observer.
This is fixture construction, not observer cleanup or a production database
repair. The separate live case must retain and read committed WAL frames while
its writer remains open. See SQLite's [WAL lifecycle](https://sqlite.org/wal.html#the_wal_file)
and [checkpoint contract](https://sqlite.org/pragma.html#pragma_wal_checkpoint).

The corrected native fixture isolates a host-library limitation: Apple Python
3.9.6 / SQLite 3.43.2 cannot read the settled WAL database with absent sidecars,
despite a writable parent; its live-WAL case passes, as do all five cases under
the runner's Homebrew Python. ARM release isolation changes PATH before later
measurement setup, so rediscovering `python3` there selects a different interpreter
from the validated bootstrap. Capture its resolved executable before isolation
and use that absolute path to create the existing hash-locked measurement venv
afterward. Keep the complete HOME/PATH isolation and packaged-runtime proof.
Both performance lanes must run all five observer tests through their exact venv
under the final environment before timing either runtime, and log its Python and
SQLite versions. No interpreter-version pin, read-write/immutable observer or
performance exception is introduced. Python 3.9 syntax compatibility remains
distinct from this vendor SQLite behavior. The corrected native campaign still
must prove that the original benchmark failure is resolved.

Native toolchain admission retains the declared version, executable provenance,
failure status and existing install timeout. Failed Rustup installation must
report its exit status and bounded tails from both output streams; spawn and
timeout failures retain the original diagnostic and any partial output. Offline
controls cover those failure paths without executing a toolchain install. Their
hardlink fixtures use `os.link`, preserving the same inode evidence on Python 3.9.
This improves failure evidence; it does not identify the cause of an earlier
installation failure whose command output was discarded.

The retained Intel Mac failure subsequently showed the supposedly read-only
`rustup toolchain list` probe syncing and downloading the missing pinned toolchain
under its 20-second identity timeout. The following explicit install then met
partial-toolchain rollback conflicts; the log does not prove that every conflict
was caused solely by the timed-out probe. At the shared verifier subprocess
boundary, copy the caller environment and set `RUSTUP_AUTO_INSTALL=0`, as defined
by the [Rustup environment contract](https://rust-lang.github.io/rustup/environment-variables.html).
Older Rustup 1.27.1 ignores that environment setting. Its inventory is
non-installing, but `show active-toolchain` and proxy commands can install a
missing directory-selected toolchain. Before those commands, require exactly
one installed selection from Rustup's inventory, accepting its older `override`
and current `active` labels, and require that selection to match the repository
pin. A default row alone is insufficient: a missing directory override must
fail without downloading. Use the validated inventory selection directly and
run version probes with `rustup run <selected-toolchain> <tool> ...`, without
`--install`. Skip `show active-toolchain` and proxy execution: on the older
manager those can also install missing file-declared components in an existing
toolchain. Keep Rustup responsible for project selection rather than parsing
its override database ourselves; retain validation of every selected proxy's
exact backing before executing the manager.

The explicit install command, its arguments and 120-second budget remain intact.
Preserve caller homes, executable provenance, project selection, version and
commit checks, diagnostics and global environment. Offline boundary controls cover
unset, disabled and enabled caller settings, inventory label variants, ambiguous
or absent selections, and explicit installation. A real Rustup 1.27.1 smoke using
isolated homes and a loopback-only intentional-404 mirror reproduces the missing
directory-override download before the guard and confirms zero requests afterward;
healthy installed verification succeeds, missing-pin read-only verification
refuses without requests, and explicit bootstrap still requests the mirror. A
partial installation missing Clippy and rustfmt also reproduces unwanted
requests before the fix and refuses without requests afterward. No toolchain is
installed by that smoke. Native replay through the corrected workflow remains
required.

Cold external-policy coverage uses the existing `notify::Watcher::paths_mut`
batch API. Remove and re-add every selected parent, and retire removed parents,
before explicitly committing once. This preserves same-path replacement handling
while avoiding a full FSEvents stream restart per directory command on both Mac
architectures. Linux and Windows retain the canonical backend's per-path command
behavior. No source epoch, callback fence or policy sampling is skipped.

Check the operation budget before creating a batch, between native edits and
around commit. Commit even after an edit failure or cancellation so a deferred
backend is not left stopped. Keep the original typed operation error when both
editing and commit fail; any failure invalidates observer continuity. Failed
commit retains the bounded union of prior and desired registrations for retry
and teardown. Immediate/deferred backend controls prove command equivalence,
one commit, partial/commit failures, cancellation and coverage recovery; existing
native replacement and policy-race tests protect the real adapter. FSEvents'
existing internal stream-start error reporting is unchanged by the batch API.

Continuous-watch handoff proof uses the canonical `notify::Watcher` boundary
and an interval-wait closure in one private shared runner. Production supplies
`RecommendedWatcher` and `thread::sleep`; no runtime switch or reconciliation hook
is added. A controlled backend delivers a policy recheck after each fence's
snapshot, forcing exactly three uncertain attempts in both initial and later
refresh branches. It proves native watcher release before the same-control polling
owner, four total cycles, the bounded-native-refresh reason, pending work in the
first polling capture, subsequent policy/source edits, and unchanged project
identity and authored purpose state. Existing cancellation/deadline proof retains
the original clock and never renews a handoff budget.

Native Windows root-replacement proof exposed a separate continuous-watch failure:
the selected pathname can disappear between retiring the old directory and moving
its staged replacement into place. A controlled Linux backend reproduces the same
failure during initial reconciliation. Immediate policy-fence retries and later
plan/coverage setup can both encounter that gap. The source-inspection adapter
currently erases typed root absence into its public diagnostic. Preserve a private,
nonserialized selected-root absence witness through that adapter; do not parse
diagnostic strings or retry all source-inspection or `NotFound` errors.

The same typed cause must survive current project-identity admission, including
absence after plan reload and a temporary non-directory at the selected name.
Preserve the existing public diagnostic and strict project-mismatch behavior.
At filesystem and worktree root admission, consume `fs::metadata` through one
small private validator instead of `Path::is_dir`, which turns permission and
other metadata errors into `false`. Preserve those original I/O errors; reserve
the non-directory result for an existing non-directory and the explicitly
unsupported empty-root spelling. Test permission failure through that actual
metadata-result boundary, without depending on the test process's privileges.
Worktree canonicalization can retain the same selected-root absence as a typed
repository-boundary error. Admit that cause only when its native path is exactly
the selected root and its I/O kind is absence/non-directory; Git-control, child
and permission failures remain strict. Preserve the public boundary diagnostic.

The parallel walker can observe a root gap after the source capability has
already opened. Preserve the native I/O kind and innermost named path through
single walk-error context wrappers, retaining the original structured error.
Composite, non-I/O and pathless errors remain generic failures: they cannot
attest selected-root absence. Use the same adapter for Windows entry-metadata
errors. If the walk instead admits a non-directory at its depth-zero root,
reject it as a non-directory root before either platform's file-admission path.
A retained source capability must not turn that temporary file into an ordinary
descendant source. Real parallel-walker tests remove or temporarily replace the
root after admission and verify both causes and successful scanning after
restoration. Child-path and permission controls remain strict; no new retry
owner, filesystem polling or I/O is added to the successful walk.

Use one private closure-based continuous-watch reconciliation helper, bounded to
three attempts. Only absence/non-directory errors naming the already selected
canonical root or its retained configured spelling permit a wait and retry; the
operation's original control is checked before each attempt and around the
existing watch-interval wait. Initial and later native setup, policy fencing and
refresh, and initial/later polling refresh use that boundary. On native recovery,
retain policy uncertainty, require recursive rebinding and select full work even
if the native root callback is delayed. Typed native missing-root registration
failures retain the same recovery classification. Successful cycles alone count;
exhaustion returns the original failure. Persistent absence, cancellation and
deadline refusal preserve the last complete publication, identity and purposes.
No new store, schema, observer thread, cross-request evidence or public payload
field is added. One-shot CLI/MCP and unrelated I/O or identity failures retain
their existing strict behavior. The existing policy-overlap polling handoff and
preregistered performance/platform targets remain independent requirements.

The Linux real-CLI flood fixture separately proves native producer compatibility
with noise before startup and after a quiet initial capture. Sibling writes can
legitimately miss every bounded fence, so either notify or automatic polling is
valid; any polling outcome must retain the bounded-native-refresh reason. Actual
policy and source edits, four-cycle completion, document outcomes and authored
state remain required. Pause and join the noise producer between edit phases;
keep it active during the source/policy refresh under test. Increasing a corpus
or counting writes cannot force or prove callback/fence overlap.

The fixture identifies idle native receive and polling sleep through
`/proc/<pid>/syscall`, target ABI futex/sleep constants and inotify presence.
Native idle requires an untimed channel park, excluding timed debounce and worker
join waits. Kernel `wchan` symbols remain diagnostics. Retain repeated syscall
snapshots, exact post-stop equality, owned-child signal guards and cleanup.
Preserve the required phase, exit status, observed wait and child stdout/stderr on
failure. Authenticated hosted and installed-runtime probes distinguish the
original unavailable-symbol observer defect from the later incorrect assumption
that unrelated noise always forces handoff. No retry, deadline or performance
acceptance threshold changes. The Linux
[syscall](https://man7.org/linux/man-pages/man5/proc_pid_syscall.5.html) and
[wchan](https://man7.org/linux/man-pages/man5/proc_pid_wchan.5.html) interfaces
define the distinction between the actual call and its symbolic kernel location.

Choose the smallest causal checks for each implementation slice and a focused before/after comparison for its performance claim. Reuse existing fixtures and counters. Run the complete installed/platform matrix at release acceptance; repeat a passing check only for changed inputs, failures, an uncovered behavior or a mandatory gate. Measurement confirms the architecture; it does not replace the architectural work.

The RC1 work-count target for unchanged current-contract source is one exact detection discovery/comparison with no full-node decode, parse work or new publication. Zero-source-work admission is deferred to #725. Delta processing must avoid unrelated parsing/publication. Cold discovery, initial indexing, full reindexing, symbol rebuilding and recovery each have separate targets. Report each result independently so a warm-path gain cannot conceal another slow path. Normal cases must finish within their selected budgets; injected deadlines still prove safe refusal. A larger budget alone is never a performance result. Unaffected navigation and supported upgrades must not regress beyond preregistered noise/resource limits.

## Risks / Trade-offs

- Missing or coalesced events could hide an edit. Detect continuity loss and test overflow, offline edits, restart, races, same-size/same-mtime changes and exact fallback.
- A read-oriented observer could incorrectly authorize a write. Require writer-safe root/policy/generation validation and concurrent-reader/writer failure tests.
- A small delta could still rebuild the graph or decode every stored summary. Assert affected work and row/byte counts as well as elapsed time.
- Adaptive budgets could hide slow algorithms or violate caller intent. Test the estimator separately from speed and preserve explicit-limit precedence.
- Cold exact discovery still has a cost proportional to the evidence it must inspect. Report this honestly, measure it separately and do not claim constant-time discovery without a trusted change source.

## Migration Plan

No schema change or public route removal is required by this plan. Source admission adds the canonical `cap-std` and `cap-fs-ext` dependencies and the Windows `winx` backend in the filesystem owner. The licensed, repository-owned patch to canonical `ignore` records consumed policy inputs while preserving its matching semantics; dependency provenance and release notices remain part of the release gates. Introduce each proven improvement in its existing shared owner with causal tests and exact fallback. If storage or a persistent component becomes necessary, review its concrete compatibility, migration and rollback design before implementation. Reuse installed-upgrade and four-platform gates before #715 accepts 0.5.1. Preserve 0.5.0 user databases and authored state.

## Dependencies / Cross-Issue Impact

#708 is a required 0.5.1 child and blocker of #715. Remove its old 0.6.0 membership and #493 blocker relationship without changing #493's remaining children. It has no implementation prerequisite on #310, #314, #388 or the language work in #563. Those 0.6.0 features consume the accepted indexing baseline later.

#699 supplies the cooperative-budget and caller-limit contract. Update affected help, schemas and default-budget guidance together without weakening explicit caller control. #358 supplies historical context only; task 2.1 establishes the optimized 0.5.0 baseline and numerical targets. The release owner implements no performance fix; this plan does not publish 0.5.1.

The final combined candidate consumes accepted #731/#732/#739/#741/#738 source,
including derivation-26/semantic-23, bounded source recovery and external-policy
verification. Preserve its source/root/watch/publication guards while applying
#708 budgets and exact-admission optimizations. The scoped static-binding and
external-policy cost references do not waive any preregistered performance,
unaffected-navigation or supported-upgrade target. Measure required one-time
contract repair separately from current-contract reuse on the combined source.

Dense acceptance distinguishes actual source recovery from changed raw-graph
publication. Accepted #731 binding evidence is transient and tied to original
parser ordinals: the full dense closure recovers 255 unchanged import sources in
addition to the one changed source. Its oracle must account for all 256 parses,
exact changed raw output, unchanged authored state and full-scan convergence.
The earlier dense one-parse expectation is superseded for this contract; the
large sparse one-parse limit and all numeric latency/throughput/RSS limits remain.
Existing owning recovery tests protect ordinal correspondence, exact counts and
source hashes. No persistent parser-evidence cache is justified by this oracle
correction.

Resolution-key publication reuses validated witnesses within each prepared batch.
A bounded standard-library set borrows up to 1,024 complete canonical keys without
cloning their material. Equality includes the complete project/domain/digest/
canonical witness; every miss goes through the existing SQL collision guard.
Additional distinct keys use that ordinary path without growing the set or
refusing a valid publication. All export and dependency owner writes
still run. Cache lifetime follows one batch inside the existing transaction, with
no schema, query-plan, connection, WAL or publication-policy change. SQLite trigger
coverage proves reduced witness writes, exact owner rows and rejection of a corrupt
persisted witness; unchanged performance and sampled-RSS targets remain required.

## Open Questions

None.

## Notification-delivery finding during platform proof

The macOS ARM64 installed release test exposed delayed callback delivery after an
immediate saved edit. Notify 8.2.0 exposes no backend delivery fence; draining the
Rust receiver and checking its callback counter cannot prove the native event queue
is empty, on macOS or other asynchronous backends. One-shot refresh currently uses
exact no-repair admission and transfers its discovery to the existing writer. The user explicitly accepted shipping RC1 with exact reconciliation and measured
gains, deferring zero-source-work warm admission to unscheduled #725. No sleep or
Linux timing pass can replace the future causal proof. Task 2.4 retains exact
unchanged/change/failure acceptance rather than the deferred target.

The optimized native mutation test subsequently isolated a distinct namespace
failure: a target-database `Create(File)` callback arrived after exact admission
and immediately forced recursive rebinding. A callback kind describes a native
notification, not proof that the opened database was replaced. The ARM trace
establishes the delayed hint; it does not establish before/after inode equality.
A deterministic same-object callback reproduction protects the common mechanism.

Retain optional physical database identity at the existing storage boundary. On
supported Unix filesystems, retain the original SQLite-owned connection through
a lazy shared lease rather than opening a separate `File`. Independent descriptor
closes cancel the process's POSIX locks, including locks SQLite still believes it
holds. This affects both transient path-validation opens and final retirement of
a retained raw descriptor. Separate-process tests reproduce loss of WAL main-file
shared locks and rollback-journal reserved locks at capture, validation and final
witness drop. The ordinary WAL write/close visibility case did not fail on the
bundled SQLite; it is positive compatibility evidence, not a reproduced outcome.

Path checks use metadata only, following supported database aliases, and require
a valid lease plus the complete native device/inode key. Keep SQLite's
`SQLITE_FCNTL_HAS_MOVED` check against the actual opened connection: a replacement
can preserve project ID/generation, and a restored pathname can conceal a handle
still open on the replacement. Captured-key equality is not current admission
authority; all consumers use the live path/store checks. Unsupported native/VFS
identity, including the current Windows implementation, retains conservative
namespace handling. Other-platform connection ownership remains unchanged.

An internal Unix connection owner preserves the existing store access shape,
`Send`/non-`Sync` properties and exclusive mutation. Native identities share one
lazy `Arc<Mutex<...>>` lease per captured store. Store retirement first invalidates
the lease, rolls back any unfinished transaction, flushes the prepared-statement
cache and releases unused SQLite page-cache memory. Only successful cleanup may
transfer the idle original connection to the lease. Failed cleanup leaves the
witness invalid and closes through SQLite. This preserves implicit rollback and
prevents a retained identity from holding a read snapshot or uncommitted write.
A narrow take-once `ManuallyDrop` boundary transfers ownership during destruction;
it introduces neither a second path open nor an unsafe thread-safety assertion.
Final release remains SQLite/VFS-owned so another connection's locks survive.

Keep this witness outside the path-based registry key. Preserve the last accepted
file object across epoch invalidation and carry an exact-read candidate during
cold admission. Callbacks perform no filesystem/SQLite I/O: they queue exact
canonical-target creation hints. Controlled drain may ignore a hint only while
the live retained object and selected target still match. Selected-link creation,
removal, rename, rescan, unknown identity and continuity loss retain existing
refusal or exact repair. Reader/writer identity is checked during admission and
inside the final receiver fence, including exact mutation fallback. Source,
policy, root, generation, cancellation and rollback remain independent. This
adds no native delivery fence and does not permit zero-source-work freshness.

The registry remains bounded to sixteen entries; accepted entry identities share
one lease with operation witnesses. Older leases survive only while bounded live
operations retain them. Each lease can retain the original SQLite connection and
its idle WAL/SHM resources, so release page/statement caches and measure the
actual resource effect. There is no extra connection open, pool, thread, global
lifetime registry, persistent schema or dependency change. Clones share the same
retained connection, and last-clone drop coordinates its close through the VFS.
A renamed/unlinked database may remain pinned for that lifetime. The bundled
SQLite close path checks whether its main file moved before WAL cleanup;
replacement-WAL preservation is also covered directly.

Metadata-only database checks do not wait for a FIFO writer and preserve
follow-symlink alias semantics. Other source/control readers retain their shared
nonblocking regular-file opener. The original production raw-open search found
the two corrected identity paths; it did not establish the behavior of
dependency-owned path probes. Hydration candidate-file I/O occurs after its
private store is dropped and parent-directory opens are fsync operations. Coverage
includes all six separate-process lock cases, real-store retirement/clones,
authorizer-denied rollback, read/write transaction cleanup with a checkpoint
oracle, replacement WAL survival, FIFO, copied replacements via SQLite backup,
replacement-open/path-restore, database aliases and the existing source/policy/
continuity/cancellation controls. Checkpoint controls prove an active read blocks
cleanup and finished/rejected reads do not; idle identity connections are not
misclassified as leaked transactions. Installed native proof and every unchanged
four-platform performance target remain required.

Native Mac staging subsequently proves another manifestation at database-location
preflight: metadata and canonicalization preserve an active reader's kernel lock,
but `whichdisk::resolve(database_file)` loses it while SQLite still reports
SHARED. The plain-WAL sequence passes. The locked dependency is `whichdisk` 0.7.0;
its Apple observation opens and closes the selected regular file. Earlier
inspection of a cached 0.6.0 source did not describe this build. This affects new
store post-initialization checks, existing writable/read-only opens, public
location validation and read-snapshot admission while other connections are live.

At the existing SQLite location owner, use the workspace's locked `rustix`
pathname `statfs` on macOS. It returns mount point, mount source and filesystem
type together without opening a database descriptor. Preserve the exact canonical
probe, native pathname bytes, location comparison and local/network/unknown
classification; do not infer a file's volume from its parent, a device number or
lexical mount ancestry. Missing paths retain nearest-existing-parent inspection;
aliases and APFS firmlinks retain the exact selected filesystem. Decode bounded
kernel C strings completely, rejecting absent termination, empty values and an
invalid filesystem-type encoding. Windows and Linux resolution, including the
existing Btrfs fallback, remain unchanged. The direct locked `rustix` dependency
is Apple-target-only; no new package, unsafe application code, schema or VFS is
introduced. Native kernel-lock controls cover public/internal location checks and
store reopen/retirement. The diagnostic's direct unsafe dependency probe is
replaced by the public product validation boundary; its failing native original
is retained as causal evidence, not invoked inside a passing product workflow.

The cross-platform contract is preservation of every live SQLite connection's
locks. Descriptor-free inspection is required where an independent close can
release those locks. Linux's existing resolver uses `O_PATH`; the
[kernel close path](https://github.com/torvalds/linux/blob/v6.12/fs/open.c#L1516-L1533)
does not remove POSIX record locks for that capability, and the Unix
separate-process tests exercise public and internal location inspection on Linux
and macOS. Windows's resolver opens independent file objects while SQLite retains
its own locking handle. Microsoft's [file-handle model](https://learn.microsoft.com/en-us/windows/win32/fileio/file-handles),
[byte-range lock contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex)
and [filesystem behavior, sections 3.3 and 3.4.4](https://download.microsoft.com/download/4/3/8/43889780-8d45-4b2e-9d3a-c696a890309f/file%20system%20behavior%20overview.pdf)
support the inference that closing the resolver's independent handle preserves
SQLite's still-open handle's locks. This is platform-semantic and source evidence;
the native Windows profile/lifecycle tests provide ordinary compatibility proof,
not a Windows separate-process lock oracle. The exact-path, alias, filesystem
classification and invalid-parent requirements remain unchanged.

The remaining Apple dependency call belongs to managed-name case classification,
which queries a containing directory's volume capabilities. A case-aliased
database name beneath a regular-file `.projectatlas` parent could reach that
query before admission rejected the invalid parent. Require the resolved lookup
parent to be a directory before the capability query; do not open a regular-file
parent merely to classify its child's name. A separate-process DELETE-journal
writer proves that ordinary admission and reset refusals preserve an existing
SQLite reserved lock on that parent. Valid directory/case/Unicode and external
path behavior retain the existing native controls. This is a type check for
ordinary invalid inputs; it does not introduce an atomic namespace guarantee
against unsupported adversarial replacement of the lookup parent during probing.

## Windows drive-root admission cost

The Windows prefix guard materializes `whichdisk::list()` only to check one
drive letter. In the locked 0.7 dependency this also reads every volume's facts
and physical-device/removal metadata; ordinary read-only opens repeat the guard
at three existing validation boundaries. Query the selected drive root afresh
with canonical `windows-sys` bindings instead: `GetDriveTypeW` must identify local
fixed/removable/optical storage, and `GetVolumeNameForVolumeMountPointW` must return
a complete, bounded volume GUID root. Reject malformed successful output. These
queries borrow stack buffers and open no application-owned database handle; keep
the unsafe exception confined to that two-call helper with explicit buffer and
lifetime invariants.

[Microsoft's volume-root API contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumenameforvolumemountpointw)
does not guarantee ReFS support. On a failed native query retain the original
fresh inventory membership check, including its error/refusal behavior; do not
drop accepted ReFS locations or interpret an unsupported query as local evidence.
Keep the full exact-file resolver, filesystem classification, device/mount
coherence and every pre/post-open revalidation unchanged. UNC/device prefixes
remain refused before selected-path resolution; substituted directories remain
outside volume membership. No cache, full-resolver replacement, schema change,
new package/version or acceptance-threshold change is needed. Compare native
admission with the original inventory across drive letters, exercise aliases and
refusals, and measure guard cost separately. Only a fresh complete installed
campaign can establish whether either Windows regression is repaired.

## Regular-file races across source and control readers

The native database investigation exposed the same check/open assumption in source
hashing, source-policy hashing and Git pointer reads: earlier regular-file metadata
does not prove the object subsequently opened is still regular. Repair the owning
readers before consuming bytes. Policy witnesses and database
identity reuse the filesystem crate's followed-alias regular-file opener; each retains its own
error mapping, cancellation/read bounds and root/selection contract. Followed
policy and database aliases remain supported. Initial absent, directory and other
policy witness states retain their existing classification; a nonregular object
substituted after regular admission refuses conservatively. The existing Git
configuration reader already uses a nonblocking open plus descriptor validation;
retain that behavior and prevent controlling-terminal acquisition there too.

Git `.git`/`commondir` control pointers have a different direct-leaf contract.
Their private opener uses Unix nonblocking/no-follow flags or Windows
`FILE_FLAG_OPEN_REPARSE_POINT`, then rejects an indirect or nonregular opened
object before the existing bounded read. Keep the prior structural preflight,
byte limits, pointer parsing and typed errors. The shared followed-alias opener
must not relax this control-file rule. Deterministic fixtures substitute a FIFO or
symlink after retaining the original regular metadata; normal pointer restoration
and regular/aliased control reads remain positive controls. Native Windows
compilation and reparse-point execution remain separate platform proof; each
optimized native package lane runs the direct-pointer substitution/restoration
unit contract before accepting the packaged runner.

This scope covers the confirmed scanner, policy, database and structural-control
read boundaries. It adds no schema, watcher thread, cache, filesystem restriction
or cross-operation source handle cache. It does not promise preemptible ordinary filesystem
I/O or assert a complete audit of unrelated repository readers. Descriptor checks
add metadata work, which must be included in the unchanged installed performance
targets rather than waived as hardening overhead.

## Direct source discovery under namespace changes

Source symlinks are excluded by the existing scanner contract, including aliases
whose targets remain inside the selected root. Earlier metadata does not authorize
following a replacement leaf or parent. A private concrete `SourceRootReader`
anchors one directory capability to the already selected root for each discovery.
Borrow it across the existing walker workers. Traverse each native parent component
with `cap-fs-ext::DirExt::open_dir_nofollow`, open the leaf with
`FollowSymlinks::No`, and verify the opened object is regular before reading.
Use Unix nonblocking/no-controlling-terminal flags and the unchanged cooperative
controls before component opens and source chunks. Because Unix directory handles
allow the root pathname to move, compare the opened regular descriptor's device and
inode with the walker's admitted metadata before hashing. This refuses pairing
metadata from a replacement root with bytes from the retained selected root. The
node uses the opened descriptor's modification time and streamed byte count; final
source and publication revalidation remain necessary for concurrent writes. Drop all capabilities when
this discovery ends; `RootScanPolicy` retains no native handle between requests.

Use canonical `cap-std` and `cap-fs-ext` 4.0.3 rather than local Windows NT traversal
or a pre/post-canonicalization race check. The filesystem crate alone owns this
dependency and domain wrapper. Resource work is O(path components + source bytes),
with one root handle per discovery and bounded transient directory/file handles
per existing worker. A ten-request unchanged MCP syscall profile found repeated
parent opens raised filesystem work substantially compared with stable 0.5.0;
native Windows, Linux and Intel Mac measurements also failed unchanged navigation
or refresh targets. Reuse only the complete parent capability chain for sibling
files within each existing parallel walk visitor. On Unix, freshly compare every
direct parent's no-follow device/inode with its retained capability before a hit;
refuse replaced, indirect or uncertain ancestors. On Windows, retain every ancestor
capability with the canonical backend's no-delete sharing, preserving each name
until the cursor changes or ends. Fresh handle metadata must still reject an
in-place transition to a name-surrogate reparse point; write sharing allows an
empty directory to become a junction without a rename. Preserve ordinary Cloud
Files tags. Ordinary atomic file saves remain allowed.

Bound each cursor to eight descendant directories, all cursors to 64 retained
directory handles per process, and Unix occupancy to at most one eighth of the
captured soft descriptor limit. Acquire permits without waiting; deeper paths,
unavailable permits or unsupported platforms use the existing secure walk.
Close all directory handles before returning permits, including error, cancellation
and unwinding. The logical quota cannot know other owners' actual open handles:
recognized native handle-capacity errors during chain construction or a cached
source open discard the complete partial/retained chain before retrying the same
secure lower-handle walk once. Use Unix `EMFILE`/`ENFILE` and the existing Windows
SDK's too-many-open-files/no-system-resources codes; all identity, no-follow,
permission, path and control errors remain strict. If fallback fails, return its
failure. Prove both allocation and hot-cursor exhaustion under a real isolated
low descriptor limit, including positive bytes/fresh replacement and negative
stale-identity/same-inode-alias controls. A different parent discards the old chain; no chain survives its
discovery. Single-target scans remain uncached. Component checks, exact source
hashing, work counters and publication fences are unchanged; this adds no worker,
schema, persistent cache or cross-request source witness. Windows native proof
must cover in-discovery parent rename exclusion, ordinary file saves and rename
after cursor release. Actual optimized latency, syscall and resource measurements
must establish the benefit under real parallel-walker ordering; reuse alone does
not satisfy the unchanged targets.

Keep the capability chain but avoid a general path resolver at its already
admitted leaf. On supported Unix hosts, the leaf owner first requires exactly
one native normal component, then uses the existing locked `nix` crate's safe
`openat` with no-follow, nonblocking, no-controlling-terminal and close-on-exec
flags. Its owned descriptor becomes a `File`; the existing opened regular-file,
device/inode, exact-byte and publication checks remain authoritative. Parent
admission and fresh ancestor identity validation are unchanged. This uses no
ambient path, raw descriptor ownership or application unsafe code. Causal tests
retain ordinary saves, native names and hardlinks, reject same-object aliases,
FIFOs and directories, and check descriptor flags and the single-child boundary.

Fresh ancestor identity validation remains required before each cached source
open. A same-directory-object alias control isolates no-follow enforcement from
identity comparison: the alias resolves to the exact retained directory and file
objects. A different-directory/same-leaf control separately isolates identity
comparison. Both inside/outside targets and ancestor levels, ordinary recovery,
root swaps, native names, FIFO and handle pressure remain covered.

On Linux, evaluate the existing locked `nix` crate's safe `openat2` wrapper
before extending the userspace cursor. The [Linux kernel interface contract](https://man7.org/linux/man-pages/man2/openat2.2.html) distinguishes all-component resolution restrictions from leaf-only `O_NOFOLLOW`. A root-relative path made only of native
normal components can be resolved with `RESOLVE_BENEATH` and
`RESOLVE_NO_SYMLINKS` in one kernel operation. Keep the selected root capability,
read-only/nonblocking/no-controlling-terminal/close-on-exec/no-follow flags,
opened regular-file and device/inode checks, exact source bytes, work budgets
and publication fences. Do not prohibit ordinary mount points. This route holds
no ancestor cache: the kernel freshly authorizes every component for that open,
so the cursor's stale-handle identity check is unnecessary on that route.
Ordinary direct-directory replacement can therefore succeed when the current
source object matches admission; stale opened bytes and descendant aliases
remain forbidden. The existing single-target component walk remains unchanged. Explicitly select
that component route in `uncached()`; availability of kernel resolution is
independent of the retained-directory quota, including when that quota is zero.
Each worker positively probes the optional syscall once against `.` relative
to the already-open root, with the same resolve policy and
`O_PATH | O_DIRECTORY | O_CLOEXEC`. Drop this one transient descriptor before
opening a source. Success selects kernel resolution. Only `ENOSYS`, `EPERM`,
`EACCES` or `ENOENT` at this content-free probe selects the existing secure
component cursor for the rest of that discovery; do not infer availability from
an expected error that a syscall filter could synthesize. Other probe errors
are refusals. Once admitted, real-source permission, alias, path and race errors
stay strict; a real-path `ENOSYS` may still select the secure fallback. This
preserves syscall-filtered hosts without confusing source-specific permission
denial with backend unavailability. Keep only a closed, per-worker
unchecked/kernel/component state, with no global source witness. Prove selection
and fallback before implementation acceptance. All other alias, identity, path,
I/O and control failures remain strict. Require causal all-component no-follow
and source-identity controls, supported native paths/hardlinks/saves, root swaps,
nonregular files and cancellation, followed by the same shipping-profile
four-CPU comparison and installed-platform targets. No new dependency, unsafe
code, worker, schema or persisted state is needed. This is an experiment until
the measurements establish a benefit; other platforms retain the verified
component opener unless their native all-component contract is separately proved.

On macOS, evaluate root-relative `openat` with `O_NOFOLLOW_ANY` from the locked
`nix` crate's libc constants. Apple introduced this all-component restriction in
Darwin 20; older kernels may ignore unknown open flags, so a successful open or
`F_GETFL` cannot establish support. Select it only after the safe `uname` API
positively identifies Darwin with release major at least 20; unknown, malformed,
older or unavailable identification retains the secure component route. Enable
the existing nix dependency's `feature` feature for that API. Replace leaf-only
`O_NOFOLLOW` with `O_NOFOLLOW_ANY`: Apple rejects their combination. Preserve
all other source flags, native normal descendant components, selected root
capability, opened-file device/inode/regular-file validation, cooperative control
and exact publication checks. Do not interpret a real source permission, alias,
missing-path or I/O failure as backend unavailability. The explicit uncached
single-target route stays unchanged. Reuse native kernel-backend compatibility
controls on both Mac architectures, including same-object leaf and ancestor
aliases, supported native names, selected root aliases, hardlinks, atomic saves,
root replacement, FIFO and stopped work; separately prove conservative version
selection. Native shipping-profile timings must establish a benefit before this
experiment is accepted; Linux and other platforms retain their existing paths.

Kernel resolution checks cooperative control before and after the complete
path-resolution syscall; the component backend checks between ancestor opens.
A deep path or slow mounted filesystem can therefore defer stop observation
across that one call. As with other filesystem operations this is not a hard
wall-clock deadline. The post-open check must discard the returned descriptor
and refuse before hashing or publication if work stopped during resolution.
Prove this with a real owned-descriptor return from the source-open seam, not
only pre-cancelled input or a stopped availability probe.

For a held Windows ancestor, query fresh attributes with the already locked
canonical `winx` backend. An ordinary non-reparse directory needs no full
`cap-std` metadata construction or duplicate identity query. Every reparse point
still receives the original full `dir_metadata` classification: this preserves
Cloud Files and rejects in-place junction conversion without interpreting tags
locally. Errors remain strict; no-delete sharing, leaf admission and all resource
bounds remain unchanged. Repeat the real native junction and Cloud positive
controls and the installed latency/RSS measurements before accepting the change.

The upstream `fs-set-times` dependency retains `io-lifetimes` 2 while
`cap-*` uses 3. On Windows, the canonical `winx` backend retains the 0.52
Windows binding/import-library line while the existing application and watcher
use 0.61/0.60. Duplicate allowances name only those exact upstream splits and
are removed when the canonical dependency ranges converge; advisory, license
and registry-source gates remain unchanged.

Targeted scans first rebase the explicitly selected root spelling through the
existing policy helper, shared by exclusion checks and targeted scans, then preserve native component semantics: Unix inspects unresolved source
components before lexical normalization; Windows uses its standard native absolute
normalization on non-verbatim absolute input before rebasing. Canonicalizing a parent first hides an inside-root symlink. Do not
resolve child aliases into admitted direct paths. Ordinary relative paths, the
selected root alias, in-root parent traversal and direct atomic-save replacement
remain supported. Symlink leaves/parents, including inside-root targets, are
excluded; a substitution after admission refuses before hashing redirected bytes.

Windows directory capabilities prevent rename/delete while held by discovery;
none may survive into idle MCP operation. Ordinary source file handles retain
normal sharing so editor replacement remains supported. The canonical backend
checks native symlink types rather than blanket-rejecting every reparse tag;
Cloud Files placeholders must retain their normal read/hydration behavior.
Native root-move, file replacement and reparse compatibility need platform proof;
static backend inspection alone is not a claim that hydration was executed.
The Windows-only Cloud Files fixture registers one owned temporary root through
Microsoft's official `windows-sys` bindings, creates two online-only placeholders,
proves ordinary followed-open hydration on one and real scanner hydration plus a
second hydrated read on the other. A partial directory callback supplies a nested
online source so the same real scan also proves directory population and
no-follow parent reads. The parent owns and serves the provider; a separate
ordinary client process performs both scans and writes an exact completion marker.
Parent-owned callback counters and hydrated-byte checks prove that the client
caused actual hydration and population. The marker proves client completion only;
the parent explicitly disconnects/unregisters after success, failure or timeout
and reports cleanup errors. A bounded client process contains missing-callback
read waits. The registered callback table and its functions have process lifetime,
remaining valid through disconnect without assuming the native API copies them.
Native failure
or missing prerequisites fail this proof rather than silently skipping it. No
account, network, external sync root or production FFI is involved. The native
optimized filesystem suite runs in each existing package lane. API contracts:
[Cloud Files](https://learn.microsoft.com/en-us/windows/win32/cfapi/build-a-cloud-file-sync-engine),
[registration](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfregistersyncroot)
and [transfer](https://learn.microsoft.com/en-us/windows/win32/api/cfapi/nf-cfapi-cfexecute).
This fixture is not evidence until the actual Windows run passes.

Causal proof includes leaf/parent replacement after retained metadata crossed with
inside/outside targets and a targeted same-root ancestor alias. Positive controls
cover selected-root aliases, hardlinks and native path spelling. The private
reader proves raw non-UTF-8 components only where native creation accepts them;
[APFS requires valid UTF-8 filenames](https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html).
The fixture recognizes only native EILSEQ during creation, after a successful
Unicode read; it never masks errors from the reader. Product path normalization
continues its existing non-UTF-8 refusal. Other controls cover
replacement after an open descriptor, fresh-discovery replacement, absence and
resource/cancellation bounds. On Unix, regular atomic replacement between metadata
and open is an intentional transient refusal; a fresh-metadata retry succeeds.
The runtime propagates this inspection failure before affected-work publication
instead of mutating the prior valid index. These handles
prevent redirected reads through the tested source components; they do not claim
an atomic global filesystem snapshot. Existing source comparison, policy, native
root identity and transactional publication checks remain required.

## Concrete automatic-budget policy

The coefficients, unknown-work admission, explicit-limit precedence and same-clock
fallback rules are specified once in
[the indexing reference](../../../docs/benchmarks/v0.5.1-indexing.md#automatic-budget-coefficients).
Use a private concrete runtime policy over already-required node inventories and
verification counters. Carry the selected duration with the existing publication
batch so final validation and publication retain the same bound; no shared mutable
deadline, extra source walk, public schema or new core API is necessary.

Incremental graph staging retains the original bounded control until the existing
projection owner has measured old persisted work, new staged graph bytes and
affected-path identities. Its selected child then applies to summaries, final
validation and publication using the same start time. Unknown/spilled or removed
work keeps the ceiling; no sizing query or source traversal is added.

The existing release workflow has an explicit opt-in prepublication comparison
across its four native package lanes. Optimized development packages may retain
the current 0.5.0 workspace label so child performance proof can precede #715's RC
freeze; artifact/source identities distinguish them from published 0.5.0. The
preregistered input catalog, targets and compact Linux draft evidence are linked
from the indexing reference. Pure arithmetic/completeness tests run normally;
full measurements remain manual and do not establish RC publication acceptance.

Warm MCP comparison retains two isolated client processes and fixtures before
the first call. Use ten serial paired rounds with alternating order: round zero
is baseline then candidate with one discarded warm-up each; rounds one through
nine alternate candidate/baseline and baseline/candidate. Retain exactly nine
timed calls per runtime, their round and position, and a separate actual-call
order audit including warm-ups. Preserve success and unchanged-publication checks
after every call. Standard-library `ExitStack` closes both clients on success,
partial construction, request failure or state refusal; both must close before
either offline same-size/same-mtime edit and CLI/content control. This changes
measurement order, without changing product behavior, resource targets or sample
counts. The prior fixed-order warm blocks confounded runtime with between-block
host conditions; retain their observed failures and require corrected-protocol
proof before acceptance, without attributing those failures to a product cause.

Native packaging retains independent optimized shipping and contract target trees
and the shipping-runtime digest guard. Its 60-minute job budget accounts for both
builds: the Intel Mac shipping build consumed 16 minutes before contract compilation
exhausted the previous 30-minute combined budget. This construction budget changes
neither indexing deadlines nor any installed performance acceptance threshold.

For Windows regular files, the parallel walker reuses the locked `ignore` crate's
native directory-entry metadata only for preliminary admission. The existing
ambient path lookup also existed in 0.5.0; it is now redundant with the secured
reader's authoritative opened-handle metadata. This removes one path lookup per
regular file without treating directory-entry size, timestamp or type as final
source evidence. Every source still passes the anchored no-follow leaf open,
opened regular-file check, exact byte hash and publication fences. Missing or
substituted leaves fail at that open. Unix keeps its fresh device/inode admission;
directory and targeted-path handling remain unchanged. No entry metadata survives
the discovery or becomes a content cache. Native producer tests cover ordinary
saves and stale-entry deletion/alias substitution; native installed measurements
must establish the actual latency effect before accepting this optimization.


## Policy directory canonicalization within a selection pass

Test-only native-call attribution on the actual repeated current-read owner
shows two fresh discovery passes per refresh. In each pass, 27 selected-binding
resolutions remain distinct and fresh; ordinary directory ancestors perform 81
canonicalizations for 10 exact native paths. These diagnostic counts identify
eligible repeated work, not shipping performance acceptance.

Use one small standard-library directory resolver while constructing one
`PolicyWatchInputs::discover` namespace. Reuse only successful canonical results
for ancestors freshly classified as ordinary directories. Keep selected-input
and alias-control bindings, alias-parent and alias-directory canonicalization,
metadata/readlink classification, policy bytes, configuration/Git discovery and
control checks fresh. Key by `OsString`, preserving exact native selector bytes
or units; `PathBuf` equality can normalize separators or a trailing slash.

Bound scratch to 64 entries and 64 KiB of total encoded key/value lengths, plus
bounded container overhead. A full or byte-limited memo performs the ordinary
resolution without caching or refusing the input. Never cache failures. Drop
scratch after namespace construction, before fresh policy-content hashing;
it is absent from the immutable selection, equality, observer and database.
Every discovery creates new scratch, including the mandatory post-install pass.
Do not reuse it for event drains, `bindings_changed`, source-policy witnesses,
other selected-root spellings or later MCP calls. Same-path directory watch
rebinds and all before/after acceptance fences remain unchanged.

Causal proof must distinguish actual native ordinary-directory calls from the
uncached binding/alias calls. Compare stable selections against zero-budget
resolution, exercise entry/byte-cap fallback, raw selector distinctions,
absence-to-presence and errors, and prove fresh root/external alias retargets
and policy bytes across independent discoveries and registration. Shipping
reference/shallow/deep timing and memory results, followed by the registered
installed matrix, determine whether this bounded reuse is retained.

## Native current-read attribution

Native CI retains the existing debug/all-feature stage profile and runs the same
bounded current-read test with default features and the ordinary release profile.
Place the optimized diagnostic after document resource proof so its build reuses
the already-required shipping release dependencies. Retain separate native log
artifacts for both profiles; preserve the exact test and its source, policy,
snapshot, publication and observer assertions.

The test warms one registry before five repeated admissions on its 1,024 generated
one-function sources plus the observer helper's source fixture. It attributes
inclusive owner durations and work counts;
nested spans are not additive. It does not time MCP transport, use the installed
matrix's twelve-function fixture, measure RSS, or compare the packaged 0.5.0
baseline. An optimized owner profile can localize a remaining native latency
cost, but cannot establish a product speedup, explain an RSS miss, replace the
registered installed measurements or waive their limits. Keep previous failed
cohorts intact while investigating their owning paths.

## Windows policy-directory attribute trial

The optional fresh `GetFileAttributesW` branch was tested and removed. Its native
paired policy fixture measured full-metadata versus attribute medians of 6.2003
and 6.1113 ms (1.44% less), with only three of five pairs faster. Full-metadata
calls fell from 158 to 60, but 130 attribute probes raised total measured lookup
API invocations to 190. These counters do not measure physical I/O. The small,
noisy timing difference does not justify the added selector/reparse/fallback
surface or explain the installed Windows warm-MCP miss. Keep the established
full-metadata path and the separately justified bounded directory resolver memo.
The original diagnostic remains in the archived source-CI run and implementation handoff;
no target or failure is discarded. Continue investigating the actual owning work.

## Supplemental native navigation memory attribution

After the explicit installed campaign, the Apple Silicon job may run one bounded
resource diagnostic on that same campaign's completed navigation fixtures and
exact executable hashes. The route remains manual, prepublication-only and
opt-in, with separate output and artifacts; it runs after the original report is
saved even when a registered target fails. Source revision, architecture and
workflow-run identity must match. Missing or incomplete originals fail closed.

Use the native `/usr/bin/time` program high-water counter: Darwin's current
maximum resident set size is bytes; GNU `%M` is KiB and is converted to bytes for
local method verification. Preserve its raw resource output and a tiny native
floor control. Run one discarded warmup each and nine alternating serial pairs
of the original detailed-navigation command, with normal telemetry and C locale
for stable resource labels. Use one cooperative 180-second helper budget within
the five-minute workflow step; each owned process group gets at most 120 seconds of its remaining budget. Kill and
reap the owned group before reporting timeout, reserving workflow cleanup margin.
Retain partial attempts and failures. Keep capture files outside the original
campaign and check that original publication, authored state and report bytes
remain unchanged after every observation.

These supplemental counters have different accounting from the registered
5 ms sampled process-tree RSS. They do not replace its samples, reevaluate its
targets, establish an inequality between counters, or waive an original failure.
Native timing is diagnostic only. Linux execution verifies the helper's method;
Apple Silicon attribution requires actual Apple Silicon execution.

## Selected output encoding and shipping code generation

Detailed CLI relation fitting previously built TOON even for JSON, then discarded
it at the existing serializer. Guard that encoder at both the single-project and
federated callbacks with the closed `OutputFormat::Toon` variant, matching the
legacy route. Retain the JSON value, fitting owner, output accounting and schemas.
No new strategy, abstraction or encoding dependency is necessary. Actual CLI
breakpoints prove removal of three discarded encoder calls in the selected JSON
case; positive JSON/TOON single/federated output checks protect compatibility.

The remaining detailed-navigation resource misses concern process memory, beyond
SQLite statement allocations alone. A causal statement-cache flush removed
compiled statements but reduced sampled RSS by only 4 KiB and increased median
latency by 7.29%; it was removed. A shipping ThinLTO trial increased ELF `.text`
by 3.38% and was rejected before runtime measurements. Neither trial changes the
retained schema, statement-cache behavior or default LTO setting.

Use one actual workspace release profile, `codegen-units = 1`, for ordinary
shipping builds and all native packages. This is source configuration rather than
a benchmark-only profile or environment override. The local shipping build
reduced `.text` by 8.50%; the scoped paired screen reduced detailed maximum sampled
RSS by 15.85%, with summary latency increasing 3.61% and compatible-open latency
decreasing 2.28%. These observations support native validation, not acceptance
against published 0.5.0 or a claim that executable pages alone explain every RSS
sample. Native build time, all indexing paths, positive CLI/MCP workflows,
features, compatibility and resource limits remain binding. Keep the existing
60-minute package budget and original registered matrix unchanged. Release DB
and CLI unit checks, actual executable wire compatibility, mandatory push gates
and independent review precede the new native candidate campaign.

## Native PHP regression construction boundaries

The Intel Mac source job exhausted the combined 20-minute PHP step while
compiling its second test target. The symbol-library target took 9m02s to build
and its PHP assertion passed in 0.66s; the following CLI unit target was still
compiling dependencies when the shared step expired. These were unoptimized
test-profile builds, so this log does not establish a release-codegen runtime
regression. The full Mac route has no preceding target-compilation step.

Give the symbol-library, CLI graph-projection and CLI/MCP navigation targets
separate native steps, each retaining the existing 20-minute bound. Keep their
commands, order, compile condition, features, ignored-test admission and assertion
filters unchanged across all four platforms. Cargo reuses the same target tree
between steps; the later native source-policy and document controls retain their
existing budgets and dependencies. This separates three cold construction and
test boundaries without weakening a product deadline or a registered performance
limit. Preserve the failed original and require corrected native source proof;
the shipping profile, package construction budget and installed matrix are
unchanged.

## Selected CLI serialization boundary

The remaining eager renderers passed an already-built TOON string to a formatter
that discarded it for JSON. Defer that work with a native `FnOnce() -> String`
at the existing CLI serializer: the closed output-format match invokes only the
selected encoder. Apply the same boundary to ordinary commands, tracked source
and candidate-set output, legacy/detailed relations, coverage stabilization and
slice fitting. Keep the existing wire envelopes, newline behavior, final byte
bounds and telemetry baselines. Return the selected owned string directly;
there is no renderer registry, new dependency, dynamic dispatch or cache.

Directory-walk telemetry consumes the selected serialized output, including the
already stabilized coverage response, rather than encoding it again. Preserve
the distinct directory and candidate-set accounting policies. MCP's intentional
TOON envelopes and cooperative analysis encoders already select their format;
they retain their existing bounds and cancellation behavior. No source, policy,
database identity, reload or publication safeguard changes.

The initial watch-only Linux screen used two frozen ordinary shipping binaries
against one unchanged 1,024-source reference fixture with normal telemetry. One
warmup pair and 31 alternating measured pairs produced medians of 45.724523 and
44.422684 ms; all 203 controls passed, with byte-identical JSON and TOON and
unchanged source, publication and approved purposes. Sampled maximum RSS rose
from 20,140,032 to 20,484,096 bytes. These local results justify investigating the
shared mechanism, not a claim about the Apple Silicon miss or final all-route
performance. Require independent source review, causal encoder-selection tests,
positive real CLI formatting/budget/telemetry proof and the unchanged complete
native installed matrix before acceptance. Preserve the C1 cohort's sole
cold-no-op failure and every older failed cohort.

The subsequent complete shared-serializer screen recorded median watch latency
45.043716 to 45.763497 ms (+1.60%) and maximum sampled RSS from 19,656,704 to
18,866,176 bytes (-4.02%). It does not establish a watch speedup. Preserve that
screen as failed because its legacy positive control requested ten rows from a
twelve-row fixture and the original binary correctly refused. Retain all 62
timing samples and twenty completed wire comparisons. An independently reviewed
wire-only continuation verifies the same byte-exact refusal in the candidate,
uses twenty rows for the positive legacy read and finishes detailed/coverage
checks without repeating timings. All 26 positive JSON/TOON comparisons across
thirteen read commands preserve exact output and state. Native numerical
acceptance remains required, and the failed Apple Silicon cohort is unwaived.
