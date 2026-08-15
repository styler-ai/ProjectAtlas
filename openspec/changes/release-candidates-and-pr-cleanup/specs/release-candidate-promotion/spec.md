## ADDED Requirements

### Requirement: Release versions have one bounded stable or RC contract
The release system SHALL accept exactly `vMAJOR.MINOR.PATCH` stable tags and `vMAJOR.MINOR.PATCH-rcN` release-candidate tags where each numeric component is canonical decimal notation and RC `N` is a positive decimal integer without leading zeroes. It SHALL derive the same classification from an unprefixed Cargo workspace version, treat exactly `MAJOR.MINOR.PATCH-dev.N` with canonical non-negative `N` as publication-ineligible development state, operate for any accepted version tuple without hard-coded release-series values, and reject every other prerelease spelling.

#### Scenario: Stable version is accepted
- **WHEN** the classifier receives `0.4.5` or `v0.4.5`
- **THEN** it reports normalized tag `v0.4.5`, stable classification, and publication eligibility

#### Scenario: Release candidate is accepted
- **WHEN** the classifier receives `0.4.5-rc1` or `v0.4.5-rc1`
- **THEN** it reports normalized tag `v0.4.5-rc1`, prerelease classification, and publication eligibility

#### Scenario: Development version is not published
- **WHEN** automatic release classification receives canonical Cargo workspace version `1.2.3-dev.1`
- **THEN** it reports publication ineligible without dispatching a release

#### Scenario: Unsupported version fails closed
- **WHEN** the classifier receives `v0.4.5-rc0`, `v0.4.5-rc01`, `v0.4.5-beta1`, build metadata, missing components, or extra text
- **THEN** it returns a nonzero error and no release outputs

### Requirement: Stable and RC releases share the stable milestone
The release system MUST derive `vMAJOR.MINOR.PATCH-00` as the owning IssueOps milestone for both the stable tag and every RC of that version.

#### Scenario: RC milestone routing
- **WHEN** `v0.4.5-rc1` enters the release checklist gate
- **THEN** the gate validates milestone `v0.4.5-00` rather than an RC-named milestone

#### Scenario: Stable milestone compatibility
- **WHEN** `v0.4.5` enters the release checklist gate
- **THEN** the gate continues to validate milestone `v0.4.5-00`

### Requirement: GitHub publication preserves prerelease and Latest truth
The release workflow SHALL serialize runs by exact requested version, SHALL publish only from the exact current `origin/main` head, SHALL create RC tags as non-draft GitHub prereleases with explicit Latest exclusion, and SHALL create stable tags as non-draft normal releases. A stable publication MUST require a previously published non-draft RC of the same base version on the stable head, and an RC publication MUST be refused after the same base version's stable tag exists. After create or repair it MUST verify the exact release tag, draft state, prerelease classification, tag head, and repository Latest endpoint before reporting publication success.

#### Scenario: Same-version publication is serialized
- **WHEN** two create or repair runs request the same exact stable or RC version
- **THEN** the later run waits without cancelling the in-flight run and cannot interleave its archives or checksums

#### Scenario: Non-main or stale publication is refused
- **WHEN** a publishing workflow checkout differs from its event SHA or current `origin/main`
- **THEN** publication fails before creating, repairing, or tagging a release while branch prepublication proof remains available

#### Scenario: Final stable release requires a prior RC
- **WHEN** a stable release is requested without any published non-draft RC of the same base version on its history
- **THEN** publication fails before creating or repairing the stable release

#### Scenario: Stable promotion follows the highest published RC
- **WHEN** several RC releases exist for the same base version
- **THEN** the highest canonical RC tag must exist and be an ancestor of the stable release head

#### Scenario: RC after final is refused
- **WHEN** an RC is requested after the same base version's stable tag already exists
- **THEN** publication fails without creating or repairing the RC

#### Scenario: RC publication keeps the previous stable release Latest
- **WHEN** `v0.4.5-rc1` is created or repaired while `v0.4.4` is the current stable release
- **THEN** the RC record is marked prerelease and GitHub's Latest endpoint does not identify `v0.4.5-rc1`

#### Scenario: RC publication preserves the exact previous Latest stable tag
- **WHEN** an RC is created or repaired and the workflow captured the current Latest tag before publication
- **THEN** GitHub's Latest endpoint remains exactly that captured tag after publication

#### Scenario: Published RC completes the installed agent workflow
- **WHEN** an RC is published and the Linux installer smoke accepts its exact hosted artifact
- **THEN** the RC remains a non-Latest prerelease, indexes a real project, serves fresh brief and source evidence through its generated stdio MCP configuration, and the release workflow succeeds only after the remaining platform smokes also pass

#### Scenario: New stable publication becomes Latest
- **WHEN** final `v0.4.5` is newly created after its milestone is complete
- **THEN** the release record is not prerelease and GitHub's Latest endpoint identifies `v0.4.5`

#### Scenario: Existing release repair preserves Latest
- **WHEN** an existing stable or RC release is repaired while another stable release is the captured Latest release
- **THEN** GitHub's Latest endpoint remains exactly on that captured release

#### Scenario: Repair requires an observable canonical Latest release
- **WHEN** an RC publication or existing-release repair cannot observe a canonical stable tag from GitHub's Latest endpoint before mutation
- **THEN** publication fails before creating, repairing, or uploading release assets

#### Scenario: Existing release classification mismatch is refused
- **WHEN** asset repair finds an existing release whose prerelease state differs from the version-derived state
- **THEN** the workflow fails before uploading or replacing assets

#### Scenario: Draft recovery target is refused
- **WHEN** asset repair finds an existing draft release for the requested tag
- **THEN** the workflow fails before uploading or replacing assets

#### Scenario: Existing tag head mismatch remains refused
- **WHEN** create or repair finds the requested tag at a commit other than the workflow head
- **THEN** the workflow fails without moving the tag or mutating the release

#### Scenario: Prepublication milestone gating remains acyclic
- **WHEN** a mapped implementation issue is validated before release publication
- **THEN** its tasks require generic and prepublication proof only, while exact hosted-release verification remains a separate post-closure release operation

### Requirement: Automatic release dispatch preserves verified handoff behavior
`03-Auto-Release` SHALL dispatch `02-Release` for a new eligible stable or RC workspace version only after the clean optional-parser handoff is input-compatible with the exact pushed `main` commit. It SHALL not dispatch a supported development version or an already tagged version.

#### Scenario: New RC dispatch
- **WHEN** `0.4.5-rc1` lands on `main`, its tag is absent, and the exact parser-pack handoff is valid
- **THEN** auto-release dispatches `02-Release` with version `v0.4.5-rc1` and the verified handoff run ID

#### Scenario: Stable dispatch compatibility
- **WHEN** a stable workspace version lands under the same verified conditions
- **THEN** auto-release preserves the existing stable dispatch behavior

#### Scenario: Allowed integration histories share one handoff rule
- **WHEN** an eligible version reaches `main` through an allowed merge commit, squash merge, or rebase merge
- **THEN** auto-release compares clean parser proof with the exact pushed commit without requiring a second parent

#### Scenario: Ineligible or existing version does not dispatch
- **WHEN** the workspace version is the supported development form or its normalized tag already exists
- **THEN** auto-release does not resolve a handoff or dispatch publication

### Requirement: Release notes remain cumulative through candidate promotion
For any accepted release series, the release-note generator MUST use the greatest preceding stable tag as its history baseline and MUST ignore RC tags as baselines. Every RC and the final stable release in that series SHALL therefore include all changes since the preceding stable release.

#### Scenario: First RC uses the preceding stable release
- **WHEN** the first RC of a release series is generated after a preceding stable tag
- **THEN** its notes cover the preceding stable tag through the RC head rather than all repository history

#### Scenario: Later RC remains cumulative
- **WHEN** a later RC is generated and earlier RC tags exist for the same series
- **THEN** its notes still use the preceding stable tag and include changes from the earlier RCs

#### Scenario: Final promotion includes all RC work
- **WHEN** the final stable release is generated after one or more RC tags for the same series
- **THEN** its notes use the preceding stable tag and include every change introduced during the RC sequence

#### Scenario: Existing stable-to-stable behavior remains compatible
- **WHEN** a stable release is generated without any RC tags in its series
- **THEN** its notes continue to use the preceding stable tag

### Requirement: Release artifact identity remains exact
Optional-parser release verification and installer downstream-pin checks MUST preserve and compare the complete accepted version tag, including an `-rcN` suffix when present. Release staging SHALL accept an optional immutable clean-main Atlas seed only as a complete archive/manifest pair whose shared content-addressed basename contains that same exact tag, and SHALL include both files in release checksums without taking ownership of seed creation or hydration.

#### Scenario: RC parser assets retain exact identity
- **WHEN** an optional-parser package and proof are staged for an RC
- **THEN** their declared version and filenames retain the complete RC tag and a stable proof cannot be relabeled as that RC

#### Scenario: Exact RC workflow pin is current
- **WHEN** an installed RC inspects a downstream ProjectAtlas workflow pinned to its complete RC tag
- **THEN** neither the POSIX nor PowerShell installer reports that exact pin as stale

#### Scenario: Stale and unrelated pins remain classified correctly
- **WHEN** downstream files contain an older ProjectAtlas tag and a tag belonging to another repository
- **THEN** the older ProjectAtlas tag is reported stale and the unrelated repository tag is ignored

#### Scenario: Exact stable or RC seed pair is included
- **WHEN** the separately owned producer supplies `projectatlas-main-atlas-seed-<tag>-<snapshot-digest>.tar.zst` and the matching `.manifest.json` for the requested stable or RC tag
- **THEN** release staging accepts both immutable files and records a SHA-256 checksum for each

#### Scenario: Seed production remains optional and separately owned
- **WHEN** no main Atlas seed asset is present because the producer has not joined the release handoff
- **THEN** the release adapter continues without fabricating a seed or weakening the ordinary archive gates

#### Scenario: Partial or wrong-version seed inventory fails closed
- **WHEN** a local seed pair is partial, or a hosted seed asset has a different tag, malformed digest, symlink, second snapshot digest, byte mismatch, or no complete exact staged pair for interrupted-upload recovery
- **THEN** release staging fails before creating or repairing the GitHub release

#### Scenario: Hosted seed repair is immutable
- **WHEN** an existing release already contains the exact validated seed pair
- **THEN** staging requires byte equality and repairs only replaceable assets without clobbering either seed object

#### Scenario: Interrupted seed-pair upload is recoverable
- **WHEN** an existing release contains exactly one expected immutable seed member and a retry supplies the complete validated byte-identical pair
- **THEN** the retained member is not clobbered and the retry uploads only its missing companion

### Requirement: Dependency automation follows the active release branch
Repository dependency automation SHALL target `main`. Obsolete automated PRs targeting `dev` MUST be explicitly dispositioned without merging their dependency changes into v0.4.5.

#### Scenario: Future dependency update routing
- **WHEN** Dependabot creates a Cargo or GitHub Actions update after this change lands
- **THEN** its base branch is `main`

#### Scenario: Existing obsolete PR cleanup
- **WHEN** a `dev`-targeted automated PR has no unresolved review feedback and is outside v0.4.5 scope
- **THEN** it is closed with a reason and its head may be pruned without changing dependency versions

### Requirement: IssueOps hard-gates architecture evidence and final reconciliation
For every open mapped issue, IssueOps MUST require the `Architecture Diagrams` section either to contain local `docs/*.md#heading` links on `main` whose exact linked sections contain closed fenced Mermaid blocks accepted by the repository-locked Mermaid syntax parser, or to contain exactly `N/A: <reason>` with a non-empty reason. A heading, prose, an empty, declaration-only, malformed, syntactically invalid Mermaid, or another code-fence type SHALL NOT satisfy the diagram path. The final OpenSpec task MUST require comparison of the finished implementation with the diagrams, correction of either until they agree, or reconfirmation of the reasoned `N/A`.

#### Scenario: Linked section contains a real Mermaid diagram
- **WHEN** an architecture link resolves to a local Markdown heading whose owned section contains a closed `mermaid` fence beginning with a diagram declaration and followed by actual diagram content
- **THEN** IssueOps accepts the architecture evidence

#### Scenario: Prose-only architecture section fails
- **WHEN** the linked heading exists but its section has only prose, an empty or declaration-only Mermaid fence, an ordinary code fence, or syntactically invalid Mermaid content
- **THEN** IssueOps fails the mapped issue

#### Scenario: No architecture change is a conscious decision
- **WHEN** the issue records `N/A: <reason>` and no architecture diagram is needed
- **THEN** IssueOps accepts the reasoned decision while rejecting bare or unexplained `N/A`

#### Scenario: Final architecture reconciliation is mandatory
- **WHEN** the local OpenSpec checklist does not end with the standard implementation-to-architecture reconciliation task
- **THEN** IssueOps fails the mapped issue before closure or release

### Requirement: Release milestone planning admits only implementation-ready issues
When an open issue is assigned to a canonical release milestone, IssueOps MUST require exactly `status:ready`, a local OpenSpec mapping, readable proposal/design/delta-spec/tasks artifacts, complete required proposal and design sections, explicit cross-issue impact or a reasoned `N/A`, resolved open questions, at least one delta-spec requirement and scenario, and checked `1.x` contract/specification tasks. The existing mapped-issue contract MUST additionally pass for synchronized tasks, issue shape, architecture evidence, and final reconciliation.

#### Scenario: Ready mapped issue can be planned
- **WHEN** an open issue satisfies every readiness artifact and issue-contract requirement before milestone assignment
- **THEN** the issue-event IssueOps gate accepts the planned release scope

#### Scenario: Unmapped or backlog issue cannot be planned
- **WHEN** an open issue has no local OpenSpec mapping or retains a non-ready status label
- **THEN** IssueOps fails its release-milestone planning event

#### Scenario: Incomplete specification cannot be hidden behind a ready label
- **WHEN** a ready-labeled issue is missing a required OpenSpec artifact or section, has unresolved open questions, omits cross-issue impact, lacks requirement scenarios, or has unchecked contract tasks
- **THEN** IssueOps fails the planning event
