## Context

`03-Auto-Release` reads the Cargo workspace version on each push to `main` and historically tried to exclude a non-SemVer `.dev` spelling. `02-Release` currently validates only stable `vMAJOR.MINOR.PATCH`, derives the checklist milestone by appending `-00` to that raw tag, and creates a normal GitHub release. Those decisions are duplicated workflow policy and cannot represent `v0.4.5-rc1` without either rejecting it, looking for `v0.4.5-rc1-00`, or replacing the stable Latest release.

The repository already uses Python standard-library policy scripts with `--self-test` entrypoints in CI. Release jobs run on Ubuntu with Python and authenticated `gh`; no product runtime or database change is needed.

## Goals / Non-Goals

**Goals:**

- Keep one exact stable/RC admission and milestone-classification rule across auto-dispatch and publication.
- Make GitHub release classification explicit, idempotent, and verified after publication.
- Keep RC and final release notes cumulative from the preceding stable release for any release series.
- Preserve exact prerelease identity through parser assets and installer downstream-pin checks.
- Preserve the full existing package, parser-pack, installer, exact-head, and stable-release contracts.
- Stop automated dependency PRs from targeting the retired `dev` release line.

**Non-Goals:**

- General SemVer prerelease parsing, arbitrary channels, or a reusable versioning framework.
- Any Rust crate, API, schema, index, MCP, or product-runtime change.
- Publishing final `v0.4.5`, changing the default branch, deleting `dev` while public links still depend on it, or admitting routine dependency upgrades to v0.4.5.

## Decisions

### Use one small standard-library policy script

Add one repository-owned Python script that accepts either a workspace version or `v`-prefixed release tag, recognizes only stable and `-rcN` with `N >= 1`, derives the normalized tag, base version, stable milestone, prerelease boolean, and publication eligibility, and writes validated GitHub step outputs. The same importable classifier is reused by release-note and optional-parser scripts. It owns a generic self-test table for stable, RC, development, and malformed inputs; no release number is embedded as policy.

This reuses the repository's existing CI-script pattern and avoids duplicated shell regular expressions across two workflows and several jobs. A Rust helper, dependency, action, or generic SemVer abstraction would add a build/runtime boundary for policy used only by GitHub Actions.

### Derive classification, never accept a manual prerelease flag

`02-Release` derives prerelease state and `vMAJOR.MINOR.PATCH-00` from the requested version. Manual callers cannot independently select stable/prerelease state or an RC-specific milestone. `03-Auto-Release` uses the same classifier; stable and RC versions are eligible, canonical Cargo `MAJOR.MINOR.PATCH-dev.N` versions are explicitly ineligible, and other forms fail closed.

### Verify existing and newly created GitHub release state

Publication runs are serialized by the exact requested version, and publishing—not branch-safe prepublication proof—requires the workflow checkout to equal both the event SHA and current `origin/main`. The publish job repeats that check immediately before its first release mutation so a long package build cannot publish a head that `main` has since passed. Before asset repair, an existing release must have the exact requested tag, be non-draft, have the expected prerelease boolean, and resolve to the workflow head. Before publication, the workflow captures the current Latest stable tag. A new RC is created with `--prerelease --latest=false`; a new stable release is created without prerelease flags. After create or repair, the workflow re-reads the release record and tag head. New RCs and every repair require GitHub's `/releases/latest` endpoint to identify a canonical stable ProjectAtlas tag and remain exactly on that captured tag; only a newly created stable release is required to become Latest.

This keeps repair idempotent without silently changing classification. The endpoint check verifies the user-visible repository contract rather than assuming GitHub's create semantics.

### Make RC-first promotion executable

Publication of an RC fails when the corresponding stable tag already exists. Publication of a final stable release queries every GitHub release page, selects the highest canonical non-draft prerelease with the same stable base, requires its tag to exist in the checkout, and proves that candidate tag is an ancestor of the final head. Prepublish-only package proof remains usable without hosted state because it cannot create a release. This makes “RC first” a version-agnostic admission rule instead of guidance or a `0.4.5` special case.

Automatic handoff resolution compares prior clean parser proof with the exact pushed `main` commit. It does not assume a second merge parent, so the repository's allowed merge-commit, squash, and rebase histories share the same input-equivalence rule. The labeled optional-parser proof trigger follows pull requests to `main`.

### Use the preceding stable tag as the promotion-series notes baseline

Release notes ignore RC tags when selecting history. An RC and the final stable release both select the greatest stable tag lower than their base version, then summarize that stable tag through the candidate head. Later RCs therefore remain cumulative, and the final stable notes include every change introduced during all RCs. This is computed from arbitrary accepted version tuples rather than a hard-coded current release number.

### Carry the exact accepted version through release artifacts and installer pins

Optional-parser verification uses the shared classifier and requires proof/package identity plus filenames to retain the complete accepted tag. The existing POSIX and PowerShell installer pin scanners recognize the same optional `-rcN` suffix, so a workflow correctly pinned to an installed RC is not reported stale. No product-runtime version abstraction is added.

The release adapter also owns a read-only discovery hook for the separately produced clean-main Atlas seed. Absence is valid until that producer participates in the release handoff. If any `projectatlas-main-atlas-seed-` asset is present locally or on a repair target, the hook requires one regular archive and manifest with the shared basename `projectatlas-main-atlas-seed-<exact-tag>-<snapshot-digest>`, rejects malformed, other-version, or multi-digest inventories, and includes the complete pair in `SHA256SUMS`. Byte-identical hosted seed assets are retained rather than clobbered; any mismatch fails before release mutation. An interrupted first upload may leave one hosted member only when a retry supplies the complete validated pair and that member has the expected exact name and bytes; the retry uploads only the missing companion. The hook never creates, opens writable, hydrates, or interprets the seed; those responsibilities remain with the seed producer and runtime owned by issue #430.

### Retarget automation without merging dependency scope

Dependabot targets `main`. Existing automated PRs against `dev` are closed with an explicit disposition after review-thread inspection; their branches may be removed once GitHub confirms closure. No dependency version is changed in this issue.

### Make architecture evidence executable IssueOps policy

Reuse the existing IssueOps architecture-link validator. An open mapped issue may either provide repository-local `dev/docs/*.md#heading` links or write exactly `N/A: <reason>` with a non-empty reason. Every linked heading owns a section through the next same-or-higher heading; that section must contain a closed `mermaid` fence whose body passes the repository-locked canonical Mermaid syntax parser. Headings, prose, declaration-only, empty, syntactically invalid Mermaid, and other code fences do not satisfy the contract. CI installs the parser once from its lockfile without install scripts; the validator starts only the local parse API, not a browser renderer or runtime download.

The final task in every mapped OpenSpec checklist uses one standard acceptance sentence requiring the finished implementation and architecture diagrams to agree, or the reasoned `N/A` to be reconfirmed. Syntax is machine-checked by the locked parser; visual communication and semantic truth remain the final acceptance review rather than a separate evidence ledger.

### Gate milestone planning at the issue transition

The issue-event workflow runs the same IssueOps validator when an open issue is assigned to or edited within a canonical release milestone. Planning requires exactly `status:ready`, a local OpenSpec mapping, proposal/design/delta-spec/tasks artifacts, the standard design sections, explicit dependencies or a reasoned `N/A`, resolved open questions, at least one requirement and scenario, and checked `1.x` contract/specification tasks. The ordinary mapped-issue pass continues to enforce synchronized tasks, the concise issue shape, architecture evidence, and final reconciliation task. Release-time milestone completion remains the stricter all-issues-closed gate.

This checks the transition that creates planned scope without scanning unrelated future milestones on every source pull request or adding a separate validation service.

## Risks / Trade-offs

- **GitHub changes Latest behavior** -> Verify both release metadata and the live Latest endpoint; fail publication if the invariant is not true.
- **A typo on `main` silently skips a release** -> Only canonical Cargo `MAJOR.MINOR.PATCH-dev.N` is a supported ineligible state; other unsupported versions fail the auto-release job.
- **Repair uploads assets to a misclassified release** -> Compare exact tag head and prerelease state before upload.
- **Concurrent repair runs interleave mutable assets and checksums** -> Serialize the workflow by exact requested version without cancelling an in-flight repair.
- **A manual publish runs from stale or non-main source** -> Require the event SHA, checkout commit, and fetched `origin/main` commit to match before publication.
- **A stable version bypasses candidate validation or an RC appears after final** -> Require the highest published same-base RC on the stable head, and refuse RC publication once the stable tag exists.
- **RC notes omit earlier candidate work or include all repository history** -> Select the prior stable tag by parsed version and ignore RC tags as baselines.
- **RC asset or downstream pin identity is truncated** -> Reuse the classifier in parser verification and test the existing installer scanners with exact RC pins.
- **A partial or wrong-version stable-main seed is published** -> Fail release staging when any discovered seed-prefixed inventory is not one exact-tag archive/manifest pair, and checksum both validated files.
- **The policy script becomes a version framework** -> Keep the accepted grammar fixed to stable and `-rcN`, expose only workflow outputs, and use no dependency.
- **Retargeted dependency PRs expand v0.4.5** -> Keep them outside the milestone and close/defer them until after the RC scope.
- **Architecture sections contain prose or stale diagrams** -> Require a real Mermaid declaration or reasoned `N/A`, then make final implementation-to-diagram reconciliation the last OpenSpec task.
- **IssueOps accepts Mermaid-looking prose or downloads an unpinned parser** -> Parse every candidate with the lockfile-owned Mermaid package installed once with scripts disabled.
- **An aspirational issue is assigned to a release before its design is usable** -> Fail the issue-event gate unless its mapped artifacts, contract tasks, dependencies, open questions, and readiness label satisfy the implementation-ready contract.

## Migration Plan

1. Land the classifier, cumulative release-note selection, parser/installer and optional seed-asset compatibility checks, focused self-tests, workflow integration, Dependabot retarget, IssueOps mapping, and release guidance on `main` before setting the workspace version to `0.4.5-rc1`.
2. Close obsolete `dev`-targeted automated PRs and prune only their closed or merged heads.
3. Let `03-Auto-Release` dispatch `02-Release` when the verified v0.4.5 candidate reaches `main`.
4. After every milestone implementation issue is closed, run the separate release operation and confirm the candidate is a prerelease at the exact head while `/releases/latest` remains on the preceding stable release.
5. Roll back by reverting the workflow/policy change before any later tag; an already published RC remains a prerelease and does not require deleting or rewriting release history.

## Dependencies / Cross-Issue Impact

Issue #448 owns release and IssueOps policy. It does not implement #430 worktree continuity or #440 classified documentation navigation, but it prevents either issue from remaining planned in a release milestone until its own mapped OpenSpec and cross-issue handoff are implementation-ready.

## Open Questions

None.
