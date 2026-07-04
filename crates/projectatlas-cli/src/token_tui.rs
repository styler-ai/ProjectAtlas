//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::telemetry::{
    TOKEN_ACCOUNTING_OBSERVED_DELTA, TOKEN_BASELINE_DIRECTORY_WALK, TokenBucketOverview,
    TokenOverview, TokenTrendReport, TokenTrendWindow,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Bar, BarChart, Block, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::{Frame, Terminal};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 55;
/// Fixed terminal height for the token trend dashboard snapshot.
const TREND_DASHBOARD_HEIGHT: u16 = 30;

/// Render the token overview as a human terminal dashboard.
pub(crate) fn render_token_dashboard(
    overview: &TokenOverview,
    session: Option<&str>,
    trends: &[TokenTrendReport],
) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    render_dashboard_to_string(width, DASHBOARD_HEIGHT, |frame| {
        render_overview_frame(frame, overview, session, trends);
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
fn render_overview_frame(
    frame: &mut Frame<'_>,
    overview: &TokenOverview,
    session: Option<&str>,
    trends: &[TokenTrendReport],
) {
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
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(8),
        ])
        .split(inner);

    render_overview_summary(frame, sections[0], overview);
    render_overview_bars(frame, sections[1], overview);
    render_overview_trends(frame, sections[2], trends);
    render_file_handling_optimization_overview(frame, sections[3], overview);
    render_overview_gauges(frame, sections[4], overview);
    render_bucket_table(frame, sections[5], overview);
    render_overview_notes(frame, sections[6], overview);
}

/// Draw the top overview metadata block.
fn render_overview_summary(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let text = vec![
        Line::from(vec![
            label("Lookups"),
            value(overview.calls),
            Span::raw("   "),
            label("Gross estimate"),
            Span::raw("without "),
            value(overview.estimated_without_projectatlas),
            Span::raw(" / with "),
            value(overview.estimated_with_projectatlas),
        ]),
        Line::from(vec![
            label("Gross saved (without - with)"),
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
        "Saved/avoided tokens",
        overview.tokens_avoided,
        max_positive,
        Color::Green,
    );
    render_gauge(
        frame,
        chunks[1],
        "Measured summaries",
        overview.measured_tokens_saved,
        max_positive,
        Color::Yellow,
    );
    render_gauge(
        frame,
        chunks[2],
        "Narrowed files",
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
        .max(overview.legacy_gross_estimated_saved.max(0).unsigned_abs())
        .max(1);
    let block = Block::bordered().title("Gross tokens: without vs with ProjectAtlas");
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(5), Constraint::Min(3)])
        .split(inner);
    let bars = token_comparison_columns(overview)
        .into_iter()
        .map(|(label, _value_text, value, color)| {
            Bar::with_label(
                token_comparison_short_label(label),
                saturating_usize_to_u64(value),
            )
            .text_value("")
            .style(Style::default().fg(color))
            .value_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        BarChart::vertical(bars)
            .max(saturating_usize_to_u64(max_value))
            .bar_width(12)
            .bar_gap(6)
            .label_style(Style::default().fg(Color::DarkGray))
            .value_style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        sections[0],
    );
    render_token_comparison_totals(frame, sections[1], overview);
}

/// Return the values used by the with/without/saved comparison chart.
fn token_comparison_columns(overview: &TokenOverview) -> [(&'static str, String, usize, Color); 3] {
    [
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
            "Saved by ProjectAtlas",
            signed_count(overview.legacy_gross_estimated_saved),
            overview.legacy_gross_estimated_saved.max(0).unsigned_abs(),
            Color::Green,
        ),
    ]
}

/// Return a short label that fits under a compact `BarChart` bar.
fn token_comparison_short_label(label: &str) -> &'static str {
    match label {
        "Without ProjectAtlas" => "Without",
        "With ProjectAtlas" => "With",
        "Saved by ProjectAtlas" => "Saved",
        _ => "Value",
    }
}

/// Draw exact totals below the compact token comparison chart.
fn render_token_comparison_totals(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let columns = token_comparison_columns(overview);
    let table = Table::new(
        [
            Row::new([
                Cell::from(columns[0].0),
                Cell::from(columns[1].0),
                Cell::from(columns[2].0),
            ])
            .style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ),
            Row::new([
                Cell::from(columns[0].1.clone()).style(
                    Style::default()
                        .fg(columns[0].3)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(columns[1].1.clone()).style(
                    Style::default()
                        .fg(columns[1].3)
                        .add_modifier(Modifier::BOLD),
                ),
                Cell::from(columns[2].1.clone()).style(
                    Style::default()
                        .fg(columns[2].3)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ],
        [
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ],
    )
    .column_spacing(2);
    frame.render_widget(table, area);
}

/// Draw compact saved-token trends for the standard reporting windows.
fn render_overview_trends(frame: &mut Frame<'_>, area: Rect, trends: &[TokenTrendReport]) {
    let block = Block::bordered().title("Saved-token trends");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    let areas = [
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[0]),
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]),
    ];
    let windows = [
        TokenTrendWindow::Day,
        TokenTrendWindow::Week,
        TokenTrendWindow::Month,
        TokenTrendWindow::Year,
    ];
    for (index, window) in windows.into_iter().enumerate() {
        let trend = trends.iter().find(|report| report.window == window);
        let area = areas[index / 2][index % 2];
        render_trend_sparkline(frame, area, window, trend);
    }
}

/// Draw one compact saved-token trend strip.
fn render_trend_sparkline(
    frame: &mut Frame<'_>,
    area: Rect,
    window: TokenTrendWindow,
    trend: Option<&TokenTrendReport>,
) {
    let periods = trend.map_or(0, |report| report.periods.len());
    let latest = trend.and_then(|report| report.periods.last()).map_or_else(
        || "no data".to_string(),
        |period| signed_count(period.estimated_saved),
    );
    let data = trend
        .map(|report| {
            report
                .periods
                .iter()
                .map(|period| saturating_usize_to_u64(period.estimated_saved.max(0).unsigned_abs()))
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| vec![0]);
    frame.render_widget(
        Sparkline::default()
            .block(Block::bordered().title(format!("{window} | latest {latest} | {periods}p")))
            .data(data)
            .style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        area,
    );
}

/// Draw a compact table-style file-handling optimization overview.
fn render_file_handling_optimization_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    overview: &TokenOverview,
) {
    let block = Block::bordered().title("File Handling Optimization Overview");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(file_handling_saved_tokens_line(overview)).wrap(Wrap { trim: true }),
        sections[0],
    );
    frame.render_widget(
        Paragraph::new(file_handling_reads_line(overview)).wrap(Wrap { trim: true }),
        sections[1],
    );
    let observed_ratio = ratio(
        overview.observed_file_read_replacements,
        overview.likely_file_reads_avoided,
    );
    frame.render_widget(
        Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .ratio(observed_ratio)
            .label(format!(
                "observed {} / modeled {}",
                percentage_label(
                    overview.observed_file_read_replacements,
                    overview.likely_file_reads_avoided
                ),
                percentage_label(
                    overview.modeled_file_reads_avoided,
                    overview.likely_file_reads_avoided
                )
            )),
        sections[2],
    );
    render_file_handling_table(frame, sections[3], overview);
}

/// Return the conservative saved-token equation for file handling.
fn file_handling_saved_tokens_line(overview: &TokenOverview) -> Line<'static> {
    Line::from(vec![
        label("Saved/avoided tokens"),
        signed_value(overview.tokens_avoided),
        Span::raw(" = "),
        signed_value(overview.measured_tokens_saved),
        Span::raw(" observed + "),
        signed_value(overview.deduped_modeled_tokens_avoided),
        Span::raw(" avoided"),
    ])
}

/// Return the headline file-read-avoidance equation.
fn file_handling_reads_line(overview: &TokenOverview) -> Line<'static> {
    Line::from(vec![
        label("File reads avoided"),
        value(overview.likely_file_reads_avoided),
        Span::raw(" = observed "),
        value(overview.observed_file_read_replacements),
        Span::raw(" + search-modeled "),
        value(overview.modeled_file_reads_avoided),
        Span::raw(format!(" ({})", overview.read_avoidance_confidence)),
    ])
}

/// Draw observed and modeled file-handling rows.
fn render_file_handling_table(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let rows = [
        Row::new(vec![
            Cell::from("Observed summary/slices"),
            Cell::from(grouped_count(overview.observed_file_read_replacements)),
            Cell::from(signed_count(overview.measured_tokens_saved)),
            Cell::from("replaced reads"),
        ])
        .style(Style::default().fg(Color::Green)),
        Row::new(vec![
            Cell::from("Search-modeled narrowing"),
            Cell::from(grouped_count(overview.modeled_file_reads_avoided)),
            Cell::from(signed_count(overview.deduped_modeled_tokens_avoided)),
            Cell::from("avoided opens"),
        ])
        .style(Style::default().fg(Color::Magenta)),
    ];
    let table = Table::new(
        rows,
        [
            Constraint::Length(24),
            Constraint::Length(8),
            Constraint::Length(18),
            Constraint::Min(18),
        ],
    )
    .header(
        Row::new(vec!["Source", "reads", "saved tokens", "meaning"])
            .bottom_margin(1)
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .column_spacing(2);
    frame.render_widget(table, area);
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
            Constraint::Length(18),
            Constraint::Length(7),
            Constraint::Length(16),
            Constraint::Min(20),
        ],
    )
    .header(
        Row::new(vec!["How it helped", "steps", "tokens", "meaning"])
            .bottom_margin(1)
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    )
    .block(Block::bordered().title("Where the savings came from"))
    .column_spacing(2)
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(table, area);
}

/// Return a plain label for one token-savings bucket.
fn bucket_display_name(bucket: &TokenBucketOverview) -> String {
    if bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA {
        "Summaries/slices".to_string()
    } else if bucket.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
        "Skipped folders".to_string()
    } else {
        "Fewer candidates".to_string()
    }
}

/// Explain one token-savings bucket without exposing accounting jargon first.
fn bucket_plain_meaning(bucket: &TokenBucketOverview) -> String {
    if bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA {
        "replaced reads".to_string()
    } else if bucket.denominator_kind == TOKEN_BASELINE_DIRECTORY_WALK {
        "skipped folders".to_string()
    } else {
        "narrowed files".to_string()
    }
}

/// Draw accounting notes and optional calibration metadata.
fn render_overview_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut lines = vec![
        Line::from(vec![
            label("Conservative tokens avoided"),
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
            Span::raw("optional tokenizer audit: --tokenizer o200k_base or cl100k_base"),
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

/// Format one part of a whole as a nearest integer percentage.
fn percentage_label(part: usize, total: usize) -> String {
    if total == 0 {
        "0%".to_string()
    } else {
        format!("{:.0}%", (part as f64 / total as f64) * 100.0)
    }
}

/// Return a stable ratio for Ratatui gauges.
fn ratio(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64).clamp(0.0, 1.0)
    }
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
    use super::{
        DASHBOARD_HEIGHT, dashboard_width, render_overview_frame, render_token_dashboard,
        render_token_trend_dashboard, signed_count, token_comparison_columns,
    };
    use projectatlas_core::telemetry::{
        TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow, usage_from_estimates,
        usage_from_text,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};

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
        let trends = sample_trends();
        let dashboard =
            render_token_dashboard(&TokenOverview::from_events(&events), Some("s"), &trends);

        assert!(dashboard.contains("ProjectAtlas Savings Overview"));
        assert!(dashboard.contains("Conservative tokens avoided"));
        assert!(dashboard.contains("Measured summaries"));
        assert!(dashboard.contains("Narrowed files"));
        assert!(dashboard.contains("Gross tokens: without vs with ProjectAtlas"));
        assert!(dashboard.contains("Saved-token trends"));
        assert!(dashboard.contains("day | latest"));
        assert!(dashboard.contains("week | latest"));
        assert!(dashboard.contains("month | latest"));
        assert!(dashboard.contains("year | latest"));
        assert!(dashboard.contains("Without ProjectAtlas"));
        assert!(dashboard.contains("With ProjectAtlas"));
        assert!(dashboard.contains("Saved by ProjectAtlas"));
        assert!(dashboard.contains("File Handling Optimization Overview"));
        assert!(dashboard.contains("Saved/avoided tokens"));
        assert!(dashboard.contains("File reads avoided"));
        assert!(dashboard.contains("saved tokens"));
        assert!(dashboard.contains("Observed summary/slices"));
        assert!(dashboard.contains("Search-modeled narrowing"));
        assert!(dashboard.contains("Where the savings came from"));
        assert!(dashboard.contains("Summaries/slices"));
        assert!(dashboard.contains("Fewer candidates"));
        assert!(dashboard.contains("search-modeled"));
        assert!(dashboard.contains("duplicate navigation baselines collapsed"));
        assert!(dashboard.contains("█") || dashboard.contains("▌") || dashboard.contains("▏"));

        let compare = dashboard
            .find("Gross tokens: without vs with ProjectAtlas")
            .unwrap_or(usize::MAX);
        let trend = dashboard.find("Saved-token trends").unwrap_or(usize::MAX);
        let file_reads = dashboard
            .find("File Handling Optimization Overview")
            .unwrap_or(usize::MAX);
        let buckets = dashboard
            .find("Where the savings came from")
            .unwrap_or(usize::MAX);
        let notes = dashboard.find("What this means").unwrap_or(usize::MAX);
        assert!(compare < trend);
        assert!(trend < file_reads);
        assert!(file_reads < buckets);
        assert!(buckets < notes);

        assert_header_margin(&dashboard, "Source", "Observed summary/slices");
        assert_header_margin(&dashboard, "How it helped", "Summaries/slices");
    }

    #[test]
    fn overview_dashboard_uses_ratatui_widget_styles_for_key_headers() {
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
        let overview = TokenOverview::from_events(&events);
        let trends = sample_trends();
        let buffer = render_overview_buffer(&overview, Some("s"), &trends);

        assert_cell_style(&buffer, "Source", Color::Cyan, Modifier::BOLD);
        assert_cell_style(&buffer, "saved tokens", Color::Cyan, Modifier::BOLD);
        assert_cell_style(&buffer, "How it helped", Color::Cyan, Modifier::BOLD);
        assert_cell_style(
            &buffer,
            "Saved by ProjectAtlas",
            Color::DarkGray,
            Modifier::BOLD,
        );
    }

    #[test]
    fn overview_dashboard_fields_use_consistent_accounting_layers() {
        let overview = TokenOverview::from_events(&[
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
            usage_from_estimates("s", "search", None, Some("token".to_string()), 400, 20),
        ]);
        let gross_saved = overview.legacy_gross_estimated_saved;
        let conservative_avoided = overview.tokens_avoided;

        assert_eq!(
            overview.estimated_without_projectatlas as isize
                - overview.estimated_with_projectatlas as isize,
            gross_saved
        );
        assert_eq!(overview.estimated_saved, gross_saved);
        assert_eq!(
            overview.measured_tokens_saved + overview.deduped_modeled_tokens_avoided,
            conservative_avoided
        );
        assert_ne!(gross_saved, conservative_avoided);
        assert_eq!(
            overview.observed_file_read_replacements + overview.modeled_file_reads_avoided,
            overview.likely_file_reads_avoided
        );
        for bucket in &overview.buckets {
            assert_eq!(
                bucket.estimated_without_projectatlas as isize
                    - bucket.estimated_with_projectatlas as isize,
                bucket.estimated_saved
            );
        }

        let columns = token_comparison_columns(&overview);
        assert_eq!(columns[0].0, "Without ProjectAtlas");
        assert_eq!(columns[0].2, overview.estimated_without_projectatlas);
        assert_eq!(columns[1].0, "With ProjectAtlas");
        assert_eq!(columns[1].2, overview.estimated_with_projectatlas);
        assert_eq!(columns[2].0, "Saved by ProjectAtlas");
        assert_eq!(columns[2].1, signed_count(gross_saved));
        assert_eq!(columns[2].2 as isize, gross_saved);
        assert_ne!(columns[2].1, signed_count(conservative_avoided));

        let trends = sample_trends();
        let dashboard = render_token_dashboard(&overview, Some("s"), &trends);
        assert!(dashboard.contains(&signed_count(gross_saved)));
        assert!(dashboard.contains(&signed_count(conservative_avoided)));
        assert!(dashboard.contains(&format!(
            "{} = {} observed + {} avoided",
            signed_count(conservative_avoided),
            signed_count(overview.measured_tokens_saved),
            signed_count(overview.deduped_modeled_tokens_avoided)
        )));
        assert!(dashboard.contains("observed 1 + search-modeled 3"));
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

    fn sample_trends() -> Vec<TokenTrendReport> {
        [
            TokenTrendWindow::Day,
            TokenTrendWindow::Week,
            TokenTrendWindow::Month,
            TokenTrendWindow::Year,
        ]
        .into_iter()
        .map(|window| {
            TokenTrendReport::new(
                Some("s".to_string()),
                window,
                vec![
                    TokenTrendPeriod::from_totals(format!("2026-07-01-{window}"), 2, 200, 120),
                    TokenTrendPeriod::from_totals(format!("2026-07-02-{window}"), 3, 500, 125),
                    TokenTrendPeriod::from_totals(format!("2026-07-03-{window}"), 1, 100, 80),
                ],
            )
        })
        .collect()
    }

    fn render_overview_buffer(
        overview: &TokenOverview,
        session: Option<&str>,
        trends: &[TokenTrendReport],
    ) -> Buffer {
        let width = dashboard_width().clamp(80, 140) as u16;
        let backend = TestBackend::new(width, DASHBOARD_HEIGHT);
        let mut terminal =
            Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
        let frame = terminal
            .draw(|frame| render_overview_frame(frame, overview, session, trends))
            .expect("in-memory token dashboard should render");
        frame.buffer.clone()
    }

    fn assert_header_margin(dashboard: &str, header: &str, first_row: &str) {
        let header_index = dashboard.lines().position(|line| line.contains(header));
        assert!(
            header_index.is_some(),
            "dashboard should contain table header {header:?}"
        );
        let Some(header_index) = header_index else {
            return;
        };
        let row_index = dashboard.lines().position(|line| line.contains(first_row));
        assert!(
            row_index.is_some(),
            "dashboard should contain first table row {first_row:?}"
        );
        let Some(row_index) = row_index else {
            return;
        };
        assert!(
            row_index >= header_index + 2,
            "expected a visible separator row between {header:?} and {first_row:?}"
        );
    }

    fn assert_cell_style(buffer: &Buffer, text: &str, color: Color, modifier: Modifier) {
        let found = find_text(buffer, text);
        assert!(found.is_some(), "rendered buffer should contain {text:?}");
        let Some((x, y)) = found else {
            return;
        };
        let cell = buffer.cell((x, y));
        assert!(
            cell.is_some(),
            "located text should resolve to a buffer cell"
        );
        let Some(cell) = cell else {
            return;
        };
        assert_eq!(cell.fg, color, "unexpected foreground color for {text:?}");
        assert!(
            cell.modifier.contains(modifier),
            "missing modifier {modifier:?} for {text:?}"
        );
    }

    fn find_text(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer.cell((x, y))?.symbol());
            }
            if let Some(index) = line.find(text) {
                return Some((u16::try_from(index).ok()?, y));
            }
        }
        None
    }
}
