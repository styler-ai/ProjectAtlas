# Tasks

## 1. Contract planning

- [x] 1.1 Finalize the language-navigation specification, required and additional language scope, ownership and task mapping, architecture links, platform and evidence criteria, non-goals, and native v0.6.0 release hierarchy before implementation planning.

## 2. Shared language boundary

- [ ] 2.1 Audit current detector, parser, symbol, static resolver, graph, CLI/MCP, and installed-platform behavior for the required languages, freeze representative source/dialect/encoding fixtures and pinned real projects with revision, license, provenance, expected facts and resource baselines, and verify the recorded gaps against actual navigation results.
- [ ] 2.2 Repair the shared contained-grammar-to-fact publication boundary or use the smallest maintained built-in alternative, preserve existing typed owners and authored state, and prove real SQLite write/read, failure/rollback, fallback, and source-selector behavior with causal tests.

## 3. Required language implementations

- [ ] 3.1 Deliver ABAP source recognition, useful declarations/spans, provable static calls and dependencies, resolver integration, navigation and analysis, and incremental behavior; verify representative positive, negative, malformed, dynamic, and exported-source fixtures through CLI and MCP.
- [ ] 3.2 Deliver COBOL source and COPY recognition, supported fixed/free forms, declarations/data structures/spans, static CALL/PERFORM/COPY facts, navigation and analysis, and incremental behavior; verify dialect, encoding, incomplete, and real-source fixtures through CLI and MCP.
- [ ] 3.3 Deliver Fortran source recognition, supported fixed/free forms, modules/procedures/types/spans, static USE/CALL facts, navigation and analysis, and incremental behavior; verify dialect, malformed, dynamic, and real-source fixtures through CLI and MCP.
- [ ] 3.4 Deliver Ruby source recognition, declarations/spans, provable require and call facts, resolver integration, navigation and analysis, and incremental behavior; verify representative project, malformed, dynamic-dispatch, and source-selector fixtures through CLI and MCP.
- [ ] 3.5 Deliver SQL core navigation and an explicit PostgreSQL, SQLite, MySQL/MariaDB, SQL Server, and Oracle feature matrix, including proven objects/spans/dependencies and incremental behavior; verify claimed dialect features and explicit limitations through CLI and MCP.

## 4. Major-language and installed acceptance

- [ ] 4.1 Audit Rust, Python, JavaScript/TypeScript, Java, C/C++, C#, Go, Kotlin, PHP, and PowerShell against the same feature matrix, fix material navigation gaps, and verify any additional major or legacy language before advertising its achieved support level.
- [ ] 4.2 Prove all five required languages on pinned real projects through exact installed Windows, Linux, macOS Intel, and Apple Silicon CLI/MCP workflows, including expected impact sets, dependency paths, cycles and dead-code candidates with unresolved/dynamic coverage safeguards, init/scan, edits/renames/deletions, wrong-root and missing-index refusal, cancellation/reopen/retry, compatibility, resource bounds, and required locked Rust, database, documentation, and platform gates.
- [ ] 4.3 Publish the runtime-derived support and dialect matrix, representative legacy-to-Rust planning example, source/unknown-behavior evidence, performance results, and updated agent guidance; independently review real navigation and reconcile source, specifications, and existing architecture diagrams before acceptance.
