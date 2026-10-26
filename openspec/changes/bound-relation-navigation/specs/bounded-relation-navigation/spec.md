## Purpose

Let coding agents inspect useful relation pages while keeping returned data, query work and intermediate state bounded and preserving trustworthy graph evidence.

## ADDED Requirements

### Requirement: Ordinary navigation uses bounded detailed pages
CLI and MCP relation navigation SHALL default to the existing detailed view, ten rows and 32 KiB encoded output. An exact file anchor, optionally refined by a symbol selector, SHALL be required. Missing anchors SHALL receive actionable narrowing guidance. Explicit detailed limits and continuation SHALL retain their established validated bounds.

#### Scenario: Ordinary anchored call
- **WHEN** a caller supplies an indexed file without selecting a view or limits
- **THEN** it receives a detailed page bounded by ten rows and 32 KiB, with truthful completeness and supported continuation when more eligible data remains

#### Scenario: Unanchored call
- **WHEN** a caller omits the required anchor
- **THEN** the call refuses with guidance to select a file or discover candidates, without materializing repository-wide connections

### Requirement: Explicit analysis retains its existing contract
Analysis SHALL remain opt-in. Explicit analysis view SHALL retain its existing 50-row and 256-KiB output defaults, profile/mode behavior, budget validation and continuation contract. Ordinary detailed and explicit legacy default selection SHALL not reinterpret analysis requests.

#### Scenario: Explicit analysis without limits
- **WHEN** CLI or MCP explicitly selects analysis without overriding rows or output bytes
- **THEN** it uses the established analysis defaults and semantics, rather than the ten-row/32-KiB detailed defaults or legacy complete-array policy

### Requirement: Explicit compatibility results are complete and bounded
Explicit legacy view SHALL preserve the existing array shape only for complete small results. Its omitted row limit SHALL remain 50; requested rows SHALL be 1 through 200. Successful encoded output SHALL default to 32 KiB and never exceed 256 KiB or a smaller caller bound. Results exceeding rows or bytes SHALL refuse with detailed-view narrowing/continuation guidance rather than silently omit data.

#### Scenario: Complete small array
- **WHEN** an explicit legacy request matches fewer than or exactly its requested rows and fits the output bound
- **THEN** CLI and MCP preserve the complete legacy relation fields, direction, order and array shape

#### Scenario: Omitted compatibility row limit
- **WHEN** explicit legacy view omits its row limit and matches 11 through 50 complete rows that fit its byte bound
- **THEN** both adapters return the complete array using the retained 50-row default rather than applying the detailed ten-row default

#### Scenario: Oversized reproduction
- **WHEN** either adapter receives the legacy 10,001-row request, including with a 4,096-byte output bound
- **THEN** it refuses before a large query or result allocation and gives a bounded detailed alternative

#### Scenario: More rows or bytes than admitted
- **WHEN** an otherwise valid legacy request has additional eligible rows or its complete encoded result cannot fit
- **THEN** it returns a typed bounded refusal instead of a partial successful array

### Requirement: Compatibility work is bounded before decoding
Legacy candidate discovery, filtering, sorting and decoding SHALL share cancellation, a maximum 1,000 ms cooperative deadline, a 100,000 SQLite virtual-machine-instruction ceiling and a 1 MiB decoded/intermediate ceiling. Smaller applicable caller limits SHALL take precedence. Exhaustion SHALL refuse the whole result and release database execution hooks and temporary limits.

#### Scenario: Sparse broad match
- **WHEN** a small requested output requires excessive scanning or ordering on a high-degree fixture
- **THEN** the operation stops within its cooperative work policy without decoding or retaining the whole relation set

#### Scenario: Wide stored values
- **WHEN** a relation contains a value wider than the remaining admitted byte budget
- **THEN** the operation refuses before copying that value into an owned result and does not expose a partial page

#### Scenario: Cancellation and recovery
- **WHEN** execution is cancelled or reaches a work or byte limit
- **THEN** a later independent query can succeed with all execution hooks and connection limits restored

### Requirement: Legacy query patterns have a native size ceiling
Legacy query text SHALL be limited to 1,024 UTF-8 bytes before allocating its LIKE
pattern. The temporary native LIKE-pattern ceiling SHALL be at most 1,026 bytes.
Existing wildcard and SQLite case semantics SHALL remain within that cap, and a
smaller existing native limit SHALL remain effective.

#### Scenario: Bounded query patterns
- **WHEN** a legacy query exceeds 1,024 UTF-8 bytes, including multibyte text
- **THEN** it refuses before pattern allocation or query execution with narrowing guidance, and admitted wildcard queries retain their existing matching semantics

### Requirement: Counts and continuation describe actual knowledge
Detailed results SHALL distinguish returned rows, known or unknown totals, coverage and reached limits. Exact totals SHALL not require unbounded work. Continuation SHALL remain deterministic and bound to the selected project, generation, anchor, filters and budgets. Complete legacy arrays SHALL never be presented as partial detailed pages.

#### Scenario: High-degree file
- **WHEN** a page ends before all eligible connections are known
- **THEN** its totals remain unknown or explicitly partial and supported continuation or narrowing preserves access to the remaining data

#### Scenario: Stale or retargeted continuation
- **WHEN** the project, generation or result-defining request changes
- **THEN** continuation refuses under the existing binding contract instead of returning a mixed result

### Requirement: Evidence and selection remain intact
Bound enforcement SHALL preserve admitted relation identity, kinds, direction, resolution, ambiguity, occurrences, parser provenance and content selection. It SHALL not delete, relink or hide valid stored graph data to fit a response. Unsupported controls and invalid zero or oversized bounds SHALL receive actionable errors consistently through CLI and MCP.

#### Scenario: Dense hub or cycle
- **WHEN** a bounded detailed request visits a dense hub or cycle
- **THEN** existing traversal and evidence rules remain effective, and paging or refusal explains the reached bounds

### Requirement: Relation reads remain root safe without initialization
Relation reads SHALL use the explicitly selected project or worktree's existing private database. Wrong-root, missing or incompatible state SHALL return typed recovery guidance without initialization, reset, source mutation or substitution of another database.

#### Scenario: Missing index
- **WHEN** a relation request addresses uninitialized project state
- **THEN** it returns initialization guidance and creates no database, schema or source files

#### Scenario: Wrong root or worktree
- **WHEN** the selected database identity differs from the addressed project or alias
- **THEN** the request refuses without reading another project's relations or mutating either project

### Requirement: Public guidance and real boundary proof agree
CLI defaults/help, MCP schemas, plugin guidance and user documentation SHALL describe the same normal, explicit legacy and continuation behavior. Focused real CLI/MCP proof SHALL independently measure rows examined/decoded, retained bytes and encoded output, including the oversized reproduction, high-degree queries, failures and compatibility cases.

#### Scenario: Public surface reconciliation
- **WHEN** the completed feature is checked through installed CLI JSON/TOON and MCP tool responses
- **THEN** all documented limits hold at the actual adapter payload boundary and the required existing release surface checks remain valid
