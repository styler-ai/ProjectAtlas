## ADDED Requirements

### Requirement: Representative bounded full-project Atlas preview
ProjectAtlas SHALL derive the wide token TUI Atlas preview from representative locally resolved relationships across each complete current-generation relation family while keeping database output, Rust memory, traversal, and rendering bounded.

#### Scenario: Dense full-project graph
- **WHEN** the selected atlas contains more resolved entities and relations than the preview caps
- **THEN** the preview deterministically renders 48 nodes and 64 links from the strongest connected evidence without using an opaque relation-key prefix as the population

#### Scenario: Unresolved prefix precedes resolved component
- **WHEN** more than 128 unresolved, external, containment, or self rows sort before a dominant resolved component
- **THEN** those rows cannot hide the later component and only accepted resolved non-containment relationships seed the preview

#### Scenario: High-degree articulation expands to branches
- **WHEN** one hub exceeds the 12-link visual degree cap and connects otherwise expanding branches
- **THEN** discovery considers reachable branch size before final edge admission and retains representative branches without exceeding the visual cap

### Requirement: Preview compatibility and exact-root isolation
ProjectAtlas SHALL preserve the existing Canvas, layout, palette, width threshold, visual caps, current-generation snapshot, and selected-worktree database boundary.

#### Scenario: Responsive width boundary
- **WHEN** the dashboard renders at 189, 190, 200, and 220 columns
- **THEN** the map is hidden at 189 and uses the same deterministic selected graph at every supported wide width

#### Scenario: Linked worktrees contain different graph facts
- **WHEN** sibling worktrees contain branch-only nodes or resolved relationships
- **THEN** each TUI preview uses only its explicitly selected worktree atlas and no sibling relation leaks

#### Scenario: Missing or corrupt bounded graph evidence
- **WHEN** the current graph generation is unavailable or incomplete, or a selected hub entity, adjacency row, endpoint, or truncation sentinel is invalid
- **THEN** the preview stays hidden or that requested page fails as a whole according to existing typed state and never returns partial bounded evidence
- **AND** normalized rows outside the requested page are validated only if a continuation addresses them; the preview does not repeat a whole-generation integrity audit before each bounded read

### Requirement: Reference visual remains unchanged
ProjectAtlas SHALL fix live preview correctness without replacing the checked-in README token TUI image.

#### Scenario: Release candidate visual proof
- **WHEN** the exact v0.4.3 candidate is manually reviewed in a Windows terminal
- **THEN** the live full-project map is legible and `docs/assets/token-impact-tui.png` remains byte-identical
