# Stable release and front page

## Context

See proposal.md for scope. RC4 is the accepted product baseline. Issue #492 and its v0.5.0-00 milestone are closed; #602 owns the separately authorized stable promotion. The README still recommends 0.4.5 and puts installation before most product explanation.

## Goals / Non-Goals

Ship a coherent 0.5.0 identity and make the README understandable before a reader knows Atlas commands. Preserve runtime behavior, databases, reviewed RC history, the existing Ani artwork, and the current architecture. Do not turn publication into a product-feature change or a new benchmark campaign.

## Decisions

### Keep stable acceptance separate from RC acceptance

Map this change to #602 in a singleton v0.5.0-stable-00 graph and native milestone. Keep its owner open until publication and independent readback. The publication checker selects a declared stable graph for a stable tag, otherwise retains the existing milestone route for historical stable repairs. RC tags continue selecting their original graph. A malformed declared stable graph fails closed instead of falling back. Keep the existing version parser and workflow outputs compatible because the publication checker owns graph admission.

### Explain the map before the tools

Use a centered Ani identity and the exact three-sentence punch line, then a short account of repository orientation and persistent responsibility. Follow with a visual navigation path, concrete savings evidence, whole-product capabilities, current-release highlights, and installation. Give graphs, dead-code candidates, documents, worktrees, native CLI, MCP, and local storage clear places without printing a command inventory.

Use a consistent dark surface, warm readable text, restrained blue/gold accents, generous spacing, and one point per figure. Preserve accessible Markdown equivalents, useful alt text, and methods links. GitHub Markdown owns the layout; do not introduce an application, font download, or visual dependency.

### Publish evidence with its actual meaning

Generate charts from the existing checked-in audit and composition results. Label the large-application audit as a historical modeled workflow estimate, not a current release benchmark or provider bill. Label the compact/full graph comparison as payload bytes from the retained six-question fixture evaluation. Keep baseline values, units, provenance, and limits adjacent to each chart. The front-page promise is better orientation; savings depend on the task.

### Match support claims to shipped capabilities

The accepted optional pack has 150 grammars, including COBOL, on Linux x64 and Windows x64. Do not advertise ABAP or equivalent semantic support across all languages. Link the generated matrix for built-in parsing, fallback, symbols, and semantic resolution. Native atlas commands cover local navigation, refresh, analysis, and maintenance; MCP also supplies registered worktree aliases and federation.

### Make the preferred installation path explicit

Lead with asking the coding agent to install this repository, preferably with its marketplace plugin. Show exact versioned Codex marketplace commands. Explain native installers and generated MCP configurations for Claude Code and OpenCode. Restart the terminal and coding environment after installation, then initialize the intended repository once; a plugin listing alone does not prove native runtime or MCP readiness.

## Risks / Trade-offs

- A closed RC owner could block the first stable publication: use the independent stable graph and test initial publication, malformed mappings, RC routing, and historical repairs.
- A large modeled estimate could look like measured billing savings: label it in the image and adjacent prose; link the full method and retained mixed benchmark results.
- Optional grammar coverage could be mistaken for universal semantics or macOS pack support: state the platform boundary and link the generated inventory.
- Attractive graphics could be unreadable in a README column: inspect rendered figures at desktop and narrow widths and retain text alternatives.
- Version edits can invalidate parser handoffs and installed proof: construct the stable pack and run release-owned platform/package/upgrade gates against exact accepted inputs.

## Migration Plan

Land the reviewed preparation from accepted main without changing product behavior. Run the clean stable parser workflow and full prepublication package and upgrade gates on exact inputs. Publish stable 0.5.0 only after those gates pass. Verify tag, assets, checksums, native aliases, plugin/skill/hooks, generated host bindings, installed behavior, and Latest; then close #602 and its stable milestone. Preserve RC artifacts and previous stable artifacts for supported recovery.

## Dependencies / Cross-Issue Impact

The completed RC4 campaign in closed issue #492 is the product prerequisite. Existing dependency PRs and 0.6.0 work are outside this change. No new schema or runtime dependency is needed. Existing release, installer, and complete CLI/MCP acceptance suites remain authoritative.

## Open Questions

None.
