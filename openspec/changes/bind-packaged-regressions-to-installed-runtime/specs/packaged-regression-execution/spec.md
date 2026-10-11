# Spec Delta

## Purpose

Ensure standalone release regression runners exercise the explicitly installed ProjectAtlas candidate while retaining ordinary source-test compatibility and unchanged acceptance assertions.

## ADDED Requirements

### Requirement: Standalone regressions use the installed candidate

Release-selected regressions SHALL use the explicitly supplied installed candidate for every runtime invocation and SHALL execute without Cargo's binary environment variable.

#### Scenario: Document navigation executes outside Cargo

- **WHEN** the compiled document-navigation regression runs directly with an explicit installed executable and plugin root, and Cargo's binary variable is absent
- **THEN** both CLI invocation paths use the installed candidate and the existing CLI/MCP document assertions execute without a Cargo lookup panic

### Requirement: Source-test compatibility and assertions are preserved

The regression SHALL retain ordinary Cargo execution when no installed candidate is supplied and SHALL preserve its existing navigation, publication, failure, resource, and isolation assertions.

#### Scenario: Ordinary source execution

- **WHEN** the regression runs through Cargo without installed-candidate inputs
- **THEN** it uses the ordinary source-test binary and retains the same behavioral assertions

#### Scenario: Installed acceptance remains complete

- **WHEN** the installed regression executes against isolated fixtures
- **THEN** no assertion is skipped or weakened to accommodate standalone execution and no unrelated project state is substituted or mutated
