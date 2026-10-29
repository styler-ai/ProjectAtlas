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

### Explicit paths remain supported

An explicitly selected external database or config path is an intended operator
capability and still follows its existing identity/configuration rules. A selected
project root may itself be a canonicalizable alias. These positive paths must work
alongside automatic-state refusal. Existing config read aliases that are not written
must not be confused with unsafe generated-output writes. Avoid converting intentional
filesystem selection or Git-policy reads into this finding.

### Keep initialization adapters and refresh paths consistent

CLI/MCP and registered-worktree hydration must use the same owning admission before
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
