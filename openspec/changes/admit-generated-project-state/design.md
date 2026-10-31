## Context

The installed 0.5.0 disposable-fixture reproduction returns a failed scan because an
existing later policy requires a real project-state directory, but configuration,
nonsource metadata, database/WAL and generated host files have already been created
outside the intended automatic owner. Static CodeQL traces identify the same init
write chain. This is a late admission defect, distinct from caller-explicit paths,
#738 document-source admission and #739 Git policy input handling.

`run_init_bootstrap` canonicalizes the selected source root and checks database
identity, then calls `init_project_with_config`. The latter creates default/nonsource
files using automatic project paths. Registered-worktree init can call that helper
before hydration. CLI and MCP subsequently generate host files through their existing
writers. A valid database binding alone does not establish ownership of every output.

Ordinary navigation reproduces the same ownership error: a default database alias
can cause `overview` to initialize another database's SHM even with telemetry disabled.
A valid pending-WAL fixture also proves outside writes through hard-linked SHM.
Ordinary closed-database reads can leave bytes unchanged; preserve that counterevidence.
SQLite already refuses symbolic WAL/SHM aliases and the tested rollback-journal aliases;
hard-linked WAL/SHM remain confirmed manifestations owned here.

## Goals / Non-Goals

Admit automatic output ownership before writes, preserve supported explicit paths,
identity, authored state, ordinary/repeated/registered init and clear failure behavior.
Do not broaden source-tree traversal, add providers/schema/wire fields, disable local
capabilities or promise kernel-level protection from all concurrent ancestor races.

## Decisions

### Reuse the actual output owners

Trace defaults, nonsource files, database/WAL, generated host files, hydration
activation and direct writer callers from their current registries/call sites.
Use the existing selected-root and filesystem/project-identity boundaries. Distinguish
source-root canonicalization, automatic state ownership and explicit operator-selected
config/database destinations; none substitutes for the others. Preserve linked
worktrees and canonical selected-root aliases. No new resolver or public path policy.

### Database access is a separate admission role

Guard conventional `<selected-root>/.projectatlas/projectatlas.db` and its WAL/SHM
before any SQLite access, including root-identity discovery, CLI settings/root
reports, normal store opens, MCP startup/nearest/worktree/hydration/usage opens and
configuration-owned purpose reads. Derive the conventional owner from the selected
filesystem namespace before reading the DB; a root obtained from that DB is too late.
The same exact managed entry remains protected when spelled explicitly. Arbitrary
explicit external database paths remain supported.

Use each store or per-call state's already selected root to classify its managed
namespace; an external binding can legitimately have the same conventional filename.
Before root metadata is available, explicit configuration supplies a root without a
SQLite read. An implicit CLI binding supplies its selected conventional root. Explicit
startup storage without configuration uses the current known root for this admission;
a path outside that namespace remains an operator-isolated compatibility binding,
including when its basename looks conventional. Do not infer its root from SQLite
and then pretend the first sidecar write was preflighted. This follows the existing
explicit-binding capability and does not authorize implicit fallback to external data.

Use database-only admission for ordinary reads/writes: invalid or absent generated
host outputs must not disable valid navigation. Full generated-output admission
belongs to bootstrap, root transitions and host writers. Preserve existing selected
root and external config/database capabilities; no global root or routing framework.
Recheck current callers/registries rather than guarding only the first reported API.

### Admit before side effects and publish owned files safely

Validate the automatic project-state directory and intended outputs before bootstrap
creates or edits any generated state. Refuse non-directory, dangling/escaping or
otherwise unsupported automatic destinations according to the existing product
contract. Keep automatic output write paths from following unsafe leaf aliases.
Use existing atomic/create-new file mechanisms where applicable, preserving valid
pre-existing config/nonsource bytes and replacing only owned generated entries.
Evaluate file aliases/hard links and directory replacement as analogous write-boundary
cases rather than fixing only one directory example. Keep failures typed and causal;
cleanup must touch only the operation's owned artifacts. Revalidate at the mutation/
publication boundary; bound the actual guarantees and do not claim universal TOCTOU
elimination. Do not introduce lock files, retained directory-handle frameworks or
new dependencies without a demonstrated need and explicit reviewed tradeoff.

### Manual helpers share exact automatic document ownership

Actual installed 0.5.0 CLI and MCP tests confirm that `ignore add` follows a dangling
default `.projectatlas/config.toml` link and `ignore init-gitignore` creates the target
of a dangling root `.gitignore` link. The default map JSON sidecar also truncates
outside symlink and hardlink targets through both real interfaces. The legacy map
identity check protects only its TOON input, not arbitrary outside targets.

Reuse the operation-scoped guard and existing atomic publication with a document
role bound to one of four exact managed destinations: private `config.toml`, legacy
root `projectatlas.toml`, root `.gitignore` or private `projectatlas.json`. This role
checks only its owning directory and target; unrelated DB/host outputs do not gate
these mutations. The same managed namespace stays protected when spelled explicitly.
Custom/external operator config and map destinations keep their supported route.
A parent directory check revalidates publication containment; it does not classify
all direct children of `.projectatlas` as managed. Initialization chooses its writer
from exact document admission, preserving custom-name aliases and ordinary creation
permissions even when their selected parent is the private directory.

Acquire config write admission only for an actual change, so no-change operations
and ordinary reads preserve valid config aliases. Reject changed managed config or
default sidecar symlink/reparse entries; atomically replace regular entries so an
outside hardlink owner is not mutated. Preserve permissions of existing files and
ordinary creation permissions under the process umask, using the pinned tempfile
builder rather than changing process-wide umask or widening permissions afterward.
Keep the legacy TOON alias refusal before any JSON mutation.

Initialization routes managed config defaults through their document guard, even
inside the private directory. All newly created generated documents also use
ordinary 0666 filtered by umask; this includes nonsource defaults and host JSON
files, whose accepted writers previously used fs::write. Reinitialization preserves
existing bytes and modes, and replacement retains existing native access policy.
Native CLI and configured MCP must prove new-file modes beside explicit custom
and legacy configuration controls.

Gitignore creation treats any existing directory entry as its existing no-op,
including a dangling link. No-clobber publication handles a racing entry without
following it or claiming creation. Use the same bounded directory identities and
owned staging cleanup; do not add a general path framework or universal race claim.

Require causal owning tests and real CLI/MCP controls for dangling/live aliases,
regular hardlinks, legacy/default layouts, custom/external config and output paths,
unchanged mutations, existing-entry/racing Gitignore status and new-file modes.

### Preserve uncertainty at managed path classification

Classify the fixed document/database names with a local fallible boolean: managed,
proven external, or an ownership error. Propagate errors through all operation roles,
including generated-state capture, before side effects. Prove direct namespace
spelling before following the private directory; compare existing parent reductions
with their actual resolved location. For missing parents, resolve the deepest existing
prefix in original component order before reducing only its unresolved suffix.
Refuse a suffix that could become the exact managed parent before `mkdir_all` can
make it usable. This preserves physical symlink/`..` semantics and ordinary external
missing parents without introducing a public path framework.

Generated admission captures both private and legacy managed configuration. A missing
legacy config must be admitted before current-database native identity repair commits;
the initializer also retains its publication-boundary document check. Causal owning
tests cover all five selected DB/document roles, both parent states, generated capture,
root aliases, external physical parents and the existing-DB repair ordering.

### Recognize case aliases using the containing macOS volume

Exact fixed ASCII component spellings retain the no-query fast path. A pure candidate
filter includes Unicode simple and full folds, then establishes logical ownership
before querying the prospective containing volume. Resolve existing original parent
prefixes before reducing only unresolved suffixes, including across mount boundaries.
The whichdisk capability lookup supplies volume behavior, never ownership proof.

Native APFS evidence confirms that conﬁg.toml aliases config.toml: simple folding
alone is insufficient. Pin unicode-casefold 0.2.0, whose Unicode 9 full non-Turkic
iterator matches the documented APFS version, and apply that expanding comparison
only to a case-insensitive APFS volume. Preserve distinct names on sensitive volumes.
Do not infer aliasing from two hardlink identities or apply APFS full folding to HFS+.
The current non-APFS simple-fold path requires native HFS+/HFSX verification; do not
claim those Unicode rules are equivalent before the controlled volume tests pass.

Owning fixtures measure each native spelling pair before checking missing-leaf
admission, outside hardlinks, dangling namespaces and parent reductions. Permanent
CLI/configured-MCP controls include expanding config aliases and distinct custom
names. Controlled macOS CI covers APFS/APFSX and HFS+/HFSX with an opposite-case
nested mount. Windows Unicode aliases remain a native proof lead, not a claimed fix;
its fixtures must establish actual lookup behavior before changing classification.

### Preserve native access controls during replacement

The shared publication boundary replaces a file object rather than truncating it.
Copying std::fs::Permissions alone loses explicit ACLs. A native Linux fixture with
stricter destination access and broader inherited defaults reproduces the loss.
The affected registry is all three generated host files, private and legacy managed
TOML config, and the default JSON sidecar. Create-only Gitignore/defaults are separate.

Keep atomic replacement and outside-hardlink safety. Snapshot the admitted target's
native access controls and apply them to the operation-owned stage before writing
bytes or publishing. On Unix preserve uid/gid as well as mode so owner/group ACL
entries retain their subjects; test the mode/ACL-mask ordering. Linux reuses locked
xattr to copy kernel-owned POSIX/NFSv4 ACL values without decoding them; remove a
stage's inherited ACL when absent on the source. A source query that proves the
namespace unsupported does not authorize ignoring a successfully captured ACL.
macOS uses exacl's ordered extended-ACL representation. Verify native roundtrips.

Windows uses safe handle-based windows-permissions APIs with READ_CONTROL on the
source and appropriate WRITE_DAC/WRITE_OWNER rights on the stage. Preserve DACL
protection and ownership without lossy path conversion. The wrapper lacks a safe
control getter, so use only the native DACL SDDL header for inheritance flags, not
an application ACL parser. Avoid its NULL-DACL accessor trap; a native descriptor
that cannot be preserved must fail causally before replacement. Keep original stage
handles and deny competing data handles while inherited access is being replaced.
Verify protected, inherited, NULL, protected-empty and unprotected-empty ACL behavior, stage ACL readback,
publication failures and cleanup on Windows before accepting this implementation.
An absent DACL belongs to an unattached descriptor, not the real-file snapshot:
the Windows SDK's `SE_DACL_PRESENT` contract guarantees that bit once associated
with a securable object. Keep synthetic diagnostic handling distinct from native
NULL/empty file ACL controls; do not require an impossible absent-file fixture.

Native Windows evidence shows SetSecurityInfo can normalize only the AI control
flag on ordinary regular-file replacement while preserving owner/group, exact
ordered ACEs and their inherited markers, P, AR and NULL/empty policy. Permit only
that AI normalization for admitted regular-file destinations, which have no child
ACLs to propagate; all other header text and ordered ACEs remain exact. Do not
generalize this comparison to directories or describe inheritance state as mere
bookkeeping. Keep private diagnostics for any remaining difference and native
ordinary/protected/inherited/NULL/empty controls. The .NET fixture formatter omits
D: for NULL descriptors; inspect its binary RawSecurityDescriptor access class
instead of using omitted SDDL text as proof of an absent native file DACL.

The safe native wrappers require direct macOS-only exacl and Windows-only
windows-permissions dependencies. The latter retains bitflags 1; the dependency ban
exception is exact and justified by safe handle/OsStr compatibility. Alternatives
using in-place writes violate hardlink/atomicity guarantees; a common ReplaceFile
wrapper accepts only str paths and has different partial-publication failures.
No project unsafe code, shell runtime dependency, global ACL change, or blanket ban
on regeneration is introduced. Native failures keep the old valid output; do not
fall back to a publication that drops access controls. All platform-specific proof
and dependency gates remain required; current Linux evidence is not Windows proof.

### Explicit paths remain supported

An explicitly selected external database or config path is an intended operator
capability and still follows its existing identity/configuration rules. A selected
project root may itself be a canonicalizable alias. These positive paths must work
alongside automatic-state refusal. Existing config read aliases that are not written
must not be confused with unsafe generated-output writes. Avoid converting intentional
filesystem selection or Git-policy reads into this finding.

### Keep initialization adapters and refresh paths consistent

Normal database access, CLI/MCP initialization and registered-worktree hydration must use their
appropriate shared admission role before
creation/activation. Direct generated-host writer entry points must not bypass its
write invariant. Existing valid DB identity and authored purposes remain intact on
refusal, reinit and compatible upgrade. Root transitions must admit automatic outputs before changing identity; preserve
the existing committed-transition/follow-up-error contract for later unrelated
I/O failures rather than pretending an already committed transition rolled back.
No derivation-contract bump is needed solely
for output admission because source parsing/resolution facts are unchanged; reevaluate
only if implementation changes a derived-data contract.

## Risks / Trade-offs

- An early blanket guard can break explicit external paths -> distinguish ownership
  roles and prove positive CLI/MCP external DB/config and selected-root aliases.
- One adapter can still write first -> trace CLI/MCP/hydration/direct host writers,
  test the shared boundary and permanent installed selectors.
- A file alias or replaced parent can bypass a directory-only guard -> causal
  leaf/directory/regular-existing controls and boundary revalidation; document limits.
- Refusal/cleanup can lose valid user state -> exact file bytes, identity, all approved
  purposes and last valid publication controls; clean up only newly owned artifacts.

### Native implementation and platform evidence

Reuse existing same-file handles and tempfile publication. A direct Windows-only
winapi-util dependency reuses the already locked canonical utility solely for stable
hard-link counts; do not add unsafe platform bindings or unstable std APIs. Ordinary
DB admission stays constant in the fixed product file family and does not index,
parse or walk source trees. Preserve valid generated-file permissions and document
bounded retention when changed ownership makes automatic staging cleanup unsafe.

## Migration Plan

No schema migration/reset or authored-state conversion. Existing valid projects
continue to initialize and regenerate host configs. Unsupported automatic destinations
receive an actionable refusal before writes. Compatible runtime reopen/upgrade must
retain identity and approved state; explicit paths remain supported. Reconcile final
source with #715 installed packages/platforms and public inventory before RC1.

## Architecture Disposition

N/A: existing bootstrap, filesystem admission, configuration/database and host-output
owners remain; this change moves their existing ownership checks before side effects
and closes writer bypasses without adding a component, persistence schema or protocol.
Reassess this disposition against final implementation.

## Dependencies / Cross-Issue Impact

#741 has no direct implementation blocker on accepted main. It directly belongs to
and blocks #715 in v0.5.1-00. #731/#738/#739/#708 proceed independently; final integration
must union accepted owners and rerun the installed generated-state selectors and
positive external-path workflows through the combined CLI/MCP/host packages.

## Open Questions

None.
