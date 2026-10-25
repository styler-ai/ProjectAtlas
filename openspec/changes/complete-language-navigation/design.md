# Language navigation quality

## Context

The generated capability registry and accepted parser-pack manifest are the current authorities. ABAP is catalog-only. COBOL and Fortran have optional grammars but lack complete language-specific graph navigation. Ruby and SQL use fallback paths. Python has a built-in parser, symbols, and a static resolver; that does not prove every real project or dynamic form resolves.

## Goals / Non-Goals

Deliver the required five languages through the existing repository-navigation workflow and audit the major built-in languages. Make legacy-to-Rust planning useful with accurate source evidence. Do not introduce a transpiler, compiler, generic external-provider framework, duplicate graph, or a uniform support flag that hides different capability levels.

## Decisions

### Define full support by observable work

Each required language must recognize its declared source forms, expose useful declarations and exact spans, follow provable static calls and dependencies, answer normal CLI/MCP navigation and analysis queries, and refresh correctly after changes. Include representative dialects and a documented encoding boundary. Dynamic or malformed source must retain partial/unresolved evidence; absence from the graph is not proof that a declaration is dead.

### Reuse the seven-crate ownership

Detection and capability identity stay in core. Existing parser and symbol owners extract typed facts; existing resolvers establish only justified targets; the database owns derived generations and authored-state preservation; service, CLI, and MCP expose the same facts. Inspect and repair the shared contained-grammar-to-fact handoff before adding language adapters. Prefer existing canonical grammar dependencies and a small language owner over a new framework. Use built-in grammars where that is the smallest maintained route to all-platform support.

### Deliver the required set before optional breadth

ABAP, COBOL, Fortran, Ruby, and SQL are the minimum 0.6.0 commitment. Start with their detector/dialect contracts, then useful symbols, static facts and resolution, navigation/analysis, and incremental/platform proof. SQL requires an explicit dialect matrix, prioritizing the shared core and PostgreSQL, SQLite, MySQL/MariaDB, SQL Server, and Oracle. A dialect not proven remains explicitly limited.

Audit Rust, Python, JavaScript/TypeScript, Java, C/C++, C#, Go, Kotlin, PHP, and PowerShell against the same feature matrix. Repair material gaps and add other major or legacy languages when evidence, licensing, and maintainability support them. Do not substitute a large grammar count for completing the required set.

### Preserve source and publication authority

Use current saved bytes and exact root/database bindings. Wrong-root, missing-index, stale-selector, malformed-source, and unsupported-encoding states remain typed and bounded. Language indexing cannot mutate source or transfer authored purposes on rename. Failed or cancelled work leaves the last complete generation readable.

### Test navigation and migration work

Use focused positive, negative, malformed, dynamic, dialect, and encoding fixtures plus pinned real projects. Check expected declarations, source spans, calls, imports, analysis uncertainty, and current-source readback. Exercise init, scan, incremental change, rename/delete, CLI/MCP parity, cancellation, retry, and reopen. Measure cold and changed/unchanged refresh cost, output and memory bounds, worker usage, and publication hold time at representative scale. Prove installed Windows, Linux, macOS Intel, and Apple Silicon behavior.

A legacy-to-Rust example traces a bounded component, entry points, shared data and external boundaries, identifies affected callers, and cites original source while the agent plans Rust code and behavior-preservation tests. External SAP/database/runtime facts remain visible unknowns unless separately supplied and verified.

## Risks / Trade-offs

- A grammar succeeds but useful facts are discarded: inspect the shared parser-to-graph handoff and verify persisted facts through real navigation.
- Dynamic dispatch creates false dead-code findings: carry unresolved and incomplete coverage through analysis and exact evidence.
- Legacy dialects or encodings break otherwise clean fixtures: freeze representative source forms and test the supported boundary explicitly.
- Optional-pack containment excludes a platform: choose a maintained supported route for required intelligence on all four native tuples.
- More language work causes indexing or query amplification: retain bounded workers and transactional publication and compare representative resource measurements.

## Migration Plan

This PR publishes the plan only. Future implementation lands one complete language/shared-boundary slice at a time with its causal tests and updated capability documentation. Preserve existing databases and APIs; use the normal versioned migration path if a demonstrated persisted-contract change is necessary. Keep required languages incomplete until installed behavior and independent review pass.

## Dependencies / Cross-Issue Impact

N/A: no open implementation prerequisite is established. Issue #493 accepts the combined 0.6.0 product after #563 closes. Surface work #310 and incremental-refresh work #708 can proceed independently until a concrete shared-boundary change requires coordination. The completed PHP work supplies a structural example, not a dependency on compiler-complete semantics.

## Open Questions

None.
