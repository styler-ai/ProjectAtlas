## Why

Issue #302 was accepted too early. The shipped `projectatlas token --view tui` overview still renders like a mostly monochrome terminal report, while the approved reference in `docs/design/token-impact-tui-reference.png` is a polished Ratatui dashboard with clear window framing, warm ProjectAtlas identity treatment, semantic color roles, stronger section hierarchy, visual bars, readable table separation, and an intentional Ani mascot mark.

This is a trust issue for the token-savings surface. The dashboard is supposed to make the accounting understandable at a glance; if the screenshot looks flat, cramped, or semantically miscolored, users cannot tell which values are original baseline, ProjectAtlas work, modeled estimates, or net savings.

## What Changes

- Add a new OpenSpec change, `fix-token-tui-reference-visual-regression`, mapped to GitHub issue #304.
- Rework the token overview TUI until the real screenshot materially matches `docs/design/token-impact-tui-reference.png`.
- Keep the agreed color correction:
  - ProjectAtlas identity and ProjectAtlas-origin metrics use ivory/off-white.
  - Original/counterfactual baseline values use blue.
  - Net saved/savings values use green.
  - Modeled/search/confidence values use yellow.
- Replace the current plain ASCII Ani treatment with an image-derived Ratatui mascot mark using the committed transparent Ani PNG plus the source SVG, rendered through a portable `ratatui-image` halfblock protocol so it reads as Ani at dashboard size and fits the reference composition.
- Use Ratatui standard widgets and style primitives before custom drawing: `Layout`, `Block`, `Paragraph`, `Gauge` or `LineGauge`, `Table`, `Row`, `Cell`, `Sparkline`/`BarChart` where trends are needed, and direct `Buffer` writes only for tiny mascot/icon art that needs exact cells.
- Preserve the existing information fields and arithmetic:
  - `without_projectatlas - with_projectatlas = saved_by_projectatlas`.
  - `observed_file_read_replacements + modeled_file_reads_avoided = likely_file_reads_avoided`.
  - visible source rows reconcile to the headline saved-token total.
- Add deterministic buffer tests for text, math, semantic styles, table spacing, and bar fill proportions.
- Capture and review a real screenshot before closing the issue or releasing.

## Capabilities

### New Capabilities

- `token-tui-reference-visual-regression`: defines a regression-proof visual and testing contract for the reference-matched Ratatui token overview.

### Modified Capabilities

- `token-impact-reference-dashboard`: strengthens the prior reference-dashboard contract with explicit screenshot-parity and style-role gates after the first implementation failed visual review.

## Release Scope

This change is ready for implementation in the next release after v0.3.25. It targets the v0.3.26 milestone and issue #304.

The change is scoped to the CLI token TUI, its tests, OpenSpec/governance metadata, and documentation links needed to keep the reference visible. Token telemetry storage and MCP token-report payload contracts remain unchanged unless a compile/test fix requires a harmless internal helper change.

## Non-Goals

- Do not add terminal-specific bitmap protocols, sixel, Kitty graphics, iTerm inline images, or a broad custom renderer. The accepted image path is `ratatui-image` halfblock rendering from the committed transparent Ani asset.
- Do not change persisted token telemetry schemas.
- Do not replace the existing trend dashboard unless required to keep it compiling with shared helpers.
- Do not add repeated explanatory fields that make the overview harder to scan.
- Do not release until a real screenshot has been compared against the reference and the remaining visual differences are documented or fixed.

## Pre-Mortem

Likely failure modes:

- Tests assert labels and arithmetic but miss the same visual failure again because they do not inspect Ratatui styles.
- The implementation uses ivory in the title but leaves ProjectAtlas-origin numbers and bars blue, breaking the agreed semantic palette.
- Ani is technically present but too tiny, too ASCII-flat, too coarse, or visually unrelated to the mascot/reference.
- The table still lacks visible header separation or row dividers, so the `WHERE THE SAVINGS CAME FROM` panel remains hard to read.
- Bars are text decorations that do not reflect the underlying ratios.
- The overview becomes taller or more complex than the terminal can comfortably render, causing bottom notes/footer crowding.
- The implementation bypasses Ratatui widgets with broad custom rendering, making the dashboard brittle.
- The screenshot review happens after release instead of before merge.

Mitigations:

- Add style-role assertions against Ratatui `Buffer` cells for the critical labels/values.
- Add bar-ratio tests that inspect filled/empty gauge or bar cells for representative data.
- Use a small image-derived Ani Ratatui widget with exact cell/style tests and a real screenshot check.
- Use `Table::header(...).bottom_margin(1)`, row separators, or equivalent Ratatui styling so headers cannot run into content.
- Keep the dashboard information set close to the reference and remove duplicated totals.
- Use the global Ratatui skill and Context7 docs before changing widget APIs.
- Require subagent review for spec/code/design and require real screenshot evidence before issue closure.
