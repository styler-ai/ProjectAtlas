# SwipingGale Token Savings Finding

The SwipingGale audit that motivated the ProjectAtlas 3 hardening pass showed the intended effect of the atlas-first workflow: the agent used folder and file orientation, then selected exact summaries and slices instead of rereading broad source trees.

The positive finding recorded in issue #153 was a 99.8% token-savings observation on a large application audit. The exact percentage is workload-specific, but it validates the product target:

- Start with repository overview and folder purpose.
- Narrow to likely files with file purpose and observed summary.
- Escalate to symbol summaries and exact slices only after choosing the right target.
- Track saved tokens as avoided broad source reads, avoided wrong-file reads, and avoided unnecessary full-file reads.

ProjectAtlas token telemetry reports this through `projectatlas token` and `projectatlas token --view tui`. The telemetry is estimate-based; it is designed for trend and workflow validation rather than billing reconciliation.
