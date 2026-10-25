## ADDED Requirements

### Requirement: Classified discovery selects only indexed files
CLI `files --content-selection source|documentation|both` and MCP `atlas_files` SHALL classify only real, currently indexed candidate files. Inferred companion-test paths that are absent from the indexed file set SHALL NOT be classified or make the request fail.

#### Scenario: Small repository has no companion test
- **WHEN** a repository contains `src/main.ts` and `docs/guide.md` but no inferred companion paths
- **THEN** source, documentation, and both selections return their matching real indexed files successfully

#### Scenario: Real classification failure remains visible
- **WHEN** a selected real indexed file has invalid or unavailable classification state
- **THEN** the request retains its typed failure rather than silently dropping that real file

### Requirement: Classified CLI and MCP remain equivalent
The CLI and MCP SHALL apply query, file-pattern, rank, and limit semantics to the same admitted classified candidates and report the same success or failure boundary.

#### Scenario: Filtered classified query excludes synthetic path
- **WHEN** a classified query uses a glob or limit that would previously surface a nonexistent inferred path
- **THEN** CLI and MCP return the matching indexed paths with equivalent ordering and bounds
