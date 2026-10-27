# Proposal

## Why

Valid parenthesized Python from-import lists are extracted but rejected by semantic normalization. Installed 0.5.0 reproduces an unresolved local file dependency while the adjacent ordinary import resolves. Agents consequently miss dependencies and imported callers.

## What Changes

Accept one flat outer parenthesized named-import list, including line breaks, a trailing comma and aliases, through the existing Python semantic owner. Reject malformed or unsupported forms without fabricated targets. Advance the existing semantic contract so unchanged compatible indexes are repaired on refresh.

## Capabilities

### New Capabilities

- `python-import-resolution`: equivalent static normalization and graph resolution for ordinary and parenthesized Python from-import lists.

### Modified Capabilities

None. Existing relation routes and persisted schema remain unchanged.

## Impact

Python semantic normalization, resolution-key tests, existing CLI graph/caller integration and the semantic projection contract. Reuse the already-locked canonical unicode-ident crate as a direct dependency for Python-compatible identifier character classes. No new transitive packages, parser pack, schema or public route changes.

## Release Scope

Ready for implementation in 0.5.1-rc1 under #722, a direct child and blocker of release owner #715. Independent of #708 and #721; integrate on accepted main. #715 includes installed upgrade proof. A required one-time semantic-policy upgrade must be identified separately from already-current steady-state opens in #708 measurements.

## Non-Goals

No Python execution, dynamic imports, wildcard expansion, speculative module guessing, general Python parser rewrite, new CLI/MCP routes or storage layout changes.
