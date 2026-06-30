//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::telemetry::{TokenOverview, TokenTrendReport};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Gauge, Paragraph, Row, Sparkline, Table, Wrap};
use ratatui::{Frame, Terminal};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 35;
/// Fixed terminal height for the token trend dashboard snapshot.
const TREND_DASHBOARD_HEIGHT: u16 = 30;

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
                " ProjectAtlas Token Dashboard ",
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
            Constraint::Length(5),
            Constraint::Length(9),
            Constraint::Min(10),
            Constraint::Length(5),
        ])
        .split(inner);

    render_overview_summary(frame, sections[0], overview);
    render_overview_gauges(frame, sections[1], overview);
    render_overview_bars(frame, sections[2], overview);
    render_bucket_table(frame, sections[3], overview);
    render_overview_notes(frame, sections[4], overview);
}

/// Draw the top overview metadata block.
fn render_overview_summary(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let text = vec![
        Line::from(vec![
            label("calls"),
            value(overview.calls),
            Span::raw("   "),
            label("baseline"),
            value(overview.estimated_without_projectatlas),
            Span::raw("   "),
            label("emitted"),
            value(overview.estimated_with_projectatlas),
            Span::raw("   "),
            label("legacy gross"),
            signed_value(overview.legacy_gross_estimated_saved),
        ]),
        Line::from(vec![
            label("estimate"),
            Span::raw(format!(
                "{} / {}",
                overview.estimator, overview.estimate_scope
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
        "tokens avoided",
        overview.tokens_avoided,
        max_positive,
        Color::Green,
    );
    render_gauge(
        frame,
        chunks[1],
        "measured saved",
        overview.measured_tokens_saved,
        max_positive,
        Color::Yellow,
    );
    render_gauge(
        frame,
        chunks[2],
        "deduped modeled",
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

/// Draw horizontal comparison bars for baseline, emitted, gross, and avoided tokens.
fn render_overview_bars(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let max_value = overview
        .estimated_without_projectatlas
        .max(overview.estimated_with_projectatlas)
        .max(overview.legacy_gross_estimated_saved.max(0).unsigned_abs())
        .max(overview.tokens_avoided.max(0).unsigned_abs())
        .max(1);
    let block = Block::bordered().title("Comparison");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let bar_width = usize::from(inner.width).saturating_sub(29).max(12);
    let rows = vec![
        comparison_bar_line(
            "baseline",
            &grouped_count(overview.estimated_without_projectatlas),
            overview.estimated_without_projectatlas,
            max_value,
            bar_width,
            Color::Blue,
        ),
        comparison_bar_line(
            "emitted",
            &grouped_count(overview.estimated_with_projectatlas),
            overview.estimated_with_projectatlas,
            max_value,
            bar_width,
            Color::Cyan,
        ),
        comparison_bar_line(
            "gross",
            &signed_count(overview.legacy_gross_estimated_saved),
            overview.legacy_gross_estimated_saved.max(0).unsigned_abs(),
            max_value,
            bar_width,
            Color::Yellow,
        ),
        comparison_bar_line(
            "avoided",
            &signed_count(overview.tokens_avoided),
            overview.tokens_avoided.max(0).unsigned_abs(),
            max_value,
            bar_width,
            Color::Green,
        ),
    ];
    frame.render_widget(Paragraph::new(rows), inner);
}

/// Build one fixed-column comparison bar row.
fn comparison_bar_line(
    title: &'static str,
    value_text: &str,
    value: usize,
    max_value: usize,
    width: usize,
    color: Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{title:<9}"),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{value_text:>16}  "),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            comparison_bar_cells(value, max_value, width),
            Style::default().fg(color),
        ),
    ])
}

/// Render a bounded horizontal bar using filled and empty cells.
fn comparison_bar_cells(value: usize, max_value: usize, width: usize) -> String {
    let width = width.max(1);
    let filled = if value == 0 || max_value == 0 {
        0
    } else {
        (((value as f64 / max_value as f64) * width as f64).round() as usize)
            .max(1)
            .min(width)
    };
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

/// Draw the bucket table.
fn render_bucket_table(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut rows = overview
        .buckets
        .iter()
        .take(7)
        .map(|bucket| {
            Row::new(vec![
                Cell::from(bucket.accounting_layer.clone()),
                Cell::from(bucket.token_savings_bucket.clone()),
                Cell::from(bucket.denominator_kind.clone()),
                Cell::from(signed_count(bucket.estimated_saved)),
                Cell::from(grouped_count(bucket.calls)),
            ])
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        rows.push(Row::new(vec![
            Cell::from("none"),
            Cell::from("no telemetry rows"),
            Cell::from(""),
            Cell::from("0"),
            Cell::from("0"),
        ]));
    }
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(22),
            Constraint::Percentage(28),
            Constraint::Percentage(22),
            Constraint::Percentage(16),
            Constraint::Percentage(12),
        ],
    )
    .header(
        Row::new(vec!["layer", "bucket", "denominator", "saved", "calls"]).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    )
    .block(Block::bordered().title("Buckets"))
    .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(table, area);
}

/// Draw accounting notes and optional calibration metadata.
fn render_overview_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let mut lines = vec![
        Line::from(vec![
            label("headline"),
            signed_value(overview.tokens_avoided),
            Span::raw(" = measured "),
            signed_value(overview.measured_tokens_saved),
            Span::raw(" + deduped modeled "),
            signed_value(overview.deduped_modeled_tokens_avoided),
        ]),
        Line::from(vec![
            label("dedupe"),
            value(overview.repeated_baselines_deduped),
            Span::raw(" duplicate modeled events collapsed; gross modeled "),
            signed_value(overview.gross_modeled_tokens_avoided),
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
                "add --tokenizer o200k_base or --tokenizer cl100k_base for local tokenizer audit",
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::bordered().title("Accounting"))
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
    fn overview_dashboard_renders_chart_accounting_and_buckets() {
        let events = [
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                "abcdabcd",
                "ab",
            ),
            usage_from_estimates("s", "folders", None, Some("token".to_string()), 400, 40),
            usage_from_estimates("s", "folders", None, Some("token".to_string()), 400, 30),
        ];
        let dashboard = render_token_dashboard(&TokenOverview::from_events(&events), Some("s"));

        assert!(dashboard.contains("ProjectAtlas Token Dashboard"));
        assert!(dashboard.contains("tokens avoided"));
        assert!(dashboard.contains("deduped modeled"));
        assert!(dashboard.contains("Comparison"));
        assert!(dashboard.contains("Buckets"));
        assert!(dashboard.contains("modeled_avoidance"));
        assert!(dashboard.contains("observed_delta"));
        assert!(dashboard.contains("duplicate modeled events collapsed"));
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
