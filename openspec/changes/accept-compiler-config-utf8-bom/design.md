## Context

`load_configured_module_resolution` is the single filesystem boundary used by full and incremental graph staging. It already applies per-file and aggregate byte bounds, controlled reads, source-hash currentness, strict UTF-8 decoding, JSONC parsing, deadline/cancellation checks, and typed failures before either CLI or MCP publication. Its UTF-8 decoder currently receives the leading BOM and the JSONC parser rejects that character.

## Goals / Non-Goals

**Goals:**

- Treat one exact leading UTF-8 BOM as an encoding marker for both supported compiler-configuration basenames.
- Keep the original bytes authoritative for size and source-currentness checks.
- Preserve strict parse and non-UTF-8 failures and adapter-equivalent publication.

**Non-Goals:**

- Add another decoder, parser, dependency, or configuration abstraction.
- Remove any prefix other than exact `EF BB BF` at byte zero.
- Change configuration semantics, refresh selection, persistence, or public adapters.

## Decisions

- After the controlled read and hash comparison, borrow either the bytes following exact `EF BB BF` or the complete byte slice and pass that slice to the standard UTF-8 decoder. This keeps size/hash authority on the complete file, avoids a copy, and changes only the parser input.
- Keep the behavior in `load_configured_module_resolution`, where both `tsconfig.json` and `jsconfig.json` and both full/incremental staging already converge. A command-specific guard would leave sibling adapters inconsistent.
- Verify BOM/non-BOM equivalence and malformed/non-UTF-8 rejection directly at the loader, then use one focused real adapter test for CLI init/scan/watch and MCP watch refresh. No new test framework or fixture layer is needed.

## Risks / Trade-offs

- [A partial or misplaced BOM is accidentally accepted] → Match only the complete three-byte prefix at offset zero and retain strict UTF-8/JSONC errors otherwise.
- [Size or currentness behavior drifts] → Perform stripping only after the existing complete-byte bounds and hash comparison.
- [One refresh adapter remains broken] → Keep the production change in the shared loader and exercise the existing real CLI/MCP refresh entry points.
