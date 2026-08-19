## Context

The CLI and MCP adapters both call the same map and SQLite lint routines, but each independently concatenates pre-rendered strings. The CLI then bypasses the normal serializer, ignores `cli.format`, writes the payload to stderr, and exits. MCP wraps the same opaque text with `ok` and `exit_code`. The path is platform-neutral and does not depend on source language.

## Goals / Non-Goals

**Goals:**

- Build one typed lint result for both adapters.
- Preserve current lint rules, blocking decisions, deterministic order, and exit codes.
- Serialize CLI output through the existing JSON/TOON path on stdout.
- Preserve the MCP compatibility summary while exposing structured details.

**Non-Goals:**

- Add a lint-specific serializer, transport, dependency, or command framework.
- Change Clap global-option placement, lint policy, or database mutation behavior.
- Add platform-specific output branches.

## Decisions

### Return existing domain facts instead of formatting during collection

Map lint will retain its existing deterministic collections but return a serializable report containing notes, non-source validation facts, and optional untracked-file counts/lists. SQLite lint will return the purpose level, bounded finding counts, and the existing typed health findings. One combined `LintReport` will own `ok`, `exit_code`, the structured map/index sections, and the existing human-readable `report` compatibility summary.

Keeping only `{ ok, exit_code, report: String }` is rejected because JSON would still require text parsing. Removing the text field is rejected because existing MCP consumers may display it. Generic key/value findings or a new trait hierarchy are rejected because the two closed lint sections already have concrete domain shapes.

### Compose once in the shared runtime

The shared runtime will own `lint_project`, which calls map lint and optional database lint exactly once and computes the maximum exit code. CLI and MCP will both call this function. This removes the duplicated concatenation and makes adapter drift impossible without adding a new crate or public service.

### Reuse the standard output adapter

CLI lint will encode the combined report under the stable `lint` payload key and call the existing flushed stdout writer. TOON remains the default; `--format json` emits pretty JSON. The payload is flushed to stdout before the process exits with the report's unchanged code. Stderr remains reserved for top-level execution failures or diagnostics.

The adapter selects the requested serializer before encoding the borrowed named payload. JSON therefore does not build or discard a TOON representation, and neither format first clones the complete report into an intermediate `serde_json::Value`.

MCP will encode the same report under its existing `lint` key and never terminates the transport. Missing indexes remain a valid `index: null` result, wrong-root failures remain typed preflight errors, and lint performs no implicit scan or mutation.

## Risks / Trade-offs

- [Existing MCP clients depend on the text report] -> Retain `report` while adding typed sections.
- [A lint failure exits before stdout is observable] -> Use the existing fallible stdout writer/flush before applying the exit code.
- [Structured and text views drift] -> Derive both from the same collected report value and cover clean and failing adapter outputs.
- [Large untracked repositories produce large output] -> Preserve the existing collected scope and deterministic ordering; this change does not widen filesystem traversal or add a second copy beyond the serialized response already required.

## Migration Plan

No durable migration is required. Release the additive MCP payload and new CLI stream/format behavior together. Existing consumers that display `lint.report` continue to work; machine consumers can adopt the typed sections.

## Dependencies / Cross-Issue Impact

This change is independent from the graph persistence and qualification fixes in #471 and #473. It reuses their RC3 source and installed-candidate workflow matrix, but adds no schema, parser, graph identity, or release-classification dependency.

## Open Questions

None.
