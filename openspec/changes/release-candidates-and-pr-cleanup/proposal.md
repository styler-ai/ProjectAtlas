## Why

ProjectAtlas cannot currently publish `v0.4.5-rc1` truthfully: the release workflow rejects RC tags and would create any accepted version as a normal release, allowing a candidate to displace `v0.4.4` as GitHub Latest. The same repository state still routes Dependabot to the obsolete `dev` line, creating failed PR clutter outside the focused v0.4.5 scope.

## What Changes

- Accept exactly stable `vMAJOR.MINOR.PATCH` and release-candidate `vMAJOR.MINOR.PATCH-rcN` inputs, with `N >= 1`.
- Derive the owning stable `vMAJOR.MINOR.PATCH-00` milestone for both stable and RC releases.
- Create and repair RC releases only with GitHub prerelease metadata, and verify an RC never becomes the Latest stable release.
- Refuse a final stable publication until a non-draft RC of the same base version is published on its history, and refuse a late RC after that base version's stable tag exists.
- Preserve existing stable publication, exact-head, parser-pack handoff, package, installer, and recovery behavior.
- Keep automatic release dispatch for stable and RC workspace versions while excluding development or unsupported prerelease versions.
- Generate every RC and final stable release note set cumulatively from the preceding stable tag, so promotion never drops work introduced during an RC.
- Preserve the complete RC suffix in optional-parser release assets and downstream workflow pin checks.
- Accept an optional clean-main Atlas seed archive/manifest pair from its separately owned producer, require the exact release tag in both content-addressed filenames, and checksum both without creating or opening the seed.
- Retarget Dependabot to `main` and disposition obsolete automated PRs against `dev` without admitting dependency upgrades to v0.4.5.
- Hard-gate each open mapped issue so `Architecture Diagrams` either links to a real Mermaid diagram in the named local documentation section or records a reasoned `N/A`, and require the final OpenSpec task to reconcile that decision with the finished implementation.
- Reject release-milestone planning unless the open issue is `status:ready`, mapped to complete OpenSpec proposal/design/delta-spec/tasks artifacts, has checked contract tasks, resolved open questions, and explicitly records cross-issue impact.
- Document and test a version-agnostic prerelease-to-stable promotion contract; `0.4.5` is release proof, not policy embedded in reusable logic.
- Non-goals: arbitrary prerelease channels, hard-coded release-series policy, a versioning framework or release-version dependency, a separate release service, a new long-lived release branch model, the final v0.4.5 publication, or routine dependency upgrades in v0.4.5.

This change is ready for implementation as issue #448 in the v0.4.5 milestone.

## Capabilities

### New Capabilities

- `release-candidate-promotion`: Stable and RC version admission, GitHub prerelease/Latest classification, milestone routing, recovery validation, automatic dispatch, and current-branch dependency-update routing.

### Modified Capabilities

None.

## Impact

- `.github/workflows/release.yml` and `.github/workflows/03-auto-release.yml`
- One small repository-owned, version-agnostic release classifier shared by workflows, release notes, and optional-parser verification
- `.github/scripts/release-notes.py`, optional-parser and optional main-seed release-asset validation, and installer downstream-pin checks
- `.github/dependabot.yml`
- Release architecture/guidance, IssueOps architecture/readiness validation, its issue-event workflow, and IssueOps mapping
- One repository-locked Mermaid parser used only by IssueOps syntax validation
- GitHub releases, the Latest endpoint, milestones, and obsolete automated PR heads
- No Rust product API, crate boundary, database schema, runtime behavior, or product dependency
