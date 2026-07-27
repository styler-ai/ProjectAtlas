## ADDED Requirements

### Requirement: Parser diagnostics fail closed
The optional-parser supervisor SHALL reject any response attempt that emits post-admission diagnostic bytes, even when a syntactically valid completion frame follows, and SHALL terminate, drain, join, and reap the resident before returning typed failure.

#### Scenario: Bounded stderr precedes valid completion
- **WHEN** an admitted parser request emits bounded stderr and then a valid completion frame
- **THEN** the supervisor rejects the response, destroys the resident session, and publishes no parser result

### Requirement: Launch authority is current within one pre-READY epoch
Before every parse, the supervisor SHALL revalidate the fixed-role active manifest, worker, platform authority, accepted manifest, and selected grammar before new launch or resident reuse. Every digest read SHALL require the opened read handle to match the previously captured file epoch before accepting bytes. Filesystem uncertainty SHALL destroy resident authority without accepting stale bytes; observed drift SHALL destroy the resident and use the existing bounded digest reload with exact artifact-identity enforcement. The caller SHALL retain one unchanged no-progress epoch from parse admission through currentness probing, reload, Linux sealing, bounded process creation, platform admission, `SessionOpen`, and identity-validated READY.

#### Scenario: Same-size and same-mtime launch input changes
- **WHEN** a fixed-role launch input is replaced without changing its size or displayed modification time
- **THEN** the production epoch and read-handle identity checks detect drift before accepting bytes, launch, or resident reuse and require exact bounded reload

#### Scenario: Currentness cannot be proven
- **WHEN** currentness probing is blocked, canceled, timed out, or exceeds the unchanged no-progress epoch
- **THEN** the caller returns typed failure within its bound and any resident is terminated and reaped rather than reused

#### Scenario: Several pre-READY phases cumulatively exhaust the epoch
- **WHEN** individually sub-timeout currentness, reload, sealing, creation, admission, or handshake phases cumulatively exceed the caller's original no-progress epoch
- **THEN** launch fails before identity-validated READY without resetting that epoch between phases

### Requirement: Spawn handoff has one bounded ownership commitment
Process creation SHALL run behind one process-wide capacity-one owner that retains the unadmitted child and spawn lease. After actual process creation, the owner SHALL report readiness and offer an owner-retained zero-capacity rendezvous without transferring ownership. The caller's successful final cancellation, deadline, and no-progress check SHALL be the ownership linearization point; a bounded acknowledgement SHALL report that committed decision before the owner transfers the child. A stop before commitment SHALL only detach the caller while the owner kills and reaps the untransferred child before lease reuse. A later stop SHALL use ordinary caller or resident cleanup. Late cleanup uncertainty SHALL leave future launches fail closed.

#### Scenario: Caller stops before the final check commits
- **WHEN** cancellation, deadline, or no-progress becomes terminal before or concurrently with the final bounded check
- **THEN** no child ownership transfers to the caller, the caller returns within its bound, and the owner kills and reaps the child before releasing the spawn lease

#### Scenario: Caller stops after the final check commits
- **WHEN** the final bounded check succeeds and a later stop occurs after the acknowledgement
- **THEN** the child transfer completes under the committed decision and ordinary caller or resident cleanup owns termination

#### Scenario: Owner cleanup cannot be proven
- **WHEN** an untransferred child cannot be conclusively killed and reaped
- **THEN** the spawn lease remains unavailable until cleanup completes and later launch attempts fail closed on sticky cleanup state

### Requirement: Corrective proof has one bounded task-state transition
ProjectAtlas SHALL first prove one exact #356 implementation head with the required focused, diagram, complete-local, hosted, and live-review gates. It SHALL then permit exactly one commit that changes only #356 OpenSpec task checkbox state and mirror that state to issue #356. Behavioral, diagram, complete-local, and clean optional-parser evidence MAY carry forward only across that non-runtime tree change. Ordinary exact-head CI, strict OpenSpec, IssueOps, ProjectAtlas low lint, and fresh live-review checks SHALL run as external closure gates on the task-state head, #356 SHALL remain open until they pass, and any other tree change SHALL invalidate the affected evidence. PR #360 merge, issue closure, and review of the resulting promotion head SHALL remain external release-readiness transitions rather than checklist prerequisites.

#### Scenario: Exact implementation proof is ready to reconcile
- **WHEN** every required implementation-head gate and live review disposition succeeds
- **THEN** one task-state-only commit may reconcile the #356 checklist without rerunning unaffected behavioral, diagram, complete-local, or clean-construction proof

#### Scenario: Task state is reconciled
- **WHEN** the bounded commit and mirrored issue contain no change beyond completed #356 task checkboxes
- **THEN** #356 remains open while ordinary exact-head CI, strict OpenSpec, IssueOps, ProjectAtlas low lint, and fresh live-review closure gates run

### Requirement: Linux optional-parser authority remains sealed and exact
The artifact contract SHALL reject a native-import policy declaration above the worker's 1 MiB pre-containment ceiling before the supervisor reads or retains that payload. Linux optional-parser residents SHALL execute only the sealed verified worker and load only the retained sealed selected grammar. Landlock SHALL deny every ABI v3-handled access to user-visible filesystem paths without attempting to attach a `PATH_BENEATH` rule to anonymous grammar authority; the retained sealed grammar descriptor remains the only post-containment library authority. Executable worker and grammar objects SHALL retain exact executable modes and document objects exact read-only non-executable modes across modern creation and `EINVAL`-only legacy fallback. Hosted lifecycle residue detection SHALL identify sealed workers by kernel-reported executable identity.

#### Scenario: Native-import policy exceeds its role ceiling
- **WHEN** an artifact manifest declares a native-import policy larger than 1 MiB
- **THEN** artifact validation rejects it before the supervisor reads, retains, clones, or seals policy bytes

#### Scenario: Anonymous grammar authority survives path denial
- **WHEN** the contained worker installs Landlock and later loads the selected grammar
- **THEN** the retained sealed grammar descriptor remains usable while opening an unrelated user-visible filesystem object for a v3-handled access remains denied

#### Scenario: Modern and legacy memfd creation
- **WHEN** executable and document objects are created on the modern path or the forced-`EINVAL` fallback path under `vm.memfd_noexec=1`
- **THEN** the worker and grammar remain executable with their exact required modes, documents remain non-executable and read-only, and fallback occurs only for `EINVAL`

#### Scenario: Sealed worker residue is inspected
- **WHEN** hosted lifecycle cleanup searches for surviving optional-parser workers
- **THEN** it recognizes the sealed executable through `/proc/<pid>/exe` identity rather than mutable launch spelling
