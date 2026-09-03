## ADDED Requirements

### Requirement: Local candidate branches use owner-scoped checklist authority
ProjectAtlas SHALL validate one local candidate issue against its live GitHub checklist while comparing every unrelated open mapped task slice with the accepted base. Before content validation, the candidate issue map, every candidate-local mapped task file, and every linked architecture document SHALL be tracked regular files in the submitted candidate tree; linked architecture documents SHALL additionally be Markdown. Candidate validation SHALL parse each of those inputs from the exact UTF-8 blob in the submitted candidate tree rather than from mutable worktree bytes.

#### Scenario: Independent live progress does not invalidate a candidate
- **WHEN** the candidate owner's local tasks match its live issue and an unrelated issue's live tasks have advanced beyond the accepted base
- **THEN** validation succeeds when the candidate preserves that unrelated accepted-base slice unchanged

#### Scenario: Candidate owner differs from live tasks
- **WHEN** the candidate owner's local task slice differs from its live issue checklist
- **THEN** validation fails with the first owner-slice difference

#### Scenario: Candidate changes an unrelated slice
- **WHEN** the candidate changes an unrelated open issue's task slice from the accepted base
- **THEN** validation fails even if that unrelated slice happens to match mutable live state

#### Scenario: Candidate worktree bytes differ through a clean/smudge filter
- **WHEN** a candidate's tracked issue-map, mapped task, or linked architecture document has clean worktree bytes that differ from its submitted tree blob while ordinary Git status remains clean
- **THEN** candidate validation uses the submitted blob bytes and does not approve or parse the smudged worktree content

#### Scenario: Candidate replacement refs cannot redirect submitted objects
- **WHEN** local Git replacement refs substitute the submitted candidate commit, a descendant tree, a candidate blob, or accepted-base object
- **THEN** candidate validation disables replacement resolution and reads the original submitted object IDs

### Requirement: Candidate ownership and base authority fail closed
ProjectAtlas SHALL require exactly one mapped open owning issue and a readable accepted base for local candidate validation.

#### Scenario: One owner is resolved
- **WHEN** every post-base commit subject, including merge commits, contains exactly one well-formed `(#NNN)` reference to the same owning issue
- **THEN** pre-push validates that issue through the local candidate route

#### Scenario: Ownership is absent or ambiguous
- **WHEN** post-base commits contain empty or blank subjects, an unreferenced or malformed subject, an unmatched `(#` fragment, multiple references in one subject, or references to more than one issue
- **THEN** pre-push fails before checklist comparison

#### Scenario: Owner or base is invalid
- **WHEN** the selected owner is closed or unmapped, or accepted-base task authority cannot be read
- **THEN** candidate validation fails without falling back to global mutable state

### Requirement: Pre-push scope follows pushed remote refs
ProjectAtlas SHALL select global checklist validation when any pre-push update targets `refs/heads/main`. Candidate validation SHALL be allowed only when exactly one valid pushed target is a non-main `refs/heads/*` branch whose non-zero local object ID equals the validated checked-out `HEAD`, whose worktree has no tracked, staged, or non-ignored untracked changes, and whose tracked index entries are not marked `assume-unchanged` or `skip-worktree`; multiple non-main, deleted, mismatched, dirty, or hidden-index candidate targets SHALL fail closed before scoped validation.

#### Scenario: Feature checkout pushes main
- **WHEN** a feature checkout pushes an update whose remote ref is `refs/heads/main`
- **THEN** pre-push selects global checklist validation regardless of the checkout branch

#### Scenario: One candidate ref is pushed
- **WHEN** exactly one valid pre-push update targets a non-main `refs/heads/*` branch and its non-zero local object ID equals the validated checked-out `HEAD`
- **THEN** pre-push selects the local candidate route

#### Scenario: Multiple candidate refs are pushed
- **WHEN** two or more valid pre-push updates target non-main `refs/heads/*` branches
- **THEN** pre-push fails closed without selecting candidate validation

#### Scenario: Candidate local object differs from checkout HEAD
- **WHEN** one candidate update has a non-zero local object ID that differs from the validated checked-out `HEAD`
- **THEN** pre-push fails closed before owner, base, or scoped IssueOps validation

#### Scenario: Candidate deletion is pushed
- **WHEN** one candidate update has a zero local object ID
- **THEN** pre-push fails closed without selecting candidate validation

#### Scenario: Candidate worktree has mutable IssueOps input
- **WHEN** one candidate update has a non-zero local object ID equal to the validated checked-out `HEAD`, but the worktree has a tracked, staged, or non-ignored untracked issue-map, task, or documentation input
- **THEN** pre-push fails closed before owner, base, or scoped IssueOps validation

#### Scenario: Candidate worktree hides tracked IssueOps input
- **WHEN** one candidate update has a non-zero local object ID equal to the validated checked-out `HEAD`, but a tracked issue-map, task, or documentation input is marked `assume-unchanged` or `skip-worktree` and its filesystem content diverges while porcelain remains clean
- **THEN** pre-push fails closed before owner, base, or scoped IssueOps validation

#### Scenario: Candidate links an ignored document outside its tree
- **WHEN** a candidate issue links an architecture document that is present only as an ignored untracked worktree file and absent from the candidate tree identified by the pushed local object ID
- **THEN** pre-push fails closed before scoped IssueOps validation rather than reading the mutable file

#### Scenario: Candidate links a non-regular tree entry
- **WHEN** a candidate issue links an architecture document whose submitted candidate tree entry is a non-regular mode such as `120000`
- **THEN** pre-push fails closed before reading the worktree path or approving scoped IssueOps

#### Scenario: Candidate IssueOps inputs are non-regular tree entries
- **WHEN** the submitted candidate tree marks the issue map or a mapped task file as a non-regular entry such as `120000`
- **THEN** pre-push fails closed before reading the worktree input or approving scoped IssueOps

#### Scenario: Main and candidate refs are pushed together
- **WHEN** one pre-push update targets `refs/heads/main` and another targets a non-main branch
- **THEN** pre-push selects global checklist validation

#### Scenario: Ref-update input is malformed or unsupported
- **WHEN** pre-push receives no records, a record without four fields, or a target outside `refs/heads/*`
- **THEN** pre-push fails closed without selecting candidate validation

### Requirement: Global and hosted scopes remain unchanged
ProjectAtlas SHALL retain global checklist validation for `main` and release operations, PR-scoped validation for hosted pull requests, and planned-issue validation for issue events.

#### Scenario: Main and release validation
- **WHEN** validation runs for `main` or a release milestone
- **THEN** every active mapped live checklist is compared with the repository state as before

#### Scenario: Existing hosted scopes
- **WHEN** validation runs for a pull request or issue event
- **THEN** the existing PR-owner or planned-issue route remains authoritative

### Requirement: Real IssueOps execution remains mandatory
ProjectAtlas SHALL keep the real IssueOps self-test and all other pre-push quality gates mandatory when candidate checklist scoping is used.

#### Scenario: Candidate branch runs pre-push
- **WHEN** a valid candidate branch is pushed
- **THEN** the hook runs the existing quality gates and real IssueOps self-test before the scoped live checklist comparison
