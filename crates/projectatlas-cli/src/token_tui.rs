//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::telemetry::{
    TOKEN_ACCOUNTING_OBSERVED_DELTA, TOKEN_BASELINE_DIRECTORY_WALK, TokenBucketOverview,
    TokenOverview, TokenTrendReport,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::{Frame, Terminal};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 47;
/// Fixed terminal height for the token trend dashboard snapshot.
const TREND_DASHBOARD_HEIGHT: u16 = 30;
/// Fixed character width for each vertical token comparison column.
const TOKEN_COMPARE_COLUMN_WIDTH: usize = 24;
/// Number of glyph cells in the file-read avoidance cake chart.
const READ_AVOIDANCE_CAKE_SLOTS: usize = 12;

/// Render the token overview as a human terminal dashboard.
pub(crate) fn render_token_dashboard(overview: &TokenOverview, session: Option<&str>) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    render_dashboard_to_string(width, DASHBOARD_HEIGHT, |frame| {
        render_overview_frame(frame, overview, session);
    })
}

/// Render token trends as a human terminal dashboard.
pub(crate) fn render_token_trend_dashboard(report: &TokenTrendReport) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    render_dashboard_to_string(width, TREND_DASHBOARD_HEIGHT, |frame| {
        render_trend_frame(frame, report);
    })
}

/// Render one Ratatui frame into a deterministic string buffer.
fn render_dashboard_to_string<F>(width: u16, height: u16, render: F) -> String
where
    F: FnOnce(&mut Frame<'_>),
{
    let backend = TestBackend::new(width, height);
    let mut terminal =
        Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
    let frame = terminal
        .draw(render)
        .expect("in-memory token dashboard should render");
    buffer_to_string(frame.buffer)
}

/// Draw the full overview dashboard frame.
fn render_overview_frame(frame: &mut Frame<'_>, overview: &TokenOverview, session: Option<&str>) {
    let area = frame.area();
    let outer = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " ProjectAtlas Savings Overview ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{} ", session.unwrap_or("all sessions"))),
        ]))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(7),
            Constraint::Min(8),
            Constraint::Length(8),
        ])
        .split(inner);

    render_overview_summary(frame, sections[0], overview);
    render_overview_gauges(frame, sections[1], overview);
    render_overview_bars(frame, sections[2], overview);
    render_file_read_avoidance_chart(frame, sections[3], overview);
    render_bucket_table(frame, sections[4], overview);
    render_overview_notes(frame, sections[5], overview);
}

/// Draw the top overview metadata block.
fn render_overview_summary(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let text = vec![
        Line::from(vec![
            label("Lookups"),
            value(overview.calls),
            Span::raw("   "),
            label("Estimated tokens"),
            Span::raw("without "),
            value(overview.estimated_without_projectatlas),
            Span::raw(" / with "),
            value(overview.estimated_with_projectatlas),
        ]),
        Line::from(vec![
            label("Saved estimate"),
            signed_value(overview.legacy_gross_estimated_saved),
            Span::raw("   "),
            Span::raw(format!(
                "local {} estimate, not provider billing",
                overview.estimator
            )),
        ]),
    ];
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
}

/// Draw headline accounting gauges.
fn render_overview_gauges(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(area);
    let max_positive = overview
        .estimated_without_projectatlas
        .max(overview.estimated_with_projectatlas)
        .max(overview.tokens_avoided.max(0).unsigned_abs())
        .max(1);
    render_gauge(
        frame,
        chunks[0],
        "Total tokens avoided",
        overview.tokens_avoided,
        max_positive,
        Color::Green,
    );
    render_gauge(
        frame,
        chunks[1],
        "Measured from summaries",
        overview.measured_tokens_saved,
        max_positive,
        Color::Yellow,
    );
    render_gauge(
        frame,
        chunks[2],
        "Narrowed to right files",
        overview.deduped_modeled_tokens_avoided,
        max_positive,
        Color::Magenta,
    );
}

/// Draw one signed token gauge.
fn render_gauge(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: isize,
    max_positive: usize,
    color: Color,
) {
    let positive = value.max(0).unsigned_abs();
    let ratio = (positive as f64 / max_positive as f64).clamp(0.0, 1.0);
    let gauge = Gauge::default()
        .block(Block::bordered().title(title))
        .gauge_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
        .ratio(ratio)
        .label(format!("{} tokens", signed_count(value)));
    frame.render_widget(gauge, area);
}

/// Draw a vertical with/without `ProjectAtlas` token comparison.
fn render_overview_bars(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let max_value = overview
        .estimated_without_projectatlas
        .max(overview.estimated_with_projectatlas)
        .max(overview.tokens_avoided.max(0).unsigned_abs())
        .max(1);
    let block = Block::bordered().title("Tokens: with vs without ProjectAtlas");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let chart_height = usize::from(inner.height).saturating_sub(3).clamp(3, 6);
    frame.render_widget(
        Paragraph::new(vertical_token_comparison_lines(
            overview,
            max_value,
            chart_height,
        ))
        .alignment(Alignment::Center),
        inner,
    );
}

/// Return fixed-width vertical bars for the main token comparison.
fn vertical_token_comparison_lines(
    overview: &TokenOverview,
    max_value: usize,
    chart_height: usize,
) -> Vec<Line<'static>> {
    let columns = [
        (
            "Without ProjectAtlas",
            grouped_count(overview.estimated_without_projectatlas),
            overview.estimated_without_projectatlas,
            Color::Blue,
        ),
        (
            "With ProjectAtlas",
            grouped_count(overview.estimated_with_projectatlas),
            overview.estimated_with_projectatlas,
            Color::Cyan,
        ),
        (
            "Tokens avoided",
            signed_count(overview.tokens_avoided),
            overview.tokens_avoided.max(0).unsigned_abs(),
            Color::Green,
        ),
    ];

    let mut lines = Vec::with_capacity(chart_height + 2);
    for level in (1..=chart_height).rev() {
        let mut spans = Vec::with_capacity(columns.len() * 2);
        for (index, (_, _, value, color)) in columns.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("  "));
            }
            let filled = vertical_column_height(*value, max_value, chart_height);
            let marker = if filled >= level { "█" } else { " " };
            spans.push(Span::styled(
                format!("{marker:^TOKEN_COMPARE_COLUMN_WIDTH$}"),
                Style::default().fg(*color).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::from(
        columns
            .iter()
            .enumerate()
            .flat_map(|(index, (label, _, _, _))| {
                [
                    if index > 0 { "  " } else { "" }.to_string(),
                    format!("{label:^TOKEN_COMPARE_COLUMN_WIDTH$}"),
                ]
            })
            .map(|cell| {
                Span::styled(
                    cell,
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect::<Vec<_>>(),
    ));
    lines.push(Line::from(
        columns
            .iter()
            .enumerate()
            .flat_map(|(index, (_, value_text, _, color))| {
                [
                    if index > 0 { "  " } else { "" }.to_string(),
                    format!("{value_text:^TOKEN_COMPARE_COLUMN_WIDTH$}"),
                ]
                .map(|text| (text, *color))
            })
            .map(|(cell, color)| {
                Span::styled(
                    cell,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )
            })
            .collect::<Vec<_>>(),
    ));
    lines
}

/// Return how many rows should be filled for one vertical comparison column.
fn vertical_column_height(value: usize, max_value: usize, chart_height: usize) -> usize {
    if value == 0 || max_value == 0 || chart_height == 0 {
        0
    } else {
        (((value as f64 / max_value as f64) * chart_height as f64).ceil() as usize)
            .max(1)
            .min(chart_height)
    }
}

/// Draw a compact cake-style file-read avoidance mix.
fn render_file_read_avoidance_chart(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    frame.render_widget(
        Paragraph::new(read_avoidance_cake_lines(overview))
            .block(Block::bordered().title("File reads avoided"))
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Return cake-style lines for observed and modeled file-read avoidance.
fn read_avoidance_cake_lines(overview: &TokenOverview) -> Vec<Line<'static>> {
    let total = overview.likely_file_reads_avoided;
    let cells = read_avoidance_cake_cells(
        overview.observed_file_read_replacements,
        overview.modeled_file_reads_avoided,
        READ_AVOIDANCE_CAKE_SLOTS,
    );
    let top = cells.iter().take(4).collect::<String>();
    let middle = cells.iter().skip(4).take(4).collect::<String>();
    let bottom = cells.iter().skip(8).take(4).collect::<String>();

    vec![
        Line::from(vec![
            Span::styled(
                format!("      ◜{top}◝      "),
                Style::default().fg(Color::Green),
            ),
            label("Total likely avoided"),
            Span::raw(format!("{} file reads", grouped_count(total))),
        ]),
        Line::from(vec![
            Span::styled(
                format!("       {middle}       "),
                Style::default().fg(Color::Green),
            ),
            label("Observed summaries/slices"),
            value(overview.observed_file_read_replacements),
        ]),
        Line::from(vec![
            Span::styled(
                format!("      ◟{bottom}◞      "),
                Style::default().fg(Color::Magenta),
            ),
            label("Search-modeled narrowing"),
            value(overview.modeled_file_reads_avoided),
        ]),
        Line::from(vec![
            Span::styled(
                "      █ observed  ".to_string(),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                "▓ search-modeled  ".to_string(),
                Style::default().fg(Color::Magenta),
            ),
            label("confidence"),
            Span::raw(overview.read_avoidance_confidence.clone()),
        ]),
    ]
}

/// Return the cake cell mix for observed and modeled avoided file reads.
fn read_avoidance_cake_cells(observed: usize, modeled: usize, slots: usize) -> Vec<char> {
    let slots = slots.max(1);
    let total = observed.saturating_add(modeled);
    if total == 0 {
        return vec!['○'; slots];
    }
    let observed_slots = (((observed as f64 / total as f64) * slots as f64).round() as usize)
        .min(slots)
        .max(usize::from(observed > 0));
    let modeled_slots = slots.saturating_sub(observed_slots);
    let mut cells = vec!['█'; observed_slots];
    cells.extend(std::iter::repeat_n('▓', modeled_slots));
    cells.resize(slots, '○');
    cells
}

/// Draw the bucket table.
fn render_bucket_table(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut rows = overview
        .buckets
        .iter()
        .take(7)
        .map(|bucket| {
            Row::new(vec![
                Cell::from(bucket_display_name(bucket)),
                Cell::from(grouped_count(bucket.calls)),
                Cell::from(signed_count(bucket.estimated_saved)),
                Cell::from(bucket_plain_meaning(bucket)),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from("none"),
            Cell::from("no telemetry rows"),
            Cell::from(""),
            Cell::from("0"),
        ]));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(30),
            Constraint::Percentage(10),
            Constraint::Percentage(16),
            Constraint::Percentage(44),
        ],
    )
    .header(
        Row::new(vec![
            "How ProjectAtlas helped",
            "steps",
            "tokens",
            "plain meaning",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::bordered().title("Where the savings came from"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(table, area);
}

/// Return a plain label for one token-savings bucket.
fn bucket_display_name(bucket: &TokenBucketOverview) -> String {
    if bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA {
        "Summaries and slices".to_string()
    } else if bucket.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
        "Skipped broad folder walk".to_string()
    } else {
        "Opened fewer candidates".to_string()
    }
}

/// Explain one token-savings bucket without exposing accounting jargon first.
fn bucket_plain_meaning(bucket: &TokenBucketOverview) -> String {
    if bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA {
        "compact output replaced file reads".to_string()
    } else if bucket.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
        "ranking skipped broad folders".to_string()
    } else {
        "search/ranking narrowed files".to_string()
    }
}

/// Draw accounting notes and optional calibration metadata.
fn render_overview_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut lines = vec![
        Line::from(vec![
            label("Tokens avoided"),
            signed_value(overview.tokens_avoided),
        ]),
        Line::from(vec![
            label("Measured summaries"),
            signed_value(overview.measured_tokens_saved),
            Span::raw("   "),
            label("Narrowed navigation"),
            signed_value(overview.deduped_modeled_tokens_avoided),
        ]),
        Line::from(vec![
            label("File reads avoided"),
            value(overview.likely_file_reads_avoided),
        ]),
        Line::from(vec![
            Span::raw(" = observed "),
            value(overview.observed_file_read_replacements),
            Span::raw(" + search-modeled "),
            value(overview.modeled_file_reads_avoided),
            Span::raw(format!(" ({})", overview.read_avoidance_confidence)),
        ]),
        Line::from(vec![
            label("Repeated estimates handled"),
            value(overview.repeated_baselines_deduped),
            Span::raw(" duplicate navigation baselines collapsed"),
        ]),
    ];
    if let Some(calibration) = &overview.calibration {
        lines.push(Line::from(vec![
            label("calibration"),
            Span::raw(format!(
                "{} files, heuristic {}, {} {}",
                calibration.files,
                grouped_count(calibration.heuristic_tokens),
                calibration.tokenizer,
                grouped_count(calibration.calibrated_tokens)
            )),
        ]));
    } else {
        lines.push(Line::from(vec![
            label("calibration"),
            Span::raw(
                "optional: add --tokenizer o200k_base or cl100k_base for a local tokenizer audit",
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title("What this means"))
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// Draw the full trend dashboard frame.
fn render_trend_frame(frame: &mut Frame<'_>, report: &TokenTrendReport) {
    let area = frame.area();
    let outer = Block::bordered()
        .title(Line::from(vec![
            Span::styled(
                " ProjectAtlas Token Trends ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{} ", report.window)),
        ]))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(8),
            Constraint::Min(12),
            Constraint::Length(4),
        ])
        .split(inner);

    let summary = vec![
        Line::from(vec![
            label("session"),
            Span::raw(report.session.as_deref().unwrap_or("all sessions")),
            Span::raw("   "),
            label("window"),
            Span::raw(report.window.to_string()),
            Span::raw("   "),
            label("periods"),
            value(report.periods.len()),
        ]),
        Line::from(vec![label("estimate"), Span::raw(&report.estimate_scope)]),
    ];
    frame.render_widget(Paragraph::new(summary), sections[0]);

    let trend_values = report
        .periods
        .iter()
        .map(|period| saturating_usize_to_u64(period.estimated_saved.max(0).unsigned_abs()))
        .collect::<Vec<_>>();
    let spark_data = if trend_values.is_empty() {
        vec![0]
    } else {
        trend_values
    };
    frame.render_widget(
        Sparkline::default()
            .block(Block::bordered().title("Saved Tokens Trend"))
            .data(spark_data)
            .style(Style::default().fg(Color::Green)),
        sections[1],
    );

    render_trend_table(frame, sections[2], report);
    frame.render_widget(
        Paragraph::new(
            "Trend rows are period gross estimates. Use overview mode for deduped tokens avoided.",
        )
        .alignment(Alignment::Center)
        .block(Block::bordered().title("Note")),
        sections[3],
    );
}

/// Draw period rows for the trend dashboard.
fn render_trend_table(frame: &mut Frame<'_>, area: Rect, report: &TokenTrendReport) {
    let mut rows = report
        .periods
        .iter()
        .rev()
        .take(8)
        .map(|period| {
            Row::new(vec![
                Cell::from(period.period.clone()),
                Cell::from(signed_count(period.estimated_saved)),
                Cell::from(rate_label(period.savings_rate)),
                Cell::from(grouped_count(period.calls)),
                Cell::from(grouped_count(period.estimated_without_projectatlas)),
                Cell::from(grouped_count(period.estimated_with_projectatlas)),
            ])
        })
        .collect::<Vec<_>>();
    rows.reverse();
    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from("none"),
            Cell::from("0"),
            Cell::from("unknown"),
            Cell::from("0"),
            Cell::from("0"),
            Cell::from("0"),
        ]));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(18),
            Constraint::Percentage(16),
            Constraint::Percentage(13),
            Constraint::Percentage(10),
            Constraint::Percentage(21),
            Constraint::Percentage(22),
        ],
    )
    .header(
        Row::new(vec![
            "period", "saved", "rate", "calls", "baseline", "emitted",
        ])
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::bordered().title("Periods"));
    frame.render_widget(table, area);
}

/// Convert a Ratatui buffer into trimmed terminal text.
fn buffer_to_string(buffer: &Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut lines = Vec::with_capacity(height as usize);
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        lines.push(line.trim_end().to_string());
    }
    while matches!(lines.last(), Some(line) if line.is_empty()) {
        lines.pop();
    }
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

/// Styled field label span.
fn label(text: &str) -> Span<'static> {
    Span::styled(
        format!("{text}: "),
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
}

/// Styled unsigned value span.
fn value(value: usize) -> Span<'static> {
    Span::styled(
        grouped_count(value),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )
}

/// Styled signed value span.
fn signed_value(value: isize) -> Span<'static> {
    let color = if value >= 0 { Color::Green } else { Color::Red };
    Span::styled(
        signed_count(value),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

/// Format an optional savings rate.
fn rate_label(value: Option<f64>) -> String {
    value.map_or_else(
        || "unknown".to_string(),
        |rate| format!("{:.1}%", rate * 100.0),
    )
}

/// Return the preferred dashboard width.
fn dashboard_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(110)
}

/// Format an unsigned count with thousands separators.
fn grouped_count(value: usize) -> String {
    let raw = value.to_string();
    let mut grouped = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().enumerate() {
        if index > 0 && (raw.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(character);
    }
    grouped
}

/// Format a signed count with thousands separators.
fn signed_count(value: isize) -> String {
    if value < 0 {
        format!("-{}", grouped_count(value.unsigned_abs()))
    } else {
        grouped_count(usize::try_from(value).unwrap_or(usize::MAX))
    }
}

/// Convert `usize` to `u64` without panicking on unusual targets.
fn saturating_usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::{render_token_dashboard, render_token_trend_dashboard};
    use projectatlas_core::telemetry::{
        TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow, usage_from_estimates,
        usage_from_text,
    };

    #[test]
    fn overview_dashboard_renders_plain_language_savings_and_read_avoidance() {
        let events = [
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcdabcd",
                "ab",
            ),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 40),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 30),
        ];
        let dashboard = render_token_dashboard(&TokenOverview::from_events(&events), Some("s"));

        assert!(dashboard.contains("ProjectAtlas Savings Overview"));
        assert!(dashboard.contains("Total tokens avoided"));
        assert!(dashboard.contains("Measured from summaries"));
        assert!(dashboard.contains("Narrowed to right files"));
        assert!(dashboard.contains("Tokens: with vs without ProjectAtlas"));
        assert!(dashboard.contains("Without ProjectAtlas"));
        assert!(dashboard.contains("With ProjectAtlas"));
        assert!(dashboard.contains("Tokens avoided"));
        assert!(dashboard.contains("File reads avoided"));
        assert!(dashboard.contains("Total likely avoided"));
        assert!(dashboard.contains("Observed summaries/slices"));
        assert!(dashboard.contains("Search-modeled narrowing"));
        assert!(dashboard.contains("File reads avoided"));
        assert!(dashboard.contains("Where the savings came from"));
        assert!(dashboard.contains("Summaries and slices"));
        assert!(dashboard.contains("Opened fewer candidates"));
        assert!(dashboard.contains("search-modeled"));
        assert!(dashboard.contains("duplicate navigation baselines collapsed"));
        assert!(dashboard.contains("█") || dashboard.contains("▌") || dashboard.contains("▏"));
    }

    #[test]
    fn trend_dashboard_renders_sparkline_and_period_table() {
        let report = TokenTrendReport::new(
            Some("s".to_string()),
            TokenTrendWindow::Month,
            vec![
                TokenTrendPeriod::from_totals("2026-06".to_string(), 2, 200, 50),
                TokenTrendPeriod::from_totals("2026-07".to_string(), 1, 100, 80),
            ],
        );
        let dashboard = render_token_trend_dashboard(&report);

        assert!(dashboard.contains("ProjectAtlas Token Trends"));
        assert!(dashboard.contains("Saved Tokens Trend"));
        assert!(dashboard.contains("2026-06"));
        assert!(dashboard.contains("2026-07"));
        assert!(dashboard.contains("period"));
        assert!(dashboard.contains("█") || dashboard.contains("▅") || dashboard.contains("▁"));
    }
}
