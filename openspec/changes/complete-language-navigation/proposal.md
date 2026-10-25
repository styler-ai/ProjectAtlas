# Complete major and legacy language navigation

## Why

Agents need a reliable map of unfamiliar systems, especially when planning a migration to Rust. Detection and a loadable grammar do not establish declarations, dependencies, graph analysis, or correct source evidence. ABAP, COBOL, Fortran, Ruby, and SQL are required for 0.6.0; major existing languages also need an evidence-based capability audit.

## What Changes

- Deliver complete static navigation for the five required languages through the existing registry, parser, fact extraction, resolver, SQLite, CLI, and MCP boundaries.
- Prove recognition, dialect/encoding policy, declarations and source spans, static dependencies and calls, useful graph analysis, and correct incremental updates.
- Audit Rust, Python, JavaScript/TypeScript, Java, C/C++, C#, Go, Kotlin, PHP, and PowerShell; fix material gaps before claiming parity. Extend further modern and legacy coverage where the same quality contract can be met.
- Add representative installed-platform and real-project proof, including a legacy-to-Rust planning example with exact source evidence and visible unknowns.
- Publish capability levels and limitations from the runtime authority rather than counting aliases, formats, or grammars as equivalent supported languages.

## Capabilities

### New Capabilities

- `language-navigation`: quality and evidence requirements for language-specific repository navigation, graph analysis, incremental behavior, and migration-oriented use.

### Modified Capabilities

None.

## Impact

Future implementation spans the existing core language registry, parser and symbol owners, semantic resolution, persisted derived graph, native CLI/MCP adapters, documentation, and installed tests. This planning change maps issue #563 into the 0.6.0 release graph. Only contract planning is complete; language implementation remains pending.

## Non-goals

No compiler-complete runtime analysis, automatic transpiler, guaranteed resolution of dynamic behavior, nominal support flags, universal dialect claim, or language implementation in 0.5.0. Additional languages do not replace the required five.
