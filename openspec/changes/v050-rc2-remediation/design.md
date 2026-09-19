## Context

`v0.5.0-rc1` was published from `24ef8332` and its release owner #492 then closed. The published audit demonstrated six new defects after that closeout. The current default branch contains the historical release state plus post-RC maintenance; the remediation must keep the existing seven-crate, SQLite, installer, release-workflow, and IssueOps owners intact.

## Goals / Non-Goals

**Goals:**

- Repair each reproduced boundary at its smallest existing owner, with a causal regression test.
- Reopen the existing v0.5.0 release graph rather than inventing a parallel version/milestone for the same unreleased final version.
- Re-run the complete installed-package proof only after all six owners merge to current `main`.

**Non-Goals:**

- Do not replace the skill with hook text, grant hook trust, create an alternate parser runtime, or relax containment.
- Do not introduce a generic hook, archive-name, launcher, or process-supervision framework.
- Do not promote `v0.5.0` stable or change GitHub Latest.

## Decisions

### Use the default bundled-hook discovery contract

ProjectAtlas will ship `hooks/hooks.json` plus one bounded session-instruction command and rely on the documented default discovery path. This avoids a manifest rewrite and gives Codex one reviewable/trustable hook definition. The hook emits only routing guidance on supported `SessionStart` variants; it never initializes, scans, writes a project, or repeats the full version-matched skill.

An inline manifest hook or a copied skill was rejected because default discovery already covers the installed plugin and the extra manifest layer would replace that default rather than extend it.

### Filter classified candidates before lookup

The classified file path will keep the existing classification/error handling for real indexed files but exclude inferred, non-indexed companion paths from the candidate set before classification is loaded. CLI and MCP keep one service result so filters, ranking, limits, and failure shape agree.

Ignoring every classification error was rejected because it could hide a corrupted real indexed row.

### Make `atlas` a direct Windows executable alias

The Windows installer will materialize the verified runtime as a collision-safe `atlas.exe` alias (through the existing atomic installer ownership path) rather than using `atlas.cmd` to reconstruct a command line. Windows therefore performs normal native argument parsing once, preserving JSON, empty values, Unicode, spaces, metacharacters, exit codes, and version identity.

Quoting patches to `%*`, per-subcommand workarounds, and a new forwarding binary were rejected: they retain cmd.exe parsing as a second argument boundary or duplicate the runtime.

### Treat final asset names as a packaging contract

Parser-pack archive validation will accept the exact versioned name produced by release staging for the selected target, while manifest, platform, digest, content, and containment checks retain their current ownership. Documentation uses the final filename and the normal `atlas` route.

### Diagnose containment progress at the owning phase

The Windows containment path must expose a bounded, causal phase result sufficient to distinguish launch, worker, broker, protocol, resource, timeout, and cleanup failure. The fix is selected only after RC1's installed-asset reproduction is explained. A failed admission leaves no selected slot and a retry remains valid.

Increasing a timeout or bypassing containment was rejected because both hide the failing contract.

### Reopen the existing release root

The `v0.5.0-00` milestone and #492 will reopen. Issues #604–#609 become its sole direct new children; #492 gains a second, unchecked RC2 acceptance slice and is blocked by all six. This planning PR maps its #604 task slice; each later issue introduces its own mapped implementation change from refreshed `main`, preserving accepted task authority. The release candidate is `v0.5.0-rc2`, non-draft and prerelease; v0.4.5 remains Latest. Every bug gets one implementation PR and a complete independent review before it closes.

## Risks / Trade-offs

- [A trusted hook still cannot run in a host that disables hooks] → Document the distinction and test discovery/output separately from trust state.
- [A launcher repair could overwrite an unrelated command] → Retain the current collision, ownership, atomic-replace, repair, and uninstall checks.
- [A filename broadening could admit unrelated archives] → Bind accepted names to the runtime release version and selected target, then retain all content checks.
- [Containment remains host-specific] → Report a typed actionable prerequisite only if the supported host boundary proves it; otherwise repair the demonstrated phase.
- [Release-state mutation drifts from local tasks] → Update the issue map, issue bodies, milestone, native parent/blocker relations, and PR-state proof together, then read them back.

## Migration Plan

1. Land the release graph/OpenSpec plan and reopen the existing v0.5.0 scope.
2. Implement and merge #604–#609 one by one against refreshed `main`, preserving unresolved task and acceptance state until the owning proof and review pass.
3. Bump release-owned identities to `0.5.0-rc2`, dispatch the exact-head parser-pack proof, and run the release workflow with the required clean parser-pack run.
4. Independently read back release metadata, assets, SHA256SUMS, exact installed runtime/plugin/skill/hook identity, Windows alias parity, parser lifecycle, and supported platform results. Leave stable promotion untouched.

## Dependencies / Cross-Issue Impact

This change establishes the RC2 contract and maps #604. It coordinates #605, #606, #607, #608, and #609 as direct #492 children whose implementation changes will map their own task slices from refreshed `main`. #609 remains blocked by #607 because it needs the final accepted parser-pack filename before its installed admission reproduction. #492 remains distinct from stable-promotion owner #602.

## Open Questions

None.
