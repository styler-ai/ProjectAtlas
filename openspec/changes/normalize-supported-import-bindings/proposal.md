# Proposal

## Why

Valid imports can retain parser facts while losing persisted dependencies and aliased callers because semantic normalization treats comment-bearing or grouped syntax as an unsupported string. The broader audit after #722 reproduced this in Rust and ECMAScript; RC1 must repair the shared assumption and check its Python and embedded-script implications.

## What Changes

- Normalize supported static imports from existing parsed syntax, treating comments and formatting as trivia while preserving literal contents, source evidence, trust and resource limits.
- Preserve Rust nested use-tree bindings and supported siblings; preserve ECMAScript mixed default/named/namespace bindings without inventing targets for unsupported default expressions.
- Cover Python comment-bearing static imports and admitted JavaScript/TypeScript embedded regions, deriving the affected matrix from the semantic-provider registry.
- Preserve Python explicit line continuation and exact ECMAScript module-literal whitespace rather than normalizing source tokens as display text.
- Keep explicit ECMAScript file suffixes separate from repository-exact extensionless aliases across relative/configured lookups, preserving supported JavaScript-output candidates without wrong-directory basename fallback or compiler-precedence guesses.
- Prevent function-local import leakage and parameter/local shadowing from producing false exact callers; include confirmed generic same-file failures in Go, C and C++ without adding cross-file semantic providers.
- Preserve ECMAScript script/module and function grammar context separately for binding declarations and identifier references, including contextual names, strict directives and implicit arguments ownership. Retain admitted host regions' known mode and lexical-root relationships instead of merging independent module locals into classic or sibling scripts.
- Preserve same-file reopened C++ namespace and explicit C# partial-type identity, including anonymous namespace visibility and TypeScript exported-member merging without private-block leakage.
- Reconcile static Rust and ECMAScript re-export alias evidence and downstream imported callers, including conservative ambiguity/cycle behavior.
- Prevent source-bearing re-export slots and their consumer clones from multiplying one module literal into per-slot owned copies before staging admission; preserve exact values and cancellation without imposing a new module-length restriction.
- Invalidate stale extraction and semantic projections through their existing contracts; verify unchanged-index repair, persisted dependencies, aliased callers and incremental/full convergence with preserved identity and approved purposes.

## Capabilities

### New Capabilities

- `static-import-bindings`: syntax-aware normalization, lexical call safety and conservative persisted resolution of supported static bindings across source providers and affected native local resolver paths.

### Modified Capabilities

None. The only canonical capability currently in `openspec/specs` is token-impact reporting. This change complements the accepted `python-import-resolution` change without rewriting its historical acceptance.

## Impact

Existing symbol extraction, language-owned Rust/ECMAScript/Python normalization, generic local call resolution, resolution keys, the separate summary imported-caller consumer, parser/semantic invalidation contracts, persisted relation/caller regression tests, embedded-source adapters and release acceptance selectors. Reuse existing ASTs and typed evidence; preserve existing CLI/MCP behavior and compatibility. Carry lexical and export evidence in a typed internal source sidecar from the existing AST and recover missing sidecars only for required captured sources; retain the public graph/schema and query the existing canonical graph. Do not hide semantic metadata in display strings. Cargo structured dependencies and unsupported cross-file provider boundaries remain unchanged.

## Release Scope

High-priority #731 belongs to `v0.5.1-00`, directly under and blocking #715, and must be accepted before `v0.5.1-rc1`. It starts from accepted main including #722, has no direct open implementation blocker, and can proceed alongside #708; combined-source proof must be reconciled before release. Ready for implementation after independent specification approval and passed strict OpenSpec/scoped IssueOps validation. Stable publication requires separate authorization.

## Non-Goals

No new semantic provider or language-support claim, execution or runtime inference, arbitrary default-expression resolution, wildcard expansion, parser framework, database reset, relaxed cancellation or bounds, or unrelated v0.6.0 work.
