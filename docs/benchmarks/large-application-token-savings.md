# Large Application Token Savings Finding

A large application audit that motivated the ProjectAtlas 3 hardening pass showed the intended effect of the atlas-first workflow: the agent used folder and file orientation, then selected exact summaries and slices instead of rereading broad source trees.

The positive finding recorded in issue #153 was a 99.8% token-savings observation on a representative application corpus. The exact percentage is workload-specific, but it validates the product target:

- Start with repository overview and folder purpose.
- Narrow to likely files with file purpose and content summary.
- Escalate to symbol summaries and exact slices only after choosing the right target.
- Track saved tokens as avoided broad source reads, avoided wrong-file reads, and avoided unnecessary full-file reads.

## Corpus Scale

- Files: 679.
- Folders: 206.
- Indexed text files: 554.
- Indexed text bytes: 7,088,446.
- Symbols: 5,145.
- Relations: 12,122.
- Token telemetry calls: 142.

## Token Telemetry

- Estimated without ProjectAtlas: 221,114,448 tokens.
- Estimated with ProjectAtlas: 425,622 tokens.
- Estimated saved: 220,688,826 tokens.
- Savings rate: 99.8%.

## Responsiveness Sample

Representative warm CLI reads from the same audit stayed around 160-166 ms:

- `projectatlas summary <large-source-file> --limit 25`: approximately 165 ms.
- `projectatlas files workflow --folder .github/workflows --limit 20`: approximately 164 ms.
- `projectatlas token`: approximately 161 ms.
- `projectatlas overview`: approximately 166 ms.

ProjectAtlas token telemetry reports this through `projectatlas token` and `projectatlas token --view tui`. The telemetry is estimate-based; the default estimator is the offline `chars/bytes / 4` workflow heuristic, not provider or model billing-token accounting. It is designed for trend and workflow validation rather than billing reconciliation.
