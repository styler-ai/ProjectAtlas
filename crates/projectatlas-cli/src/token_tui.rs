//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::telemetry::{
    TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Table, Wrap,
};
use ratatui::{Frame, Terminal};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 35;
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
            Constraint::Length(3),
            Constraint::Length(10),
            Constraint::Length(12),
            Constraint::Min(8),
        ])
        .split(inner);

    render_overview_summary(frame, sections[0], overview);
    render_file_handling_optimization_overview(frame, sections[1], overview);
    render_overview_trends(frame, sections[2], trends);
    render_overview_notes(frame, sections[3], overview);
}

/// Draw the top overview metadata block.
fn render_overview_summary(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let text = vec![
        Line::from(vec![
            label("Conservative tokens avoided"),
            signed_value(overview.tokens_avoided),
            Span::raw("   "),
            label("Avoided reads"),
            value(overview.likely_file_reads_avoided),
        ]),
        Line::from(vec![
            label("Lookups"),
            value(overview.calls),
            Span::raw("   "),
            label("Estimate"),
            Span::raw("local heuristic (not billing tokens)"),
        ]),
    ];
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
}

/// Draw compact saved-token trends for the standard reporting windows.
fn render_overview_trends(frame: &mut Frame<'_>, area: Rect, trends: &[TokenTrendReport]) {
    let block = Block::bordered()
        .title("Saved-token trends")
        .border_style(Style::default().fg(Color::DarkGray));
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
        render_trend_chart(frame, area, window, trend);
    }
}

/// Draw one compact saved-token trend strip.
fn render_trend_chart(
    frame: &mut Frame<'_>,
    area: Rect,
    window: TokenTrendWindow,
    trend: Option<&TokenTrendReport>,
) {
    let periods = trend.map_or(0, |report| report.periods.len());
    let period_label = if periods == 1 { "period" } else { "periods" };
    let points = signed_trend_points(trend.map(|report| report.periods.as_slice()));
    let [lower, upper] = signed_y_bounds(&points);
    let trend_color = signed_trend_color(&points);
    frame.render_widget(
        Chart::new(vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(
                    Style::default()
                        .fg(trend_color)
                        .add_modifier(Modifier::BOLD),
                )
                .data(&points),
        ])
        .block(Block::bordered().title(format!("{window} trend | {periods} {period_label}")))
        .x_axis(Axis::default().bounds([0.0, (points.len().saturating_sub(1)) as f64]))
        .y_axis(Axis::default().bounds([lower, upper])),
        area,
    );
}

/// Convert trend periods into signed chart coordinates.
fn signed_trend_points(periods: Option<&[TokenTrendPeriod]>) -> Vec<(f64, f64)> {
    let mut points = periods
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, period)| (index as f64, period.estimated_saved as f64))
        .collect::<Vec<_>>();
    if points.is_empty() {
        vec![(0.0, 0.0)]
    } else if points.len() == 1 {
        points.push((1.0, points[0].1));
        points
    } else {
        points
    }
}

/// Return y-axis bounds that preserve the sign of trend values and include zero.
fn signed_y_bounds(points: &[(f64, f64)]) -> [f64; 2] {
    let min_value = points
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::min);
    let max_value = points
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max);
    if (min_value - max_value).abs() < f64::EPSILON {
        [min_value - 1.0, max_value + 1.0]
    } else {
        [min_value, max_value]
    }
}

/// Return a trend color that signals all-loss or mixed-sign series.
fn signed_trend_color(points: &[(f64, f64)]) -> Color {
    let has_positive = points.iter().any(|(_, value)| *value > 0.0);
    let has_negative = points.iter().any(|(_, value)| *value < 0.0);
    match (has_positive, has_negative) {
        (true, true) => Color::Yellow,
        (false, true) => Color::Red,
        _ => Color::Green,
    }
}

/// Draw a compact table-style file-handling optimization overview.
fn render_file_handling_optimization_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    overview: &TokenOverview,
) {
    let block = Block::bordered()
        .title("File Handling Optimization Overview")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compact = inner.width < 90;
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            file_handling_saved_tokens_line(overview, compact),
            file_handling_reads_line(overview, compact),
        ])
        .wrap(Wrap { trim: true }),
        sections[0],
    );
    render_savings_source_table(frame, sections[1], overview);
    let token_mix = file_handling_token_mix(overview);
    let observed_ratio = ratio(token_mix.observed_abs, token_mix.total_abs());
    let gauge_color = if token_mix.net() < 0 {
        Color::Red
    } else {
        Color::Green
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(gauge_color)
                    .add_modifier(Modifier::BOLD),
            )
            .ratio(observed_ratio)
            .label(token_mix_label(token_mix)),
        sections[2],
    );
}

/// Draw the visible source rows inside the file-handling overview.
fn render_savings_source_table(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let compact = area.width < 90;
    let (headers, constraints, column_spacing) = if compact {
        (
            ["Impact source", "Reads", "Tokens", "Meaning"],
            [
                Constraint::Length(15),
                Constraint::Length(7),
                Constraint::Length(14),
                Constraint::Min(14),
            ],
            1,
        )
    } else {
        (
            [
                "Impact source",
                "File reads",
                "Tokens saved",
                "Plain meaning",
            ],
            [
                Constraint::Length(26),
                Constraint::Length(14),
                Constraint::Length(18),
                Constraint::Min(24),
            ],
            2,
        )
    };
    let mut rows = savings_source_rows_for_width(overview, compact)
        .into_iter()
        .map(|source| {
            Row::new(vec![
                Cell::from(source.label),
                Cell::from(grouped_count(source.steps)),
                Cell::from(signed_count(source.tokens)),
                Cell::from(source.meaning),
            ])
            .style(Style::default().fg(source.color))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from("none"),
            Cell::from("0"),
            Cell::from("0"),
            Cell::from("no telemetry rows"),
        ]));
    }
    let table = Table::new(rows, constraints)
        .header(
            Row::new(headers).bottom_margin(1).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .column_spacing(column_spacing)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(table, area);
}

/// Return the conservative saved-token equation for file handling.
fn file_handling_saved_tokens_line(overview: &TokenOverview, compact: bool) -> Line<'static> {
    let modeled_suffix = if compact {
        " modeled"
    } else {
        " modeled navigation"
    };
    Line::from(vec![
        label(if compact { "Tokens" } else { "Tokens avoided" }),
        signed_value(overview.tokens_avoided),
        Span::raw(" = "),
        signed_value(overview.measured_tokens_saved),
        Span::raw(" observed + "),
        signed_value(overview.deduped_modeled_tokens_avoided),
        Span::raw(modeled_suffix),
    ])
}

/// Return the headline file-read-avoidance equation.
fn file_handling_reads_line(overview: &TokenOverview, compact: bool) -> Line<'static> {
    Line::from(vec![
        label(if compact {
            "Reads"
        } else {
            "File reads avoided"
        }),
        value(overview.likely_file_reads_avoided),
        Span::raw(" = observed "),
        value(overview.observed_file_read_replacements),
        Span::raw(if compact {
            " + modeled "
        } else {
            " + search-modeled "
        }),
        value(overview.modeled_file_reads_avoided),
        Span::raw(if compact {
            String::new()
        } else {
            format!(" ({})", overview.read_avoidance_confidence)
        }),
    ])
}

/// Signed and absolute token operands shown in the file-handling contribution gauge.
#[derive(Clone, Copy)]
struct TokenMix {
    /// Signed observed summary/slice savings.
    observed: isize,
    /// Signed deduped modeled navigation savings.
    modeled: isize,
    /// Absolute observed contribution magnitude for a ratio widget.
    observed_abs: usize,
    /// Absolute modeled contribution magnitude for a ratio widget.
    modeled_abs: usize,
}

impl TokenMix {
    /// Return the signed net total represented by the visible operands.
    fn net(self) -> isize {
        self.observed.saturating_add(self.modeled)
    }

    /// Return the absolute denominator used by the contribution gauge.
    fn total_abs(self) -> usize {
        self.observed_abs.saturating_add(self.modeled_abs)
    }
}

/// Return the token operands that back the file-handling contribution gauge.
fn file_handling_token_mix(overview: &TokenOverview) -> TokenMix {
    TokenMix {
        observed: overview.measured_tokens_saved,
        modeled: overview.deduped_modeled_tokens_avoided,
        observed_abs: overview.measured_tokens_saved.unsigned_abs(),
        modeled_abs: overview.deduped_modeled_tokens_avoided.unsigned_abs(),
    }
}

/// Return the file-handling gauge label without hiding signed losses.
fn token_mix_label(mix: TokenMix) -> String {
    if mix.observed < 0 || mix.modeled < 0 {
        format!(
            "signed saved-token mix: observed {} / modeled {}; net {}",
            signed_count(mix.observed),
            signed_count(mix.modeled),
            signed_count(mix.net())
        )
    } else {
        format!(
            "saved-token mix: observed {} / modeled {}",
            percentage_label(mix.observed_abs, mix.total_abs()),
            percentage_label(mix.modeled_abs, mix.total_abs())
        )
    }
}

/// Draw accounting notes and optional calibration metadata.
fn render_overview_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut lines = vec![
        Line::from(vec![
            label("Accounting"),
            Span::raw("observed replacements + deduped modeled navigation"),
        ]),
        Line::from(vec![
            label("Deduped"),
            value(overview.repeated_baselines_deduped),
            Span::raw(" repeated navigation baselines collapsed"),
        ]),
        Line::from(vec![
            label("Read estimate"),
            Span::raw(format!(
                "{} file-read estimate",
                overview.read_avoidance_confidence
            )),
        ]),
    ];
    if let Some(calibration) = &overview.calibration {
        lines.push(Line::from(vec![
            label("Tokenizer check"),
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
            label("Tokenizer check"),
            Span::raw("optional audit: --tokenizer o200k_base or cl100k_base"),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .title("What this means")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// One visible aggregate row for savings-source telemetry.
struct SavingsSourceRow {
    /// Human label shown in the source table.
    label: &'static str,
    /// Number of telemetry steps represented by the row.
    steps: usize,
    /// Estimated saved tokens represented by the row.
    tokens: isize,
    /// Plain-language explanation for humans.
    meaning: &'static str,
    /// Row color used by Ratatui.
    color: Color,
}

/// Aggregate visible accounting with labels that fit the current table width.
fn savings_source_rows_for_width(overview: &TokenOverview, compact: bool) -> Vec<SavingsSourceRow> {
    let mut rows = Vec::new();
    if overview.observed_file_read_replacements > 0 || overview.measured_tokens_saved != 0 {
        let (label, meaning) = if compact {
            ("Observed reads", "files replaced")
        } else {
            ("Observed reads", "full-file opens replaced")
        };
        rows.push(SavingsSourceRow {
            label,
            steps: overview.observed_file_read_replacements,
            tokens: overview.measured_tokens_saved,
            meaning,
            color: Color::Green,
        });
    }
    if overview.modeled_file_reads_avoided > 0 || overview.deduped_modeled_tokens_avoided != 0 {
        let (label, meaning) = if compact {
            ("Modeled search", "candidates skipped")
        } else {
            ("Modeled search", "search candidates skipped")
        };
        rows.push(SavingsSourceRow {
            label,
            steps: overview.modeled_file_reads_avoided,
            tokens: overview.deduped_modeled_tokens_avoided,
            meaning,
            color: Color::Magenta,
        });
    }
    rows
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

    let trend_points = signed_trend_points(Some(&report.periods));
    let [lower, upper] = signed_y_bounds(&trend_points);
    frame.render_widget(
        Chart::new(vec![
            Dataset::default()
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::default().fg(signed_trend_color(&trend_points)))
                .data(&trend_points),
        ])
        .block(Block::bordered().title("Saved Tokens Trend"))
        .x_axis(Axis::default().bounds([0.0, (trend_points.len().saturating_sub(1)) as f64]))
        .y_axis(Axis::default().bounds([lower, upper])),
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

#[cfg(test)]
mod tests {
    use super::{
        DASHBOARD_HEIGHT, dashboard_width, render_dashboard_to_string, render_overview_frame,
        render_token_dashboard, render_token_trend_dashboard, savings_source_rows_for_width,
        signed_count, signed_trend_points, signed_y_bounds,
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
        assert!(dashboard.contains("Avoided reads"));
        assert!(dashboard.contains("File reads avoided"));
        assert!(dashboard.contains("File Handling Optimization Overview"));
        assert!(dashboard.contains("Impact source"));
        assert!(dashboard.contains("File reads"));
        assert!(dashboard.contains("Tokens avoided"));
        assert!(dashboard.contains("Observed reads"));
        assert!(dashboard.contains("Modeled search"));
        assert!(!dashboard.contains("Total shown"));
        assert!(dashboard.contains("Saved-token trends"));
        assert!(dashboard.contains("day trend"));
        assert!(dashboard.contains("week trend"));
        assert!(dashboard.contains("month trend"));
        assert!(dashboard.contains("year trend"));
        assert!(dashboard.contains("Accounting"));
        assert!(dashboard.contains("repeated navigation baselines collapsed"));
        assert!(dashboard.contains("Tokenizer check"));
        assert!(dashboard_contains_chart_glyph(&dashboard));
        assert!(!dashboard.contains("How ProjectAtlas helped"));
        assert!(!dashboard.contains("Observed summaries/slices"));
        assert!(!dashboard.contains("Search-modeled narrowing"));
        assert!(!dashboard.contains("Gross tokens: without vs with ProjectAtlas"));
        assert!(!dashboard.contains("Gross comparison"));
        assert!(!dashboard.contains("Without ProjectAtlas"));
        assert!(!dashboard.contains("With ProjectAtlas"));
        assert!(!dashboard.contains("Saved by ProjectAtlas"));
        assert!(!dashboard.contains("Where the savings came from"));
        assert!(!dashboard.contains("Measured summaries"));
        assert!(!dashboard.contains("Narrowed files"));
        assert!(!dashboard.contains("Fewer candidates"));

        let file_reads = dashboard
            .find("File Handling Optimization Overview")
            .unwrap_or(usize::MAX);
        let trend = dashboard.find("Saved-token trends").unwrap_or(usize::MAX);
        let notes = dashboard.find("What this means").unwrap_or(usize::MAX);
        assert!(file_reads < trend);
        assert!(trend < notes);

        assert_header_margin(&dashboard, "Impact source", "Observed reads");
        assert_occurs_once(&dashboard, "Conservative tokens avoided");
        assert_occurs_once(&dashboard, "File reads avoided");
        assert_occurs_once(&dashboard, "File Handling Optimization Overview");
        assert_occurs_once(&dashboard, "Saved-token trends");
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

        assert_cell_style(&buffer, "Impact source", Color::Cyan, Modifier::BOLD);
        assert_cell_style(&buffer, "Tokens saved", Color::Cyan, Modifier::BOLD);
        assert_cell_style(&buffer, "Plain meaning", Color::Cyan, Modifier::BOLD);
        assert_cell_style(&buffer, "Observed reads", Color::Green, Modifier::empty());
        assert_cell_style(&buffer, "Modeled search", Color::Magenta, Modifier::empty());
    }

    #[test]
    fn overview_dashboard_uses_compact_source_table_at_narrow_width() {
        let events = [
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcdabcd",
                "ab",
            ),
            usage_from_estimates(
                "s",
                "search",
                None,
                Some("token".to_string()),
                1_234_567_932,
                42,
            ),
        ];
        let overview = TokenOverview::from_events(&events);
        let trends = sample_trends();
        let dashboard = render_dashboard_to_string(80, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame(frame, &overview, Some("s"), &trends);
        });

        assert!(dashboard.contains("Impact source"));
        assert!(dashboard.contains("Reads"));
        assert!(dashboard.contains("Tokens"));
        assert!(dashboard.contains("Observed reads"));
        assert!(dashboard.contains("Modeled search"));
        assert!(dashboard.contains("1,234,567,890"));
        assert!(!dashboard.contains("Total shown"));
        assert!(!dashboard.contains("headline conservative total"));
        assert!(!dashboard.contains("Observed summaries/slic"));
        assert!(!dashboard.contains("Search-modeled narrowin"));
        assert!(!dashboard.contains("How ProjectAtlas helped"));
        assert!(!dashboard.contains("How PA helped"));
        assert!(!dashboard.contains(" obs + "));
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

        let trends = sample_trends();
        let dashboard = render_token_dashboard(&overview, Some("s"), &trends);
        let source_rows = savings_source_rows_for_width(&overview, false);
        let source_steps = source_rows.iter().map(|row| row.steps).sum::<usize>();
        let source_tokens = source_rows.iter().map(|row| row.tokens).sum::<isize>();

        assert_eq!(source_steps, overview.likely_file_reads_avoided);
        assert_eq!(source_tokens, conservative_avoided);
        assert!(dashboard.contains(&signed_count(conservative_avoided)));
        assert!(!dashboard.contains(&signed_count(gross_saved)));
        assert!(dashboard.contains(&format!(
            "{} = {} observed + {} modeled navigation",
            signed_count(conservative_avoided),
            signed_count(overview.measured_tokens_saved),
            signed_count(overview.deduped_modeled_tokens_avoided)
        )));
        assert!(dashboard.contains("4 = observed 1 + search-modeled 3"));
        assert!(dashboard.contains("Observed reads"));
        assert!(dashboard.contains("Modeled search"));
        assert!(!dashboard.contains("Total shown"));
        assert!(!dashboard.contains("Observed summaries/slices"));
        assert!(!dashboard.contains("Search-modeled narrowing"));
        assert!(!dashboard.contains("latest"));
        assert!(!dashboard.contains("Gross comparison"));
        assert!(!dashboard.contains("Where the savings came from"));
        assert!(!dashboard.contains("Fewer candidates"));
    }

    #[test]
    fn overview_trend_titles_do_not_expose_gross_saved_values() {
        let full_file = "x".repeat(400);
        let atlas_summary = "x".repeat(320);
        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                &full_file,
                &atlas_summary,
            ),
            usage_from_estimates(
                "s",
                "search",
                None,
                Some("token".to_string()),
                326_515_240,
                80,
            ),
            usage_from_estimates(
                "s",
                "search",
                None,
                Some("token".to_string()),
                172_316_346,
                0,
            ),
        ]);
        let gross_saved_text = signed_count(overview.legacy_gross_estimated_saved);
        let conservative_text = signed_count(overview.tokens_avoided);
        assert_ne!(gross_saved_text, conservative_text);

        let trends = vec![TokenTrendReport::new(
            Some("s".to_string()),
            TokenTrendWindow::Year,
            vec![TokenTrendPeriod::from_totals(
                "2026".to_string(),
                3,
                overview.estimated_without_projectatlas as u128,
                overview.estimated_with_projectatlas as u128,
            )],
        )];
        let dashboard = render_token_dashboard(&overview, Some("s"), &trends);

        assert!(dashboard.contains(&conservative_text));
        assert!(!dashboard.contains(&gross_saved_text));
        assert!(dashboard.contains("year trend | 1 period"));
        assert!(!dashboard.contains("latest"));
    }

    #[test]
    fn overview_dashboard_token_mix_percentages_follow_saved_token_operands() {
        let full_file = "x".repeat(400);
        let atlas_summary = "x".repeat(320);
        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                &full_file,
                &atlas_summary,
            ),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 100, 20),
        ]);

        assert_eq!(overview.measured_tokens_saved, 20);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 80);
        assert_eq!(overview.tokens_avoided, 100);
        assert_eq!(overview.observed_file_read_replacements, 1);
        assert_eq!(overview.modeled_file_reads_avoided, 1);

        let dashboard = render_token_dashboard(&overview, Some("s"), &sample_trends());
        assert!(dashboard.contains("Conservative tokens avoided"));
        assert!(dashboard.contains("File reads avoided"));
        assert!(dashboard.contains("100 = 20 observed + 80 modeled navigation"));
        assert!(dashboard.contains("2 = observed 1 + search-modeled 1"));
        assert!(!dashboard.contains("Total shown"));
        assert!(dashboard.contains("saved-token mix: observed 20% / modeled 80%"));
    }

    #[test]
    fn overview_dashboard_preserves_negative_savings_in_visual_widgets() {
        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcd",
                "abcdabcdabcd",
            ),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 10, 30),
        ]);
        assert!(overview.measured_tokens_saved < 0);
        assert!(overview.deduped_modeled_tokens_avoided < 0);

        let dashboard = render_token_dashboard(&overview, Some("s"), &sample_trends());
        assert!(dashboard.contains(&format!(
            "signed saved-token mix: observed {} / modeled {}; net {}",
            signed_count(overview.measured_tokens_saved),
            signed_count(overview.deduped_modeled_tokens_avoided),
            signed_count(overview.tokens_avoided)
        )));
        assert!(!dashboard.contains("% / modeled"));

        let trend = vec![
            TokenTrendPeriod::from_totals("loss".to_string(), 1, 10, 30),
            TokenTrendPeriod::from_totals("gain".to_string(), 1, 30, 10),
        ];
        let points = signed_trend_points(Some(&trend));
        assert_float_eq(points[0].1, -20.0);
        assert_float_eq(points[1].1, 20.0);
        let bounds = signed_y_bounds(&points);
        assert_float_eq(bounds[0], -20.0);
        assert_float_eq(bounds[1], 20.0);

        let single = [TokenTrendPeriod::from_totals("one".to_string(), 1, 30, 10)];
        let single_points = signed_trend_points(Some(&single));
        assert_float_eq(single_points[0].0, 0.0);
        assert_float_eq(single_points[0].1, 20.0);
        assert_float_eq(single_points[1].0, 1.0);
        assert_float_eq(single_points[1].1, 20.0);
    }

    #[test]
    fn trend_dashboard_renders_chart_and_period_table() {
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
        assert!(dashboard_contains_chart_glyph(&dashboard));
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

    fn assert_occurs_once(dashboard: &str, needle: &str) {
        assert_eq!(
            dashboard.matches(needle).count(),
            1,
            "dashboard should show {needle:?} once"
        );
    }

    fn dashboard_contains_chart_glyph(dashboard: &str) -> bool {
        dashboard.chars().any(|character| {
            matches!(
                character,
                '█' | '▌' | '▏' | '▅' | '▁' | '\u{2801}'..='\u{28ff}'
            )
        })
    }

    fn assert_float_eq(left: f64, right: f64) {
        assert!(
            (left - right).abs() < f64::EPSILON,
            "expected {left} to equal {right}"
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
