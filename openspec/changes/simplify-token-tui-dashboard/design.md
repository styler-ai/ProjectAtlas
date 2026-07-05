## Dashboard Structure

The overview dashboard should render in this order:

1. Headline summary: conservative tokens avoided, avoided reads, lookup count, and estimator.
2. File Handling Optimization Overview: compact saved-token/read equations, observed source row, modeled source row, and observed-vs-modeled gauge.
3. Saved-token trends: day, week, month, and year signed charts.
4. What this means: only explanatory notes that do not repeat the headline numbers.

## Accounting Rules

The dashboard keeps one visible token-savings view:

- Conservative saved/avoided total: `measured_tokens_saved + deduped_modeled_tokens_avoided`.

The gross compatibility totals remain available through the token data contract, but the overview TUI does not render a separate gross saved value because it competes with the conservative headline and made the dashboard harder to audit.

The source table must use the conservative accounting view while staying backed by real telemetry bucket categories:

- Observed source buckets: observed full-file compression steps and `measured_tokens_saved`.
- Modeled source buckets: actual modeled telemetry buckets, with `deduped_modeled_tokens_avoided` allocated across those real buckets by gross modeled contribution.

Therefore:

- Source table steps sum to the overview lookup count represented by `calls`.
- Source table tokens sum to `tokens_avoided`.
- Do not add a visible total row to the source table, because that makes the visible source rows double-count when summed. The explicit equation strip carries the total once.
- File reads avoided equals `observed_file_read_replacements + modeled_file_reads_avoided`.
- Trend charts preserve signed period values instead of converting losses to positive magnitude.
- If observed or modeled token savings are negative, the token-mix gauge label shows signed operands and net value instead of unsigned percentages.

## Visual Rules

- Use Ratatui widgets rather than custom terminal drawing.
- Use signed Ratatui `Chart` widgets for saved-token trends.
- Use styled table headers with bottom margin and column spacing.
- Avoid visually separate sections that repeat the same accounting row names.
- Prefer direct labels such as `Observed reads` and `Modeled search` over long sentence-like headers.
- Keep the palette light-terminal friendly with restrained cyan, green, magenta, yellow, and blue accents.
