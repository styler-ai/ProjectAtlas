## Context

The current `projectatlas token --view tui` overview is already backed by Ratatui, but its structure reflects the earlier simplification work rather than the supplied reference design. The new target is a more polished dashboard:

- reference image: `docs/design/token-impact-tui-reference.png`,
- mascot reference: `docs/design/ani-mascot-reference.png`,
- terminal overview surface: `crates/projectatlas-cli/src/token_tui.rs`.

The design must remain deterministic in tests because the CLI renders the TUI through Ratatui's `TestBackend` and writes the buffer as text.

## Layout Contract

The overview dashboard should render with a dark, high-contrast terminal style that follows `docs/design/token-impact-tui-reference.png` directly. Do not replace the reference design with a different interpretation. The required section order is:

1. Header band:
   - small terminal-native Ani pixel/block mascot,
   - `ProjectAtlas Token Impact`,
   - supporting text `Smarter context. Fewer tokens. Real savings.`,
   - right-side metadata for session, lookup count, and estimator.
2. Hero savings panel:
   - title `TOTAL TOKENS AVOIDED`,
   - dominant green saved-token number,
   - equation row: `Without ProjectAtlas - With ProjectAtlas = Saved by ProjectAtlas`,
   - small Ratatui gauges/bars under each operand.
3. File-read strip:
   - label `FILE READS AVOIDED`,
   - total likely file reads avoided,
   - observed summary/slice reads with percentage/bar,
   - search-modeled narrowing with percentage/bar,
   - confidence label.
4. Middle panels:
   - `SAVINGS COMPOSITION` for observed-vs-modeled token mix,
   - `SIGNAL` for deduped baselines, estimate type, and tokenizer audit.
5. Source table:
   - heading `WHERE THE SAVINGS CAME FROM`,
   - rows for summaries/slices, skipped broad folder walk if data exists, and narrowed candidates,
   - styled headers with enough spacing and a bottom margin.
6. `CALIBRATION & NOTES`:
   - local-estimate disclaimer,
   - observed and modeled read counts,
   - optional tokenizer calibration command or tokenizer result.
7. Footer/status row:
   - ProjectAtlas version on the left,
   - compact key hints such as `q Quit`, `? Help`, `r Refresh`,
   - Auto status and a clock timestamp on the right.

At narrow widths, the same information may compress labels and hide purely decorative spacing, but it must preserve the title, Ani mark, headline number, equation, file-read total, observed/modelled split, composition/signal panels, source table, notes, and footer/status row.

## Accounting Contract

The screenshot demonstrates the exact fields to keep. ProjectAtlas must use the same visible fields, but the arithmetic must add up. Keep the calculation simple:

- `saved_by_projectatlas = tokens_avoided`,
- `with_projectatlas = estimated_with_projectatlas`,
- `without_projectatlas = with_projectatlas + saved_by_projectatlas`.

Therefore the displayed equation always satisfies:

`without_projectatlas - with_projectatlas = saved_by_projectatlas`.

The source table uses conservative rows:

- summaries/slices: observed full-file compression buckets and `measured_tokens_saved`,
- modeled rows: real modeled telemetry bucket categories, with `deduped_modeled_tokens_avoided` allocated across those buckets by their gross modeled contribution so the visible table remains conservative.

The visible row labels should match the screenshot when the corresponding bucket exists:

- `Summaries and slices`,
- `Skipped broad folder walk`,
- `Opened fewer candidates (A)`,
- `Opened fewer candidates (B)`.

Do not fabricate modeled sub-buckets from `modeled_file_reads_avoided`. If telemetry only has one modeled bucket, render one modeled source row. If telemetry has directory-walk and selected-candidate buckets, render the matching screenshot-style rows.

Visible source table token rows must sum to `tokens_avoided`; visible source table steps must sum to the overview lookup count represented by `calls`. File-read arithmetic remains in the `FILE READS AVOIDED` strip and must satisfy `observed_file_read_replacements + modeled_file_reads_avoided = likely_file_reads_avoided`.

Gross compatibility totals remain available in JSON/TOON reports, but the overview TUI does not present a separate competing gross saved-token headline.

## Ratatui Implementation

Use Ratatui standard widgets and style primitives:

- `Layout` for section splitting,
- `Block` for bordered sections,
- `Paragraph` with styled `Line`/`Span` for title, Ani, equations, and metadata,
- `Gauge` for compact contribution bars,
- `Table` for source rows,
- existing `Chart` usage for the dedicated trend dashboard, not for the overview KPI screen.

Do not add terminal bitmap support or a custom renderer. Ani is a tiny Unicode/block/line mascot composed in styled text. The mark should be recognizable as a small pirate/cartographer mascot without requiring image-capable terminals.

## Color Roles

Use these screenshot-matched theme roles unless a terminal compatibility issue forces a close named-color fallback:

- background: near-black navy, approximately `Rgb(5, 14, 26)`,
- panel fill: dark blue, approximately `Rgb(9, 24, 42)`,
- borders/dividers: muted blue, approximately `Rgb(44, 77, 112)`,
- heading blue: bright ProjectAtlas blue, approximately `Rgb(76, 132, 255)`,
- ProjectAtlas identity and ProjectAtlas-origin metrics: ivory/off-white, approximately `Rgb(238, 234, 224)`, matching Ani and the `ProjectAtlas` title treatment,
- original/counterfactual baseline values: bright ProjectAtlas blue, including the `Without ProjectAtlas` operand/bar,
- ProjectAtlas work/output values: ivory/off-white, including the `With ProjectAtlas` operand/bar, file reads avoided total, observed summaries/slices count/bar, measured summaries/slices composition bar, and `Summaries and slices` source-table row,
- saved-token green: bright green, approximately `Rgb(108, 220, 116)`,
- modeled/confidence yellow: warm yellow, approximately `Rgb(238, 190, 62)`,
- body text: pale blue-gray, approximately `Rgb(210, 220, 245)`,
- muted labels: gray-blue, approximately `Rgb(130, 145, 175)`,
- negative savings: red.

The dashboard color language is semantic, not decorative: blue means the original baseline, ivory means ProjectAtlas-generated work or ProjectAtlas identity, green means net savings, and yellow means modeled estimates. Do not recolor ProjectAtlas-origin metrics blue simply because blue is the section-heading color.

Tests should verify color/style on important labels through the Ratatui buffer where practical, but should not overfit every decorative character.

## Documentation

README/docs should mention:

- `projectatlas token --view tui` opens the token impact dashboard,
- Ani is the ProjectAtlas mascot,
- the versioned design references live in `docs/design/`.

## Verification Approach

- Unit tests for the rendered overview buffer:
  - section order,
  - required labels,
  - Ani mark presence,
  - accounting equation,
  - file-read equation,
  - table header spacing/styling,
  - source row sums,
  - no competing gross headline.
- CLI smoke test for `projectatlas token --view tui`.
- A local visual inspection by running the rebuilt CLI and capturing/reviewing output at common widths.

## Pre-Mortem

Risk: the terminal mark for Ani becomes unreadable at 80 columns.
Mitigation: use a very small 3-5 line mark, keep it optional/compressed at narrow width, and assert the `Ani` label/title remains visible.

Risk: the dark palette is inaccessible in some terminals.
Mitigation: rely on standard named Ratatui colors and keep plain text labels meaningful without color.

Risk: fitting trend charts plus the reference layout overflows the fixed dashboard height.
Mitigation: keep overview mode as a KPI dashboard and preserve full detailed trends through `--trend`.

Risk: visual snapshot tests become too brittle.
Mitigation: assert important labels, order, equations, styles, and table spacing rather than exact full-frame borders.
