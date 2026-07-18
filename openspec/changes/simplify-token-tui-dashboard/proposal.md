## Why

The token TUI currently repeats the same savings numbers in separate summary, gauge, file-handling, bucket, and notes sections. That makes the dashboard harder to trust: the gross estimate, conservative avoided tokens, measured replacements, modeled navigation, and file-read counters appear near each other without an obvious accounting relationship.

## What Changes

- Simplify the overview dashboard to one conservative headline summary, one compact file-handling equation strip, one source table, one trend section, and one short notes block.
- Keep Ratatui standard widgets for the terminal UI: `Block`, `Paragraph`, `Chart`, `Gauge`, and `Table`.
- Make the visible source table add up to the conservative saved-token total and file-read-avoidance total.
- Keep the day, week, month, and year saved-token trends visible in the overview.
- Use styled, spaced table headers so header text and first-row content are visually distinct.

## Capabilities

### New Capabilities
- `token-tui-dashboard`: Defines the human terminal dashboard contract for token savings, file-read avoidance, trend windows, and visible accounting consistency.

### Modified Capabilities

## Impact

- Expected code touch points: `crates/projectatlas-cli/src/token_tui.rs`, `crates/projectatlas-cli/src/main.rs`, and `crates/projectatlas-cli/tests/e2e.rs`.
- Expected test touch points: token TUI unit tests plus CLI/e2e smoke assertions for the terminal dashboard.
- No MCP payload schema change is required; MCP agents continue to use `atlas_token_report` instead of terminal TUI rendering.
