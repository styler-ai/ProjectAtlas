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

Retaining only the publication directory does not establish ownership of a staged
filename. A causal callback can rename the held temporary file and substitute a
foreign leaf without changing that directory; the prior path-based persistence or
cleanup then publishes or unlinks the wrong object. Check the direct regular stage
entry against the retained file handle before path-based metadata changes, payload
writes, publication and failed-publication cleanup. Withhold cleanup when either
directory or file identity changes, using the existing typed retention error. Keep
the same bounded final-check guarantee, without claiming an atomic adversarial race
barrier. Cover both normal-publication and controlled-failure substitutions.

On Windows, bind the rename itself to that retained stage object using the
canonical SDK `SetFileInformationByHandle` / `FILE_RENAME_INFO` boundary, with
the original handle acquiring DELETE at creation. Preserve both replacement and
no-clobber semantics. A successful rename disables cleanup of the former temporary
name; a failed rename retains the existing identity-checked cleanup path. Check
the variable UTF-16 buffer size and use SDK-aligned storage, without introducing
a second path resolver or claiming an atomic destination-directory race barrier.
Native causal coverage exchanges the old stage name for a foreign file and proves
that no-clobber preserves the destination, replacement publishes the held object,
and neither operation changes the foreign entry.

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
Native 9377241 HFS+ proof rejects modern simple folding: long-s is distinct, while
ZWJ/BOM aliases bypassed classification. Use a separate HFS predicate for the fixed
ASCII product alphabet. Inspection of Apple's frozen comparison data across every
BMP unit finds exactly sixteen zero-mapped characters and no non-ASCII mappings to
ASCII. Unicode 3.2 canonical decomposition, with HFS exclusions, adds no alias to
that fixed alphabet. Thus stream ASCII case conversion and skip only U+200C–200F,
U+202A–202E, U+206A–206F and U+FEFF; reject other non-ASCII characters. Use the
predicate in both the initial candidate filter and the case-insensitive HFS/HFSX
decision. Case-sensitive HFSX retains binary comparison, including these characters.
Keep APFS on its existing Unicode 9 rule. This is a comparison specialized to the
product names, not a general HFS implementation or imported Apple routine/table.

Source provenance: Apple TN1150 and HFS commit
d1bac2f062e6e9c0dfcce302d9aacb10173d0eea, core/UCStringCompareData.h; local analysis
verified the full table and Unicode 3.2 normalization separately. Keep the derived
behavioral facts and source reference, without redistributing Apple implementation.
Existing selected-root directory aliases can contain arbitrary Unicode; prove those
with direct directory identities and their resolved parent, before comparing fixed
managed components. Never infer managed leaf filename aliasing from file hardlinks.
Native tests must measure root aliases and every managed role on all four formats,
including missing names, outside hardlinks, sidecars and distinct custom controls.

Owning fixtures measure each native spelling pair before checking missing-leaf
admission, outside hardlinks, dangling namespaces and parent reductions. Permanent
CLI/configured-MCP controls include expanding config aliases and distinct custom
names. Controlled macOS CI covers APFS/APFSX and HFS+/HFSX with an opposite-case
nested mount. Windows Unicode aliases remain a native proof lead, not a claimed fix;
its fixtures must establish actual lookup behavior before changing classification.

### Recognize Windows directory case and native short entries

Use the actual containing directory's retained metadata-only handle and canonical
FILE_CASE_SENSITIVE_INFO; reject unknown flags or query failures as uncertainty.
Exact fixed names remain fast. Resolve existing physical prefixes before parent
reductions, then use the deepest existing creation ancestor for missing children;
native controls must establish child-directory inheritance in both modes.

A pure UTF-16-length/literal candidate screen includes a conservative superset of
8.3 names without assuming a tilde, OEM conversion or making filesystem queries.
Only after proving the logical/physical parent is managed, FindFirstFileW queries
one literal selected component. Its cFileName describes the direct matched entry,
including a reparse entry itself, not its target. An exact fixed long name is owned;
a case-only spelling is owned only in an insensitive directory. A missing short
entry is custom; unexpected lookup failures retain ownership uncertainty. The
existing leaf admission still rejects indirect managed entries. Do not use file
identity, a directory enumeration, or GetLongPathName as the name oracle.

Native eb97842 diagnostic evidence establishes parent/child case flags0/1 on both
host volumes; ASCII uppercase aliases only in insensitive directories. C: exposes
actual 8.3 aliases in both modes; D: exposes none. Dotless-i, long-s and fi-ligature
spellings were distinct on that host, not a general Unicode assertion. Owning
controls compare short-name uppercase/lowercase classification with actual lookup,
cover all fixed file roles/private directory, missing parents, reparse entries and
separate custom hardlinks. Real CLI/configured MCP ignore and map controls must
preserve a distinct lowercase managed config when a sensitive uppercase custom
config is selected, and preserve managed/outside-owner behavior for actual aliases.
Cross-compilation or diagnostic job success alone is not native acceptance.

Actual 583 raw CreateFileW controls prove ordinary final-dot/space lookup aliases
for all six fixed components in both case modes, while the same verbatim strings
are distinct. Normalize only the ordinary Win32 component before the existing
candidate/native-entry comparison. Preserve literal verbatim names and ADS syntax. Native 652 raw lookup proves
that drive paths under the `\\.\` device prefix share ordinary final-dot/space
normalization; include those spellings in all owning roles and native controls. Do not infer naming rules from a managed file's hardlink identity.

Config root inference is a producer of the admission root. Reuse the same fallible
private-directory comparison for relative `.` and absent roots, before following
the selected directory spelling. Prove real CLI/configured-MCP changes through
directory aliases preserve outside hardlinks, while distinct sensitive custom
directories retain their root semantics. Initialization emits `.` for a private
config only if its implied project root is the requested root; an explicit config
in another project's private directory must retain the requested root.

Native 652 controls establish that the locked SQLite 3.53.2 VFS normalizes ordinary
final-dot/space names to the canonical filename, but retains actual DOS short names.
The short writer commits to its own WAL while a canonical reader sees zero rows;
both WAL/SHM pairs coexist. Filename equivalence alone is not cohort equivalence.
Route every proven managed Windows database open through its canonical long leaf,
including writable/read-only wrappers, root/settings discovery, bootstrap identity
repair and direct configuration consumers. This fixed-name operation is constant
in the product file family; it does not inspect rows or walk source trees. New
concurrent managed opens always choose the same cohort, even before any WAL exists.
Explicit unrelated/custom names and verbatim literal suffixed files keep their route.

Before SQLite, query the canonical main entry's actual cAlternateFileName and refuse
if its alternate WAL, SHM or rollback journal exists. Preserve all recovery bytes;
explain that users must close the cohort and use an external SQLite recovery or
checkpoint tool with its original short spelling before reopening. Never checkpoint
one cohort through another filename, silently ignore pending commits, retarget or
delete state. A separately removed/renamed main file leaves no authoritative short
name to discover; arbitrary orphan aliases are outside this bounded proof. Do not
guess aliases or enumerate a directory to imply complete orphan recovery.

Explicit reset remains available for corrupt or pending state. Give it a separate
admission role retaining the selected root/private-directory identities without
opening SQLite or requiring valid single-link leaves. Preview canonical main and
sidecars plus only its proven alternate recovery names. Revalidate directories
before each owned-name unlink/staged move; removing an admitted hardlink leaves
outside-owner bytes intact. Preserve registered reset lifecycle checks before inventory
and each stage move. Restore a staged failure only while its namespace still verifies;
otherwise retain recoverable originals and report the ownership error without
restoring into a replacement directory. CLI root
selection for reset uses configuration/layout/cwd authority without metadata reads;
MCP uses its already captured state. Require causal owning tests for shared revisions,
legacy refusal/bytes, original-spelling checkpoint recovery, identity repair and
crash-triplet reset, plus real configured CLI/MCP purpose visibility/refusal and
preview/apply on both reset surfaces.

### Recognize Linux directory casefolding and preserve sealed outputs

The Linux platform is not uniformly case-sensitive. Keep exact spellings fast;
after the existing logical namespace proof, query a retained descriptor for the
actual containing directory with locked rustix FS_IOC_GETFLAGS and fstatfs. Resolve
existing physical prefixes and parent reductions first; missing child directories
inherit the existing creation parent's casefold flag. Never query unrelated external
parents or infer file-name equivalence from leaf inode equality.

Mounted ext4 and f2fs currently accept only their Unicode 12.1 encoding identifier;
their native casefold flag selects that model. Use a small ASCII-target projection
derived from official UCD 12.1 canonical decomposition, full/default casefold and
assigned default-ignorable deletion. It is not the APFS Unicode 9 or modern regex
model. Keep its input provenance and an exhaustive independent UCD derivation.
ASCII conversion, nineteen assigned-ignorable ranges and thirteen nonempty ASCII
chunks suffice; every other scalar cannot match a fixed ASCII product name.
Sensitive ext4/f2fs and unflagged btrfs/tmpfs retain distinct custom names; XFS's
flagged ASCII model uses ASCII folding. Flagged tmpfs is currently treated as an
unrecognized/parameterized model, not claimed sensitive. Current upstream ships
only Unicode 12.1 tables; future table additions require comparison-profile and
candidate-filter review. An unrecognized or parameterized model gives a
causal ownership error only after a potential fixed-name candidate and namespace
proof, never an external-writer downgrade. No all-filesystem claim is made.

Apply the candidate and native predicates to database/sidecars, managed documents,
host outputs, private-directory components and root aliases. Native disposable
ext4/f2fs images prove folded and sensitive siblings, inherited flags, Unicode and
custom hardlinks, reductions, outside-owner preservation and real CLI/MCP behavior.
Other driver profiles require their own evidence; do not claim ext4 proves them.

Linux replacement also refuses fs-verity-protected destinations through the retained
no-follow source before creating a stage and during final source revalidation.
Use rustix statx EMPTY_PATH with the VERITY support mask, falling back to GETFLAGS
when the field/API is unavailable. A successful clear flag proves absence; a set
flag refuses. Combined documented unsupported APIs establish unavailable fs-verity
under the kernel contract: GETFLAGS exposes verity on the Linux 5.4 implementation,
and statx exposes it since 5.5. Permission/I/O/other errors propagate, never absence.
No filesystem whitelist, re-sealing, stale signature copying or privilege change.
Native sealed-file controls prove the prior rename loses protection, unchanged
source/outside hardlinks and no leaked stage on refusal, a late-seal recheck, and
ordinary and custom-path positives. Local unsupported setup is not native proof.

Primary sources: Linux fs/unicode/README.utf8data and mkutf8data.c;
fs/ext4/super.c and fs/f2fs/super.c encoding maps; official Unicode 12.1 UCD;
https://docs.kernel.org/filesystems/fsverity.html.
The independent derivation checked every assigned scalar against the official
UCD inputs, including canonical corrections and assigned default-ignorable
deletion. SHA-256 provenance: CaseFolding.txt
`9c772627c6ee77eea6a17b42927b8ee28ca05dc65d6a511062104baaf3d12294`,
DerivedAge.txt `2fc081011d8fabaf7cf4937732dd5a6d6a57e492c43f3adfeded513387ee0ec3`,
DerivedCoreProperties.txt
`a6eb7a8671fb532fbd88c37fd7b20b5b2e7dbfc8b121f74c14abe2947db0da68`,
UnicodeData.txt `93ab1acd8fd9d450463b50ae77eab151a7cda48f98b25b56baed8070f80fc936`.
Canonical crates in the locked graph use other Unicode versions, so this tiny
fixed-ASCII projection is a reviewed exception, not a replacement Unicode library.

### Respect Windows directory-local naming

An unconditional ASCII fold over-classifies `CONFIG.TOML` in a case-sensitive
private directory and can redirect its complete selected content into the distinct
lowercase managed config. Repair the shared fixed-component classifier for database,
sidecar, document and private-directory roles. Exact spelling retains its no-query
fast path. For aliases, query the physical containing directory through a
read-attributes, non-following directory handle using the SDK
`GetFileInformationByHandleEx(FileCaseSensitiveInfo)` and its directory flag.
Propagate unsupported/error state as ownership uncertainty rather than assuming
insensitivity. Preserve unrelated custom/external selections.

Resolve original existing prefixes and parent reductions before selecting the
deepest existing directory for a missing parent. Native Win32-created child
directories inherit this flag; the documented WSL 1 creation exception is outside
this native writer. Prove actual native inheritance and both modes on one NTFS
volume, including missing private directories and reductions. Compare normal
ASCII case variants, relevant Unicode spellings and reported short-name aliases
against real filesystem lookup before claiming those surfaces. A same-inode
hardlink is not a naming-rule oracle; distinct custom names remain distinct.
Owning controls cover the fixed DB/document registry, private-directory aliases,
query failures and absence. Real CLI/MCP ignore/map operations must preserve the
distinct lowercase file when a sensitive-directory uppercase custom file is
explicitly selected. No per-volume assumption, general resolver or global case
setting is introduced.

### Preserve native access controls during replacement

The shared publication boundary replaces a file object rather than truncating it.
Copying std::fs::Permissions alone loses explicit ACLs. A native Linux fixture with
stricter destination access and broader inherited defaults reproduces the loss.
The affected registry is all three generated host files, private and legacy managed
TOML config, and the default JSON sidecar. Create-only Gitignore/defaults are separate.

Keep atomic replacement and outside-hardlink safety. Snapshot the admitted target's
native access controls and preserve them before exposing replacement payload bytes.
On Unix preserve uid/gid as well as mode so owner/group ACL
entries retain their subjects; test the mode/ACL-mask ordering. Linux reuses locked
xattr to copy kernel-owned POSIX/NFSv4 ACL values without decoding them; remove a
stage's inherited ACL when absent on the source. A source query that proves the
namespace unsupported does not authorize ignoring a successfully captured ACL.
macOS retains opaque native extended ACLs through descriptors; exacl provides only
an independent path-based test oracle. Verify native roundtrips.

Windows uses safe handle-based windows-permissions APIs with READ_CONTROL on the
source and appropriate WRITE_DAC/WRITE_OWNER rights on the stage. Acquire stage
policy rights on its original create-new handle; never reopen its mutable pathname
for SetSecurityInfo. Require READ_CONTROL and WRITE_DAC for replacements, requesting
MAXIMUM_ALLOWED alongside required data rights so an available WRITE_OWNER can be
retained without demanding it for unchanged owner/group. Fresh creation retains
ordinary data rights. SetSecurityInfo's MAXIMUM_ALLOWED child-propagation limitation
does not affect admitted regular files. Copy through a duplicated retained handle;
native hardlink-substitution and denied-TakeOwnership controls must prove both
outside-policy preservation and legitimate same-owner regeneration. Preserve DACL
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

Native Windows six-case controls isolate the retained destination snapshot as the
cause of MoveFileEx ERROR_ACCESS_DENIED: absent/released snapshots allow replacement,
while a retained snapshot refuses, for both single files and outside hardlinks.
Keep every snapshot handle through pre-copy and final destination-policy checks,
then release the complete Windows snapshot immediately before persist. Do not
drop it before verification or weaken the existing bounded final-check contract.

Independent native macOS path controls use exacl only as a target-specific test
dependency. Production uses the isolated descriptor owner below. Windows uses the
safe windows-permissions dependency, which retains bitflags 1; the dependency ban
exception is exact and justified by safe handle/OsStr compatibility. Alternatives
using in-place writes violate hardlink/atomicity guarantees; a common ReplaceFile
wrapper accepts only str paths and has different partial-publication failures.
Apart from the isolated macOS descriptor ACL, Windows descriptor/empty-ACL and
retained Windows rename exceptions
below, no project unsafe code, shell runtime dependency, global ACL change, or blanket ban
on regeneration is introduced. Native failures keep the old valid output; do not
fall back to a publication that drops access controls. All platform-specific proof
and dependency gates remain required; current Linux evidence is not Windows proof.

### Preserve Windows system policy without blanket privilege requirements

Windows replacement staging must prevent capabilities from being acquired before
the destination policy is applied. In a shared parent, an actor may have create
and inherited child DELETE rights without FILE_DELETE_CHILD on the parent or
modification rights on the protected existing output. Its already-open stage
handle remains valid after DACL tightening and even after publication. A final
identity check or handle-based rename alone does not revoke that capability.
Create replacement stages with a protected initial descriptor that admits no
broader inherited data, delete or write-DACL access; suppress implicit owner
write-DACL too. Retain the creator's original data and security handle through
policy preparation and publication. Genuinely new outputs retain normal parent
inheritance. Native controls must establish creator rights, restricted-actor
acquisition denial, positive replacement and owned cleanup before acceptance.
An SDK-only probe validates the exact creation descriptor before selecting the
production binding; do not infer Windows runtime behavior from cross-compilation.

The production constructor belongs to the existing tempfile callback. Capture a
combined owner/group/DACL/system descriptor through the retained source, require
it to match the admitted snapshots, and capture/retain the exact parent directory
and matching policy. Use canonical CreatePrivateObjectSecurityEx with the effective
thread token (process fallback only for ERROR_NO_TOKEN), real DACL/SACL inheritance
checks and file generic mapping. Require matching owner/group and ordered ACL policy
before creating a name; only preview AI is ignored. Normalize the SDK-ignored
DEFAULTED bit consistently only when SACL_PRESENT is clear, across separate,
combined and assignment-view queries. Present defaulted policy still refuses and
all meaningful final system-policy controls remain strict. Combined-snapshot
mismatch diagnostics identify source/parent, query mode, control values and match
booleans without exposing native ACEs or principals.

Protect DACL inheritance and present SACL inheritance in the owned creation
descriptor; never synthesize SACL presence from absence. Retain the original native
DACL comparison image and system-policy snapshot. CreateFileW(CREATE_NEW, SECURITY_ATTRIBUTES) returns
the one noninherited data/delete/control handle used by the publisher. Request
WRITE_DAC only when restoring an unprotected DACL; retain optional policy rights
with MAXIMUM_ALLOWED only for inheritance restoration or full system policy.
Protected ordinary replacements need no unused WRITE_DAC/WRITE_OWNER. Revalidate
the retained parent policy before creation and restoration, and restore from the
captured opaque descriptor rather than querying fresh mutable source policy.
Verify the actual stage's initial native owner/group, protected ACLs and complete
system-policy image before callbacks, repair or payload. Unexpected initial policy
refuses and cleans the empty owned stage; it is never repaired and later published.
A fresh broader source query could otherwise transiently broaden the stage and
leave retained capabilities even if a later source check passes again.

Independent restricted-token controls must prove actual parent child-creation and
owned cleanup, parent DELETE_CHILD/WRITE_DAC denial and protected-target mutation
denial. The former inherited stage must permit DELETE/WRITE_DAC handles that remain
usable after tightening and rename. The actual Rust stage callback must deny those
opens before policy preparation; use an all-share native clone of its actual DACL
to distinguish data-policy denial from sharing denial. Cover all six replacement
roles, final policy/payload, outside hardlinks and original-handle failure cleanup.
Audit/integrity/RA/SP positives must also cover protected sources without optional
discretionary policy rights. If SACL presence, NULL/allocation state and the complete
ordered opaque ACE image already match, restore inheritance with the SACL selector
and its protection flag only. Requesting unchanged LABEL/ATTRIBUTE/SCOPE selectors
can unnecessarily require WRITE_OWNER/WRITE_DAC. A meaningful data difference retains
the supported full assignment view; every single setter requires exact full readback,
with no drop-then-repair fallback. Require an unprotected mixed AU/ML/RA/SP source
under reduced and denied discretionary rights across all six roles, proving that
the stage starts protected and actual restoration occurs before payload. New files
keep ordinary inheritance. Restricted-actor reopen controls include synchronous
handle and metadata rights; their failure before tightening is a fixture failure,
not a successful negative security control.

Different-owner-but-authorized restoration is a separate native compatibility
question: compare the former setter under already enabled SeRestorePrivilege,
ordinary CPE owner validation, owner-check-only avoidance and direct native creation
with/without backup semantics in disposable diagnostics. No owner-restriction or
privilege-avoidance flag is adopted. Do not claim the production path preserves
that workflow until its actual creation/ownership readback and publication pass.

Standard Rust file creation exposes no creation descriptor. Extend only the local
official-SDK exception with effective-token queries, CPE/destroy/control and
CreateFileW; wrap native owning handles exactly once and use matching allocation
destructors. Threading is an additional feature of the already locked windows-sys,
not a new package. Keep workspace unsafe denial and the source policy boundaries.

The native Windows publisher must also preserve SACL policy: mandatory integrity,
audit entries and other system-policy ACEs belong to the replaced object, not the
new stage's inherited defaults. Querying only owner/group/DACL is insufficient.
Try a no-follow retained source handle with ACCESS_SYSTEM_SECURITY; fall back to
the existing READ_CONTROL handle only for native access/privilege denial. Never
enable privileges process-wide or require them for ordinary no-SACL regeneration.

With full access, use the canonical retained query with the named SDK selectors
SACL, LABEL, ATTRIBUTE, SCOPE, PROCESS_TRUST_LABEL and ACCESS_FILTER (0x1f8).
Use that complete selector set for native serialization. The locked
safe wrapper predates the latter two flags; bridge only those named current SDK
constants with its documented bitflags constructor. The canonical wrappers forward
these bits unchanged. No privilege change or file-data read is introduced.
Native evidence rejected BACKUP here: all 144 full-rights retained calls returned
ACCESS_DENIED despite the documented READ_CONTROL/ACCESS_SYSTEM_SECURITY rights.
Do not use BACKUP as a setter or SDDL conversion flag. Native same-handle evidence
also rejects the 0x1f8 setter: all ten nonempty audit/label/RA/SP/mixed
controls return ACCESS_DENIED while 0x78 succeeds with exact native readback.
This proves the query and setter contracts differ, not which extra flag caused
failure. Assign only SACL/LABEL/ATTRIBUTE/SCOPE (0x78). At each complete capture,
query both 0x1f8 and 0x78 through the same retained handle, validate both canonical
images against native ACE counts, and require identical relevant controls,
presence/null-versus-allocated state, ACE count and ordered image. Retain the 0x78
descriptor only after agreement. Otherwise refuse before staging; repeat this
proof for stage capture/readback. Do not silently drop a queried but unsettable
class or request extra privileges to make an unsupported selector succeed.
TL/FL setup returned INVALID_PARAMETER87; this is an unresolved native fixture
proof gap, not evidence of absence, unreachability or preserved compatibility.
Native same-handle controls
compare 0x18, 0x78, 0x1f8 and BACKUP variants against independently installed classes;
fixture/setup refusals are proof gaps, not successful preservation.
Without full access, use a LABEL-only query's
native control word to prove the original SACL is absent. MS-FSA 2.1.5.14 preserves
the original SACL-present/control bits while filtering audit ACEs from that query:
an empty filtered ACE list is not proof that a present SACL contains no auditing.
Absence means the SACL-present bit is clear, not that every related control bit
is zero: native ordinary files can retain auto-inheritance state without a SACL.
Retain those bits in the source/stage comparison; ignore defaulting only when
the SACL is absent, as the SDK specifies.
Require the no-privilege stage to prove absent policy too; otherwise refuse before
payload I/O. Keep create-only workflows and ordinary unlabeled replacement usable.
If full policy cannot be queried/copied, retain valid output rather than publishing
weaker policy. Explicitly labeled/audited replacement can therefore require already
available SACL rights; this limitation must be documented and proved natively.

Acquire any full SACL rights on the original stage creation handle. Apply through
that retained handle only, guard present-null SACLs from the wrapper's panicking
accessor, and verify ordered entries and control state before payload writes.
Retain strict SACL AI comparison until native evidence proves a safe normalization;
the earlier DACL normalization does not establish SACL behavior. Recheck source
system policy alongside owner/group/DACL immediately before replacement.
Serialize complete system policy with all six query selectors above; assign only
the empirically supported four-selector view after exact native agreement. In particular,
SACL-only serialization omits mandatory-label ACEs even when the descriptor holds
them; complete comparison requires SACL plus LABEL. Native controls must distinguish
identical audit/control policy with high versus medium integrity labels, repair a
lower-labeled stage, and reject independent source or stage label changes.
Before admitting a nonempty SACL comparison image, compare the native ACL's ACE
count with the count obtained by the canonical SDDL decoder. Refuse omitted entries
before staging. Query native presence and the borrowed ACL pointer independently
of the comparison image, then use the fallible canonical ACL-size wrapper; add no
parser, unsafe layout access or speculative security-info flags.
This is an omission guard, not proof of byte/semantic fidelity for in-place codec
changes. Native mixed and sole-label controls must reject deliberately SACL-only and
NULL-marker images without panicking, and accept complete supported policy.
The SDK experiment disproves the assumption that the MS-FSA raw SACL copy implies
an unfiltered Win32 result: an independently installed RA-only or SP-only ACL is
present in the full query but omitted by the SACL|LABEL query. Test the actual
retained GetSecurityInfo route as well as the independent named oracle. The
ACE-count guard cannot detect classes already filtered by a query. Cover current
SDK selectors independently, including process trust/access filter fixtures where
NTFS supports them. If the selected canonical serializer omits returned entries,
the existing count guard refuses before staging rather than publishing partial policy. Do not add a BACKUP setter, require unused ownership rights, or
hand-parse native ACEs in production.

Native RA-only, SP-only and mixed audit/label/RA/SP controls must prove the former
retained query omits policy, then compare the complete source, held stage and
published native ACL bytes/order/control and preserve the outside hardlink. Initial
fixture SET may canonicalize cross-class order: verify each exact requested ACE
payload and SID independently, then retain exact ordered before/after comparisons
from the actual installed source. The synthetic scoped-policy SID proves opaque
preservation, not domain CAP enforcement. Preserve ordinary absent-policy positives
and unprivileged refusal. The shared fixture must compile on both Windows
PowerShell/.NET Framework and PowerShell 7; use their common runtime APIs.
Run expanded owning fixtures from an owned UTF-8-with-BOM temporary script, held
until the child completes, rather than an oversized `-Command` argument. Share
that transport across full-fixture and declaration-only consumers; keep its
execution-policy setting scoped to the fixture process and prove large Unicode
script execution without changing product shell or privilege behavior.

The unprotected present-NULL SACL case needs a non-NULL zero-entry input to the
native setter: NTFS normalizes that input to the original unprotected NULL state,
whereas a NULL input retains protection. Gate this adapter strictly on native
PRESENT, unprotected control, and a NULL pointer; absence and protected NULL retain
their existing representation. Initialize SDK `ACL` storage inside a DWORD-aligned owning wrapper with
`InitializeAcl(size_of::<ACL>(), ACL_REVISION)`, check BOOL, and borrow the initialized
storage as the canonical wrapper's opaque ACL only for the held setter call. Strict
source/stage native readback remains authoritative and failure refuses before
payload. Do not treat named-setter proof as held-setter proof; native controls must
include the actual creator's optional label/WRITE_OWNER rights and their absence.
Reuse the already locked official `windows-sys` 0.61.2 as a Windows-only optional
CLI dependency with `Win32_Security` and `Win32_Storage_FileSystem`; use its canonical getter bindings too. No new
package version, hand-written ACL layout, ACE parsing or privilege changes.

The independent reviewer approved a second tiny platform-local unsafe exception:
SDK GetSecurityDescriptorControl, GetSecurityDescriptorSacl and InitializeAcl in a private Windows
security module, borrowing the opaque descriptor returned by windows-permissions.
The latter is needed because the safe accessor panics on a present NULL SACL; only
borrow a native non-NULL ACL while its owning descriptor lives. Check BOOL and descriptor
revision; introduce no parser, descriptor-layout cast, dependency or privilege
toggle. The canonical safe wrapper has no control getter, and SDDL omits
SE_SACL_DEFAULTED. Inspect that bit and refuse a defaulted SACL because the current
SetSecurityInfo adapter cannot reproduce it. Workspace unsafe denial remains
unchanged elsewhere. Native tests must prove ordinary source/stage absence,
filtered audit presence, label-only and mixed audit/label policy, available and
unavailable privilege, inherited stage mismatch, outside hardlinks, source-policy
changes and retained-handle substitution. Do not claim ReFS or defaulted-file
fixture coverage without actual proof.

Primary references: Microsoft's SECURITY_INFORMATION, SECURITY_DESCRIPTOR_CONTROL,
GetSecurityDescriptorControl and MS-FSA 2.1.5.14 documentation. This is an access
policy correction, not a general promise to clone every Windows file attribute.

### Prepare native policy before exposing replacement payloads

Linux captures uid/gid, mode, POSIX/NFSv4 ACL values and SELinux/Smack file-label
values through a no-follow descriptor bound to the admitted regular destination.
The source descriptor is retained. Skip only a proven unsupported attribute
namespace; preserve absence as well as bytes. Include the Smack object, exec and
mmap labels, not directory/socket-only attributes or content-bound IMA/EVM data.

A newly created empty named probe may be used only when its complete captured
access policy already equals the destination policy. No setters are necessary in
that case. Otherwise identity-check and discard that empty inode permanently.
Smack checks access at open rather than every subsequent I/O, so relabeling a
visible inode cannot revoke an already-open data handle. Use already-locked rustix
safe filesystem APIs to create O_TMPFILE, write and sync complete bytes while
unnamed under the creator's policy, then apply and verify all captured metadata
and sync before linking a generated staging name
and atomically publish. Link by descriptor where permitted or the documented
proc-self-fd symlink route for unprivileged callers. Unsupported anonymous creation,
linking or policy assignment refuses before replacing valid output. Equivalent
named-probe policy retains ordinary filesystems and avoids gratuitous relabeling.
This ordering also avoids requesting further data writes after a SELinux SID
change could revoke them on the retained handle. Revalidate the admitted target's
inode and captured access policy before replacement; an absent Replace target
must remain absent. Create-only no-clobber races retain their existing result.

macOS inherited ACL allows can override0600. A directory can also permit entry
replacement without an inheritable ACL, so equal ownership and absent inheritance
do not make a pathname ACL setter safe. Every existing-target stage therefore uses
a private same-volume staging directory, whose ACL is cleared and0700 mode verified
before payload file creation. Native ACL reads/writes take a borrowed retained File
and call Darwin's actual descriptor ACL API, including workspace hardening, source
capture/rechecks and stage copying/readback. Native tests on bf8df82 disprove the
/dev/fd path proxy: it returns empty ACLs for an inode with an extended ACL, and
the ordinary preservation test detects ACL loss. Remove that route completely.
The descriptor stays owned and live throughout each native call; no mutable project
pathname is supplied to an ACL getter/setter. The directory mode
setter also uses a NOFOLLOW, directory-only descriptor bound to its retained identity. Keep that directory
owned by the writer even when the destination has a different owner. Keep genuinely new
outputs in their original parent to preserve supported inherited grants. Retain
private-directory identity, verify both stage parent and held file, and remove only
the verified empty directory without recursive TempDir cleanup. A failed cleanup
after successful persist is explicitly a committed-publication follow-up error.
Native nobody-held-FD controls prove the original window and the protected actual
publisher; positive new-file controls retain normal inheritance. No process-global
security-context changes or shell runtime dependency.

#### Controlled macOS native ACL exception

The independent reviewer explicitly approved one small private macOS-only FFI
module after the failed proxy proof. Workspace unsafe_code remains deny elsewhere.
The locked exacl crate exposes no safe descriptor API and libc exposes no Darwin
ACL symbols. Vendoring the complete ACL crate to expose those private routines is
a larger maintenance surface. Bind only six Apple SDK primitives: acl_get_fd,
acl_set_fd, acl_init, acl_free, acl_size and acl_copy_ext. Apple's current Libc
acl_file.c forwards get/set to ACL_TYPE_EXTENDED; use that implementation evidence
and native proof rather than the inconsistent archived generic man page.

Keep returned ACLs opaque and owned by a null-checked RAII allocation freed exactly
once. Borrow File throughout each get/set; transfer no raw descriptor ownership.
Distinguish absent ACL from allocated empty ACL. Only native no-ACL ENOENT on a
still-valid held file becomes absence; unsupported, access and allocation failures
propagate. Apply the captured native object directly. Compare only OS-produced
acl_copy_ext encodings: check signed acl_size, a generous 1 MiB bound, aligned
zero-initialized storage and exact returned size. No ACL/ACE parser, layout cast,
text translation or imported binary decoder is introduced. Apple acl_translate.c
zeroes the external encoding and includes ordered entries, identities and flags.
Clear directory ACLs with an owned acl_init(0) object, then require descriptor
readback to be empty before payload creation. Prove actual getter/setter behavior,
absence/empty/error handling, source/stage/directory identity, write-only capture,
all four substitutions and normal ACL inheritance on both architectures and all
four controlled volume formats before accepting this exception.

Native ACL authorization probes request O_WRONLY without O_APPEND/O_TRUNC for a
granted write right. Darwin treats append as a separate right; a sibling direct
creation control proves write access and append denial before comparing actual
stage/final ACLs and inode continuity. A shared-directory/no-inheritance/same-owner
case demonstrates the unsafe following setter and the protected replacement, with
outside ACL and byte preservation. These do not promise an atomic defense against
arbitrary concurrent ancestor substitution.

The former no-follow path option still had a real-object substitution gap, and
Darwin's acl_set_link_np can lstat and then call the following setter. Descriptor
routines replace that unsafe assumption. A four-case native causal fixture replaces
an admitted file/directory name with either a symlink or a different real object;
the path setter mutates the foreign ACL, while descriptor reads/writes must preserve
foreign policy and bytes and target only the retained original inode. This route
requires native proof on both supported architectures and all four controlled volume
formats; unsupported descriptor ACL operations refuse without a pathname fallback.
It does not claim universal concurrent ancestor protection for publication/cleanup.

Controlled macOS volumes enable ownership on both mounts and place the disposable
outer fixture beneath traversable /private/tmp. Disk/ancestor diagnostics and a
nobody traversal control precede access assertions. Preserve the same inherited-ACL
baseline, protected publisher and new-file positives; do not treat a fixture setup
failure as proof that a platform lacks the vulnerability or that the fix passed.

Windows failure cleanup clears a copied readonly attribute only through the verified
owned staging handle and rechecks its identity before automatic removal. This keeps
cleanup independent of optional POSIX-delete support, while preserving the original
destination and outside hardlinks. Failed normalization retains the stage explicitly.
Then mark only that original DELETE-capable handle for classic deletion with the
canonical FILE_DISPOSITION_INFO, and disable path cleanup after success. A protected
source policy can deny a fresh pathname DELETE despite the creator's retained right;
never reopen the stage name or leave NamedTempFile drop to bypass that distinction.
Changed identity or failed handle cleanup retains the stage with its original cause.

### Refuse unsupported existing Windows EFS replacement

EFS policy includes file encryption keys and authorized recipients, independently of
DACL/SACL policy. The pinned standard library forwards its full attribute word, but
the native full-policy publisher control still returns success while losing EFS;
successful FILE_BASIC_INFO preparation is not preservation proof. Do not attempt
flag copying, re-encryption with a fresh key, key-list synthesis or decryption.
Check the retained admitted file's FILE_ATTRIBUTE_ENCRYPTED during capture before
staging/security preparation, and again during final retained-source verification.
Refuse this unsupported existing managed replacement with an EFS-specific cause.
Create-only operations return not-created for an already present direct regular
file before replacement-policy capture; they never need EFS-copy rights. A native
no-clobber race also preserves its encrypted winner. Existing alias/nonregular
refusals remain unchanged. Only the replacement source is restricted: SQLite/in-place explicit custom output,
source indexing, root selection and new outputs remain supported. Genuinely new
managed files in encrypted directories retain native EFS inheritance.

Use canonical windows-sys Win32_Storage_FileSystem metadata constants; this adds no
new native call or package version. Causal proof compares a former permissions-only
stage with guarded publication, both caller privilege modes and all six existing
replacement roles. Preserve original/outside hardlink identity, bytes, encryption
and directory entries; require the EFS-specific refusal before the stage callback.
Try late native encryption while the source snapshot is retained, verify the flag
actually changed before asserting the final refusal, or report the exact native
mutation limitation without claiming that branch executed. Prove generated-host
and managed-document creation in an encrypted parent plus custom-output positives.
A marker and same-user readback do not establish authorized-key-list fidelity.

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

Controlled macOS fixture teardown must establish image/device/mount ownership
before its first detach, verify successful detach, and revalidate complete ownership
and unmounted state before every bounded non-force busy-device retry. Changed or
uncertain state latches failure and retains the fixture; EXIT cleanup must not retry
an already failed teardown. Causal models cover false-success and foreign ownership
alongside normal/partial-unmount and transient/permanent-busy controls. Real hosted
native completion remains necessary. Hosted Rust checks retain all gates with
limited debug information to avoid independent executable-fixture copies exhausting
disk; this does not change optimization, assertions or test selection.

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
hard-link counts; native binding exceptions are the macOS ACL owner and the tiny
Windows descriptor getters and SDK empty-ACL initialization above; do not add unrelated unsafe bindings or
unstable std APIs. Ordinary
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
