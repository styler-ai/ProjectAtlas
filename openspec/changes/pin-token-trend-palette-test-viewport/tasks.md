## 1. Deterministic Token Trend Palette Regression

- [x] 1.1 Freeze the regression contract: this test owns an explicit 140-by-30 full-trend viewport, while real token TUI invocations retain live terminal capture and compact selection below 80 by 30.
- [x] 1.2 Change only `trend_dashboard_light_theme_remaps_semantic_palette` to render through `render_token_trend_dashboard_with_theme_in_viewport(&report, TokenDashboardTheme::Light, test_viewport(140, 30))`, retaining every existing semantic palette assertion.
- [ ] 1.3 Pass the exact regression repeatedly in non-TTY and approximately 80-by-24 PTY execution; pass `cargo test -p projectatlas-cli --bin projectatlas --all-features --locked token_tui::tests`, `cargo fmt --check`, required hosted CI, strict OpenSpec validation, and the IssueOps checklist gate without changing production viewport behavior.
- [x] 1.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
