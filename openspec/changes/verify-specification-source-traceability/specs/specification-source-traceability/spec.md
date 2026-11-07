## Purpose

Let agents navigate between actual repository specifications and the source they document, with correct purpose metadata and explicit evidence limits, through the existing ProjectAtlas relation workflow.

## ADDED Requirements

### Requirement: Associations have authoritative repository evidence
Specification/source associations SHALL use explicit supported repository references whose intended implementation ownership has been reviewed. A resolved path SHALL establish a navigation fact, not semantic conformance. Names, prose similarity, timestamps and purpose text alone SHALL NOT create an association.

#### Scenario: Actual specification identifies its implementation
- **WHEN** a reviewed OpenSpec file contains a supported explicit reference to an indexed implementation target
- **THEN** the graph exposes that exact specification/source association with its original document location and resolution evidence
- **AND** acceptance verifies the reference against the actual requirement and implementation responsibility, not merely that the target file exists

#### Scenario: No authoritative source reference
- **WHEN** a specification contains only prose, an unrelated example, or an unsupported dynamic reference
- **THEN** the system does not invent implementation ownership
- **AND** it distinguishes missing authored evidence, supported extraction with no candidates, unsupported syntax, unresolved candidates and incomplete coverage

### Requirement: File-first navigation is bidirectional
A valid canonical `documents` fact SHALL be traversable from the owning document file to its resolved implementation and from that implementation through the inbound `documented_by` view. Heading and occurrence evidence SHALL remain available without requiring a heading query to discover a file-owned association or persisting a duplicate inverse fact.

#### Scenario: Source reveals its specification
- **WHEN** an agent requests inbound documentation relations for a source file with an admitted specification reference
- **THEN** it receives the correct specification file and retained heading/occurrence evidence
- **AND** outbound traversal of that specification returns the same exact implementation identity

#### Scenario: Repeated references under several headings
- **WHEN** one specification references the same implementation under several headings
- **THEN** file-first traversal retains one logical association and all bounded occurrence evidence
- **AND** supported exact heading selectors reach the appropriate evidence without changing the logical file ownership

### Requirement: Agent workflows expose dependencies and specifications
The documented agent relation workflow SHALL retrieve both source dependencies and applicable specification links using the existing classified, directional relation surfaces. A legacy source-only result SHALL NOT be presented as a complete specification search. Required additional requests SHALL be explicit, bounded and retain the selected root and evidence identities.

#### Scenario: Agent asks for a source file's context
- **WHEN** an agent follows the shipped relation recipe for a source file
- **THEN** it can inspect the relevant source-dependency view and the inbound specification view without manually guessing specification paths
- **AND** the recipe specifies content selection, direction, filters, coverage inspection and any continuation or second bounded request needed by the existing interface

#### Scenario: Legacy omitted selection
- **WHEN** an existing caller omits classified selection and uses the established source-only defaults
- **THEN** its supported behavior remains compatible
- **AND** the agent-facing guidance identifies how to request specification context rather than treating omitted documentation as absence

### Requirement: Multiple specifications retain their identity and authority
The relation workflow SHALL retain every applicable explicit specification association within its bounds, with exact document identity and evidence. Historical, proposed and accepted specifications SHALL NOT be silently treated as interchangeable current requirements. Unknown authority or lifecycle status SHALL remain unknown; a link alone SHALL NOT prove current normative authority.

#### Scenario: Several specifications document one source file
- **WHEN** several reviewed specifications contain relevant admitted references to the same source
- **THEN** the workflow preserves each distinct document association and its evidence rather than selecting one by name or recency

#### Scenario: Historical or ambiguous authority
- **WHEN** indexed documentation includes a historical specification or its current authority cannot be established from the repository's declared specification lifecycle
- **THEN** navigation retains the truthful reference and available lifecycle context without labelling it the sole authoritative current specification
- **AND** an agent is directed to the declared repository specification context for semantic reconciliation

### Requirement: Purpose metadata belongs to the correct endpoint
Each source and document endpoint SHALL project its own accepted purpose from its authoritative local owner with existing status and provenance. Missing or unapproved purpose SHALL remain unavailable under the relation contract; an external or unresolved target SHALL not borrow another endpoint's purpose. Purpose text SHALL NOT change relation identity or content classification.

#### Scenario: Different accepted source and specification purposes
- **WHEN** a specification and its implementation have different accepted purposes
- **THEN** both directional relation views show the correct purpose at each endpoint with its actual owner path, status and provenance

#### Scenario: Unapproved or non-local purpose
- **WHEN** an endpoint has only a suggested purpose, has no accepted purpose, or has no local resolved owner
- **THEN** the existing unavailable or not-applicable state is reported truthfully
- **AND** no suggestion is promoted to approved or copied from the opposite endpoint

#### Scenario: Purpose changes without source changes
- **WHEN** an accepted endpoint purpose is edited through its owning API without changing source or relation facts
- **THEN** a new relation query reflects the new purpose without rebuilding the graph
- **AND** existing generation/purpose-bound continuations refuse or restart as required rather than mixing purpose revisions

### Requirement: Links resolve exactly and preserve trust boundaries
Specification references SHALL use the existing supported static-reference and exact indexed-target rules. Missing, ignored, ambiguous, case-conflicting, unsupported, outside-root and indirect targets SHALL retain typed outcomes. A wrong selector SHALL NOT be repaired by guessing a same-named file or silently dropping a failed fragment.

#### Scenario: Duplicate names in different directories
- **WHEN** several source files or symbols share a name and a specification references one supported exact target
- **THEN** only that exact identity is resolved; unrelated same-named implementations are not associated

#### Scenario: Invalid or unsupported selector
- **WHEN** a reference has a broken path, wrong heading, unsupported fragment, excluded target or unsafe indirection
- **THEN** it retains the applicable typed unresolved or refused outcome and privacy-safe evidence
- **AND** no convenient source-file fallback is reported as a fully resolved exact selector

### Requirement: Refresh removes stale associations
Full and incremental indexing SHALL converge on the same complete specification relations, occurrences and purpose projections. Document and target additions, edits, renames, deletion, ignore changes and relevant derivation changes SHALL invalidate the correct affected closure. Unchanged supported state SHALL retain the established no-op behavior.

#### Scenario: Specification reference or implementation moves
- **WHEN** a document reference, heading or target source is changed, renamed, deleted or excluded
- **THEN** incremental refresh removes stale associations and publishes the same valid and unresolved result as a complete rebuild
- **AND** unrelated accepted purposes and relations remain intact

#### Scenario: Supported upgrade and reopen
- **WHEN** an atlas created by a supported predecessor is reopened or refreshed with changed documentation-projection requirements
- **THEN** stale facts are repaired or rejected through typed recovery before they are trusted
- **AND** project identity and authored purposes survive without a reset

#### Scenario: Specification archive or directory move
- **WHEN** a specification moves or is promoted and its relative references are updated for the new owning path
- **THEN** refresh exposes the correct new specification/source identities and removes obsolete associations
- **AND** a moved reference that was not updated and no longer resolves is reported truthfully rather than retaining its former target as a successful current link

### Requirement: Partial results do not prove absence
Specification navigation SHALL preserve direction, classification, resolution, relation-specific coverage, generation, purpose revision, bounds and continuations. An empty or truncated page SHALL NOT be interpreted as proof of no applicable specification unless the owning document relation coverage and traversal are complete for the request.

#### Scenario: Output or traversal bound is reached
- **WHEN** the relation query reaches a supported row, node, edge, byte, occurrence or deadline bound
- **THEN** it reports the bound and retains its exact continuation or typed incomplete state
- **AND** the agent follows that state without changing root, filters, generation or purpose revision silently

#### Scenario: Complete zero and incomplete extraction
- **WHEN** one document has complete supported extraction with no static candidates and another has partial or failed extraction
- **THEN** their documentation coverage states remain distinguishable
- **AND** general parser coverage does not conceal or manufacture complete `documents` coverage

### Requirement: CLI and MCP expose the same facts
CLI JSON, CLI TOON and MCP SHALL agree on exact specification/source identities, relation direction and inbound view, endpoint purposes, classification, evidence, coverage and continuation semantics. Serialization or content selection SHALL not silently hide an admitted specification in the documented agent recipe.

#### Scenario: Equivalent adapter requests
- **WHEN** equivalent bounded requests inspect the same exact atlas generation and purpose revision through CLI JSON, TOON and MCP
- **THEN** all adapters expose semantically equivalent facts and typed unavailable/unresolved/partial states

### Requirement: Reads preserve exact root and publication safety
Relation reads SHALL stay bound to the explicitly addressed project or registered worktree and its existing private atlas. Missing state, wrong-root identity, incompatible state, cancellation and publication races SHALL preserve supported typed recovery and the last valid publication without implicit initialization or mutation.

#### Scenario: Wrong root or missing atlas
- **WHEN** a request addresses a mismatched root or a project without initialized indexed state
- **THEN** it returns the applicable typed failure or initialization guidance without selecting a sibling atlas or creating project state

#### Scenario: Failure during an affected refresh
- **WHEN** cancellation, a deadline, concurrent modification or injected failure interrupts specification-relation refresh
- **THEN** readers retain the prior complete publication, identity and accepted purposes and an adequate retry can converge

#### Scenario: Two worktrees have different specifications
- **WHEN** two selected worktrees contain different saved specification references or purposes for similar source paths
- **THEN** each ordinary query uses its own private facts and provenance
- **AND** an explicitly supported federated query labels participants without merging their writable state

### Requirement: Acceptance exercises real installed behavior
Acceptance SHALL cover actual reviewed repository specification/source pairs and controlled negative fixtures through the supported installed CLI/MCP and native platforms. It SHALL record confirmed defects, missing authoring, deliberate unsupported cases and remaining uncertainty; feature declarations and zero-filter test runs SHALL NOT substitute for behavioral proof.

#### Scenario: Real repository traceability
- **WHEN** representative committed OpenSpec/source pairs are tested from an isolated copy preserving their paths and authored references
- **THEN** the normal agent recipe returns the expected exact sources, specifications and purpose owners in both directions
- **AND** the test does not manufacture missing repository links only inside its fixture to conceal a real authoring gap

#### Scenario: Existing implementation is already correct
- **WHEN** the full required checks establish that the current graph behavior satisfies this contract
- **THEN** the handoff records that result and retains the implementation, adding only genuinely missing references, guidance or causal proof

**Implementation references for this planning specification:**

These explicit repository references identify the existing owners to inspect; they do not assert that every required behavior has already been verified.

They are relative to this document's current location. Archive or promotion must reconcile their paths in the resulting specification; merely copying a resolved reference does not preserve its target after a directory-depth change.

- [Markdown reference extraction](../../../../../crates/projectatlas-symbols/src/markdown.rs)
- [Document target admission and graph publication](../../../../../crates/projectatlas-cli/src/runtime/graph_projection.rs)
- [Bounded relation views and purpose projection](../../../../../crates/projectatlas-service/src/relations.rs)
- [Normalized graph storage and adjacency](../../../../../crates/projectatlas-db/src/repository_graph.rs)
- [Installed and worktree agent integration tests](../../../../../crates/projectatlas-cli/tests/e2e_worktrees.rs)
