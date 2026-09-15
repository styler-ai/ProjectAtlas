## ADDED Requirements

### Requirement: Publication admission preserves release-owner lifecycle

The checker SHALL distinguish canonical release publication from strict milestone closeout. Publication SHALL derive the milestone using the shared release classifier and permit only its declared release owner to remain open. Every other milestone member SHALL be mapped and closed. Normal task/acceptance validation and exact-main publication checks SHALL remain in force. Publication admission SHALL NOT close issues or mark release tasks complete.

#### Scenario: Publication with completed children
- **WHEN** the declared release owner is open, all declared children are closed, and native milestone membership matches the validated declaration
- **THEN** publication admission succeeds while the owner and its unfinished publication tasks remain open

#### Scenario: An implementation child remains open
- **WHEN** any member other than the declared release owner is open
- **THEN** publication admission fails

#### Scenario: Strict closeout or historical milestone
- **WHEN** strict milestone closeout is requested, or publication targets a historical milestone without a release graph
- **THEN** every milestone issue must be mapped and closed

#### Scenario: Conflicting selector modes
- **WHEN** publication and closeout selectors are combined
- **THEN** the checker refuses the ambiguous invocation

### Requirement: Publication declaration and native membership agree

The checker SHALL reject malformed release graphs, invalid or unmapped issue identities, missing roots, duplicate/self/unknown dependencies, cycles, a root whose blockers differ from its children, and missing or extra native milestone members. Declined work SHALL be reconciled explicitly without being counted as completed implementation.

#### Scenario: Declaration or membership drift
- **WHEN** the graph is invalid or its declared members differ from the native milestone
- **THEN** publication admission fails before mutation

### Requirement: A closed release owner admits only exact repair

For a declared closed release owner, publication admission SHALL require an existing non-draft GitHub release with the requested canonical tag and matching stable/prerelease classification, whose Git tag resolves to the checked-out candidate commit. Unavailable or malformed evidence SHALL fail closed. Existing downstream release identity and Latest checks SHALL remain authoritative at mutation.

#### Scenario: Repair after release closeout
- **WHEN** the owner is closed and the existing release, classification, tag commit, and candidate commit all match
- **THEN** publication admission permits repair

#### Scenario: Premature closure or mismatched repair
- **WHEN** the owner is closed but the release is absent, draft, misclassified, differently tagged, or points to another commit
- **THEN** publication admission fails without creating or changing a release
