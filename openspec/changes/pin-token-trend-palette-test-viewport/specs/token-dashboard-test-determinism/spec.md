## ADDED Requirements

### Requirement: Layout-specific token-dashboard regressions own their viewport

A token-dashboard regression that asserts behavior unique to the full or compact layout SHALL render through an explicit deterministic viewport that selects the intended layout. It SHALL NOT depend on terminal discovery, process-global dimension mutation, test order, or the host PTY dimensions.

#### Scenario: Full-layout palette regression runs inside a short PTY

- **WHEN** `trend_dashboard_light_theme_remaps_semantic_palette` runs in an approximately 80-by-24 PTY
- **THEN** the test SHALL render through `test_viewport(140, 30)` and the existing viewport-injected trend helper
- **AND** it SHALL assert the full-layout light semantic palette rather than inheriting the PTY's compact-layout choice.

### Requirement: Production viewport behavior remains authoritative

Real token TUI invocations SHALL continue to capture one live terminal viewport and select the full trend layout only at 80 by 30 or larger. The deterministic test viewport SHALL NOT alter production capture, layout thresholds, compact behavior, public output contracts, or telemetry data.

#### Scenario: Production trend runs inside a short terminal

- **WHEN** a real token trend request runs below 80 columns or 30 rows
- **THEN** ProjectAtlas SHALL continue to render the existing bounded compact trend layout
- **AND** no test-only viewport SHALL affect that production decision.

### Requirement: The regression is verified across terminal contexts

The exact palette regression SHALL pass repeatedly in non-TTY and approximately 80-by-24 PTY execution while retaining its semantic assertions. The focused token TUI tests and repository-required format, hosted CI, strict OpenSpec, and IssueOps gates SHALL remain green.

#### Scenario: Verification executes in both contexts

- **WHEN** the test-only correction is evaluated for merge
- **THEN** repeated non-TTY and PTY runs SHALL select the same explicit full-layout test viewport and pass
- **AND** existing compact/full production-boundary regressions SHALL continue to pass unchanged.
