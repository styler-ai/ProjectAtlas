# Proposal

## Why

Installed 0.5.0 can create automatic state outside its intended owner during both
initialization and ordinary database-backed navigation. SQLite read-only opens may
initialize WAL shared memory before root metadata is checked; bootstrap can write
default configuration and host files before a later destination refusal.
This confirmed self-hosting/security-triage finding is required for RC1 as #741.

## What Changes

- Admit conventional generated databases and WAL sidecars before every SQLite open,
  including root discovery and read-only navigation; admit bootstrap outputs before writes, using
  existing bootstrap/filesystem owners and the selected canonical project root.
- Cover ordinary CLI/MCP init, registered-worktree hydration, root bind/move/detach and generated host files;
  preserve identity, existing authored configuration/purposes and valid reinitialization.
- Distinguish automatic project outputs from intentionally selected external database,
  configuration and source-root paths. Preserve those supported operator capabilities.
- Prevent a generated file write from following an unsafe pre-existing leaf alias;
  preserve valid files and the last valid state on refusal or failed publication.
- Protect the same implicit destinations in config ignore edits, root `.gitignore`
  creation and the default JSON map sidecar. Preserve custom/external config and map
  outputs, existing-entry Gitignore no-ops and read-only config aliases.
- Recognize macOS filename and directory case aliases, including Unicode folding,
  from the actual destination volume; preserve distinct custom names on sensitive
  volumes and resolve missing-parent reductions across mount boundaries before writes.
- Classify Linux fixed-name aliases using the actual containing directory's native
  casefold flag and filesystem model. Match mounted ext4/f2fs Unicode 12.1 behavior,
  preserve distinct sensitive-directory custom names, and refuse uncertain ownership
  for potential aliases on unrecognized models before side effects.
- Trace analogous producer/write/admission paths and prove positive compatibility,
  causal failures and real installed CLI/MCP behavior on supported platforms.
- Preserve existing access-control lists when replacing an admitted generated host
  config, managed TOML config or JSON sidecar, including native inheritance policy
  and effective ownership. Refuse before publication if preservation cannot be proved;
  keep ordinary regeneration, outside-hardlink safety and owned failure cleanup.
- Preserve existing Linux mandatory file labels as well as discretionary ACLs.
  Snapshot access policy through the admitted file object, and prepare changed
  policy on an anonymous stage before exposing a pathname. Protect macOS replacement
  stages from inherited ACL grants before opening payload data; retain normal
  direct-parent inheritance when creating genuinely new outputs.
- Refuse replacement of fs-verity-sealed Linux outputs before staging and recheck
  the retained source before publication; retain ordinary replacement and explicit paths.
- Refuse existing EFS-encrypted Windows managed replacements through retained
  metadata before staging and recheck before publication until complete native
  encryption-policy preservation is supported. Preserve encrypted bytes/outside
  hardlinks, normal encrypted-directory creation and explicit custom paths.
- Protect Windows replacement stages at creation from broader inherited access
  and implicit owner policy-changing rights. Later DACL tightening cannot revoke
  previously acquired handles; preserve retained creator rights and ordinary
  new-output inheritance without weakening destination policy.

## Capabilities

### New Capabilities

- `project-state-ownership`: admission of automatic database access and safe project-state
  creation for initialization and generated host files.

### Modified Capabilities

None. The canonical capability inventory contains only token-impact-estimate-reporting.
This adds no source semantic provider, index schema or CLI/MCP wire field.

## Impact

Existing CLI/MCP database selection and root discovery, normal read/write store
wrappers and early metadata reads, CLI init and generated MCP-config writers, shared runtime bootstrap,
atlas_map default/nonsource creation, MCP registered-worktree initialization and
hydration activation, existing filesystem/project-binding helpers, native init tests,
installed selector inventory and #715 release acceptance. Reuse existing same-file/tempfile helpers and the already locked canonical
winapi-util as a direct Windows-only dependency for stable hard-link counts and
whichdisk as a direct macOS-only dependency for existing volume capabilities; add
no schema change. A narrowly reviewed macOS-only native ACL binding is required
because the installed safe path wrapper has no descriptor API and a native test
disproves the /dev/fd proxy approach. Native ACL preservation additionally uses the
already locked Linux xattr API and Windows-only windows-permissions; macOS exacl
remains an independent path-based test oracle, without a production dependency;
the latter needs an exact reviewed bitflags 1 dependency exception. APFS expanding
aliases use pinned Unicode 9 tables from unicode-casefold 0.2.0. These additions
require dependency gates and native behavior proof before acceptance. Anonymous
Linux staging and non-following source snapshots reuse the already locked rustix
safe filesystem APIs; macOS also uses its effective-user-ID accessor.
Windows uses the already locked official windows-sys 0.61.2 for canonical security
getters, SDK empty-ACL initialization and encrypted-file metadata constants.

Windows SACL preservation requires a second narrowly reviewed native getter:
GetSecurityDescriptorControl, missing from the canonical safe wrapper. Preserve
system policy through retained handles when full rights are already available;
otherwise prove source and stage have no SACL or refuse replacement before bytes.
No global privilege enablement or blanket elevation requirement is introduced.

## Release Scope

High-priority #741 is required for v0.5.1-rc1, directly under and blocking #715 in
v0.5.1-00. It starts from accepted main, now including the merged #731 corrections, and has no
direct implementation blocker; #738/#739/#708 must be combined before release proof.
Ready for implementation only after independent plan review, strict OpenSpec,
scoped IssueOps and the synchronized native milestone/parent/blocker graph.

## Non-Goals

No blanket denial of explicit external config/database paths, selected-root aliases,
Git metadata or normal indexing/plugins; no schema reset, new path framework,
scanner-driven speculative restrictions or stable publication. Do not claim a
universal defense against a hostile process racing every ancestor in the filesystem.
