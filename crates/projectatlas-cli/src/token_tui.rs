//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::telemetry::{
    TOKEN_ACCOUNTING_OBSERVED_DELTA, TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_FULL_FILE,
    TOKEN_BASELINE_SELECTED_CANDIDATES, TOKEN_BUCKET_FULL_FILE_COMPRESSION, TokenBucketOverview,
    TokenOverview, TokenTrendPeriod, TokenTrendReport,
};
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};
use std::time::{SystemTime, UNIX_EPOCH};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 42;
/// Fixed terminal height for the token trend dashboard snapshot.
const TREND_DASHBOARD_HEIGHT: u16 = 30;
/// Token dashboard near-black navy background.
const THEME_BG: Color = Color::Rgb(4, 10, 18);
/// Token dashboard panel background.
const THEME_PANEL: Color = Color::Rgb(7, 20, 33);
/// Token dashboard primary text.
const THEME_TEXT: Color = Color::Rgb(218, 214, 204);
/// Token dashboard muted label text.
const THEME_MUTED: Color = Color::Rgb(158, 151, 139);
/// Token dashboard mascot/title white.
const THEME_INK_WHITE: Color = Color::Rgb(238, 234, 224);
/// Token dashboard `ProjectAtlas` blue.
const THEME_BLUE: Color = Color::Rgb(93, 143, 255);
/// Token dashboard saved-token green.
const THEME_GREEN: Color = Color::Rgb(111, 216, 100);
/// Token dashboard modeled-confidence yellow.
const THEME_YELLOW: Color = Color::Rgb(230, 179, 55);
/// Token dashboard border blue.
const THEME_BORDER: Color = Color::Rgb(35, 62, 90);
/// Token dashboard loss red.
const THEME_RED: Color = Color::Rgb(235, 95, 95);

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
    frame.render_widget(Block::default().style(Style::default().bg(THEME_BG)), area);

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(10),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(5),
            Constraint::Length(1),
        ])
        .split(area);

    render_token_header(frame, sections[0], overview, session);
    render_token_hero(frame, sections[1], overview);
    render_file_reads_card(frame, sections[2], overview);
    render_composition_and_signal(frame, sections[3], overview);
    render_savings_breakdown_table(frame, sections[4], overview);
    render_calibration_notes(frame, sections[5], overview);
    render_status_bar(frame, sections[6]);
}

/// Return a screenshot-matched dashboard panel.
fn panel(title: &'static str) -> Block<'static> {
    let block = Block::bordered()
        .border_style(Style::default().fg(THEME_BORDER))
        .style(Style::default().fg(THEME_TEXT).bg(THEME_PANEL));
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(THEME_BLUE)
                .bg(THEME_PANEL)
                .add_modifier(Modifier::BOLD),
        ))
    }
}

/// Draw the title band, including the small terminal-native Ani mascot mark.
fn render_token_header(
    frame: &mut Frame<'_>,
    area: Rect,
    overview: &TokenOverview,
    session: Option<&str>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18),
            Constraint::Min(32),
            Constraint::Length(if area.width >= 110 { 46 } else { 34 }),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Ani  _/\\_x", identity_style())),
            Line::from(Span::styled("  __/____\\__", identity_style())),
            Line::from(Span::styled("   ( -_-) ", identity_style())),
            Line::from(Span::styled("  /|src docs|", identity_style())),
        ]),
        columns[0],
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("ProjectAtlas", identity_title_style()),
                Span::raw(" "),
                Span::styled("Token Impact", token_title_style()),
            ]),
            Line::from(vec![
                Span::styled("Smarter context. Fewer tokens. ", body_style()),
                Span::styled(
                    "Real savings.",
                    Style::default().fg(THEME_GREEN).bg(THEME_BG),
                ),
            ]),
        ])
        .wrap(Wrap { trim: true }),
        columns[1],
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Session: ", muted_bold_style()),
                Span::styled(session.unwrap_or("all"), body_style()),
            ]),
            Line::from(vec![
                Span::styled("Lookups: ", muted_bold_style()),
                Span::styled(grouped_count(overview.calls), body_style()),
            ]),
            Line::from(vec![
                Span::styled("Estimate: ", muted_bold_style()),
                Span::styled("local", body_style()),
            ]),
        ])
        .alignment(Alignment::Right)
        .wrap(Wrap { trim: true }),
        columns[2],
    );
}

/// Draw the dominant saved-token hero panel.
fn render_token_hero(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let block = panel("").border_style(Style::default().fg(THEME_BLUE));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(3),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new("TOTAL TOKENS AVOIDED")
            .style(header_style())
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(signed_count(overview.tokens_avoided))
            .style(hero_value_style(overview.tokens_avoided))
            .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new("tokens avoided")
            .style(body_style())
            .alignment(Alignment::Center),
        rows[2],
    );
    render_divider(frame, rows[3]);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(3),
            Constraint::Percentage(30),
            Constraint::Length(3),
            Constraint::Percentage(30),
        ])
        .split(rows[4]);

    let with_projectatlas = usize_to_isize_saturating(overview.estimated_with_projectatlas);
    let without_projectatlas = reconciled_without_projectatlas(overview);
    let saved_by_projectatlas = overview.tokens_avoided;
    let denominator = without_projectatlas.unsigned_abs();

    render_metric_column(
        frame,
        columns[0],
        signed_count(without_projectatlas),
        "Without ProjectAtlas",
        THEME_BLUE,
        1.0,
    );
    frame.render_widget(center_symbol("-"), columns[1]);
    render_metric_column(
        frame,
        columns[2],
        signed_count(with_projectatlas),
        "With ProjectAtlas",
        THEME_INK_WHITE,
        ratio(with_projectatlas.unsigned_abs(), denominator),
    );
    frame.render_widget(center_symbol("="), columns[3]);
    render_metric_column(
        frame,
        columns[4],
        signed_count(saved_by_projectatlas),
        "Saved by ProjectAtlas",
        signed_color(saved_by_projectatlas),
        ratio(saved_by_projectatlas.unsigned_abs(), denominator),
    );
}

/// Draw one metric operand in the hero equation.
fn render_metric_column(
    frame: &mut Frame<'_>,
    area: Rect,
    number: String,
    label_text: &'static str,
    color: Color,
    ratio_value: f64,
) {
    let bar_width = area.width.saturating_sub(2).min(34) as usize;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                number,
                Style::default()
                    .fg(color)
                    .bg(THEME_PANEL)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                label_text,
                Style::default().fg(color).bg(THEME_PANEL),
            )),
            block_bar(bar_width, ratio_value, color),
        ])
        .alignment(Alignment::Center),
        area,
    );
}

/// Return a centered operator paragraph.
fn center_symbol(symbol: &'static str) -> Paragraph<'static> {
    Paragraph::new(symbol).alignment(Alignment::Center).style(
        Style::default()
            .fg(THEME_TEXT)
            .bg(THEME_PANEL)
            .add_modifier(Modifier::BOLD),
    )
}

/// Draw the reference-style file-read strip.
fn render_file_reads_card(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let block = panel("FILE READS AVOIDED");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compact = inner.width < 100;

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(24),
            Constraint::Length(1),
            Constraint::Percentage(29),
            Constraint::Length(1),
            Constraint::Percentage(29),
            Constraint::Length(1),
            Constraint::Percentage(16),
        ])
        .split(inner);
    let total_reads = overview.likely_file_reads_avoided;
    let observed_ratio = ratio(overview.observed_file_read_replacements, total_reads);
    let modeled_ratio = ratio(overview.modeled_file_reads_avoided, total_reads);
    let observed_title = if compact {
        "Observed"
    } else {
        "Observed (summaries/slices)"
    };
    let modeled_title = if compact {
        "Modeled narrowing"
    } else {
        "Search-modeled narrowing"
    };

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                grouped_count(total_reads),
                Style::default()
                    .fg(THEME_INK_WHITE)
                    .bg(THEME_PANEL)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "file reads avoided",
                muted_style().bg(THEME_PANEL),
            )),
        ]),
        columns[0],
    );
    render_vertical_separator(frame, columns[1]);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                observed_title,
                Style::default().fg(THEME_INK_WHITE).bg(THEME_PANEL),
            )),
            Line::from(vec![
                Span::styled(
                    grouped_count(overview.observed_file_read_replacements),
                    Style::default()
                        .fg(THEME_INK_WHITE)
                        .bg(THEME_PANEL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    percentage_label(overview.observed_file_read_replacements, total_reads),
                    muted_style().bg(THEME_PANEL),
                ),
            ]),
            block_bar(
                columns[2].width.saturating_sub(2).min(24) as usize,
                observed_ratio,
                THEME_INK_WHITE,
            ),
        ]),
        columns[2],
    );
    render_vertical_separator(frame, columns[3]);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                modeled_title,
                Style::default().fg(THEME_YELLOW).bg(THEME_PANEL),
            )),
            Line::from(vec![
                Span::styled(
                    grouped_count(overview.modeled_file_reads_avoided),
                    Style::default()
                        .fg(THEME_YELLOW)
                        .bg(THEME_PANEL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    percentage_label(overview.modeled_file_reads_avoided, total_reads),
                    muted_style().bg(THEME_PANEL),
                ),
            ]),
            block_bar(
                columns[4].width.saturating_sub(2).min(24) as usize,
                modeled_ratio,
                THEME_YELLOW,
            ),
        ]),
        columns[4],
    );
    render_vertical_separator(frame, columns[5]);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("Confidence", muted_style().bg(THEME_PANEL))),
            Line::from(Span::styled(
                overview.read_avoidance_confidence.clone(),
                Style::default()
                    .fg(THEME_YELLOW)
                    .bg(THEME_PANEL)
                    .add_modifier(Modifier::BOLD),
            )),
        ])
        .alignment(Alignment::Center),
        columns[6],
    );
}

/// Draw the side-by-side composition and signal cards.
fn render_composition_and_signal(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(area);
    render_savings_composition(frame, columns[0], overview);
    render_signal_card(frame, columns[2], overview);
}

/// Draw observed-vs-modeled token composition.
fn render_savings_composition(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let block = panel("SAVINGS COMPOSITION");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mix = file_handling_token_mix(overview);
    let total = mix.total_abs();
    let compact = inner.width < 56;
    let label_width = if compact { 18 } else { 32 };
    let bar_width = inner.width.saturating_sub(label_width + 10).clamp(6, 24) as usize;

    let lines = if mix.observed < 0 || mix.modeled < 0 {
        vec![
            Line::from(Span::styled(
                format!(
                    "Signed mix: observed {} / modeled {}; net {}",
                    signed_count(mix.observed),
                    signed_count(mix.modeled),
                    signed_count(mix.net())
                ),
                body_style().bg(THEME_PANEL),
            )),
            composition_line(
                if compact {
                    "Measured"
                } else {
                    "Measured from summaries/slices"
                },
                ratio(mix.observed_abs, total),
                THEME_INK_WHITE,
                bar_width,
                label_width as usize,
            ),
            composition_line(
                if compact {
                    "Navigation"
                } else {
                    "Navigation narrowing"
                },
                ratio(mix.modeled_abs, total),
                THEME_YELLOW,
                bar_width,
                label_width as usize,
            ),
        ]
    } else {
        vec![
            composition_line(
                if compact {
                    "Measured"
                } else {
                    "Measured from summaries/slices"
                },
                ratio(mix.observed_abs, total),
                THEME_INK_WHITE,
                bar_width,
                label_width as usize,
            ),
            Line::from(Span::styled(
                "-".repeat(inner.width as usize),
                Style::default().fg(THEME_BORDER).bg(THEME_PANEL),
            )),
            composition_line(
                if compact {
                    "Navigation"
                } else {
                    "Navigation narrowing"
                },
                ratio(mix.modeled_abs, total),
                THEME_YELLOW,
                bar_width,
                label_width as usize,
            ),
        ]
    };

    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Return one composition row with a compact bar.
fn composition_line(
    label_text: &'static str,
    value: f64,
    color: Color,
    bar_width: usize,
    label_width: usize,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(
            format!("{label_text:<label_width$}"),
            Style::default().fg(color).bg(THEME_PANEL),
        ),
        Span::raw(" "),
    ];
    spans.extend(block_bar(bar_width, value, color).spans);
    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        percentage_one_decimal(value),
        Style::default()
            .fg(color)
            .bg(THEME_PANEL)
            .add_modifier(Modifier::BOLD),
    ));
    Line::from(spans)
}

/// Draw signal metadata from the reference dashboard.
fn render_signal_card(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let tokenizer = overview.calibration.as_ref().map_or_else(
        || "optional".to_string(),
        |calibration| calibration.tokenizer.clone(),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▣  ", Style::default().fg(THEME_BLUE).bg(THEME_PANEL)),
                Span::styled(
                    "Repeated baselines collapsed: ",
                    body_style().bg(THEME_PANEL),
                ),
                Span::styled(
                    grouped_count(overview.repeated_baselines_deduped),
                    body_style().bg(THEME_PANEL),
                ),
            ]),
            Line::from(vec![
                Span::styled("⌁  ", Style::default().fg(THEME_BLUE).bg(THEME_PANEL)),
                Span::styled("Estimate type: ", body_style().bg(THEME_PANEL)),
                Span::styled("local model", body_style().bg(THEME_PANEL)),
            ]),
            Line::from(vec![
                Span::styled("◇  ", Style::default().fg(THEME_BLUE).bg(THEME_PANEL)),
                Span::styled("Tokenizer audit: ", body_style().bg(THEME_PANEL)),
                Span::styled(tokenizer, body_style().bg(THEME_PANEL)),
            ]),
        ])
        .block(panel("SIGNAL"))
        .wrap(Wrap { trim: true }),
        area,
    );
}

/// Draw the screenshot-style source table.
fn render_savings_breakdown_table(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let compact = area.width < 92;
    let constraints = if compact {
        [
            Constraint::Length(20),
            Constraint::Length(1),
            Constraint::Length(7),
            Constraint::Length(1),
            Constraint::Length(14),
            Constraint::Length(1),
            Constraint::Min(18),
        ]
    } else {
        [
            Constraint::Length(30),
            Constraint::Length(1),
            Constraint::Length(10),
            Constraint::Length(1),
            Constraint::Length(18),
            Constraint::Length(1),
            Constraint::Min(26),
        ]
    };
    let rows = savings_source_rows_for_width(overview, compact)
        .into_iter()
        .map(|source| {
            Row::new(vec![
                Cell::from(source.label),
                Cell::from("|"),
                Cell::from(grouped_count(source.steps)),
                Cell::from("|"),
                Cell::from(signed_count(source.tokens)),
                Cell::from("|"),
                Cell::from(source.meaning),
            ])
            .style(Style::default().fg(source.color).bg(THEME_PANEL))
        })
        .collect::<Vec<_>>();
    let table = Table::new(rows, constraints)
        .header(
            Row::new(vec![
                "Source",
                "|",
                "Steps",
                "|",
                "Tokens Avoided",
                "|",
                "What it means",
            ])
            .style(header_style().bg(THEME_PANEL))
            .bottom_margin(1),
        )
        .column_spacing(1)
        .block(panel("WHERE THE SAVINGS CAME FROM"));
    frame.render_widget(table, area);
}

/// Draw calibration notes without duplicating headline totals.
fn render_calibration_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let calibration = overview.calibration.as_ref().map_or_else(
        || "Calibration optional -> add --tokenizer o200k_base or cl100k_base".to_string(),
        |value| {
            format!(
                "Tokenizer audit: {} over {} files",
                value.tokenizer,
                grouped_count(value.files)
            )
        },
    );
    let block = panel("CALIBRATION & NOTES");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 100 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "• Local estimate only; not provider billing data",
                    body_style().bg(THEME_PANEL),
                )),
                Line::from(Span::styled(
                    format!(
                        "• Observed reads: {}   Modeled narrowing: {}",
                        grouped_count(overview.observed_file_read_replacements),
                        grouped_count(overview.modeled_file_reads_avoided)
                    ),
                    body_style().bg(THEME_PANEL),
                )),
                Line::from(Span::styled(
                    format!("• {calibration}"),
                    body_style().bg(THEME_PANEL),
                )),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "• Local estimate only; not provider billing data",
                body_style().bg(THEME_PANEL),
            )),
            Line::from(Span::styled(
                format!(
                    "• Observed reads: {}   Modeled narrowing: {}",
                    grouped_count(overview.observed_file_read_replacements),
                    grouped_count(overview.modeled_file_reads_avoided)
                ),
                body_style().bg(THEME_PANEL),
            )),
            Line::from(Span::styled(
                format!("• {calibration}"),
                body_style().bg(THEME_PANEL),
            )),
        ])
        .wrap(Wrap { trim: true }),
        inner,
    );
}

/// Draw the compact footer/status row from the reference dashboard.
fn render_status_bar(frame: &mut Frame<'_>, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "ProjectAtlas v",
                Style::default().fg(THEME_BLUE).bg(THEME_BG),
            ),
            Span::styled(
                env!("CARGO_PKG_VERSION"),
                Style::default().fg(THEME_BLUE).bg(THEME_BG),
            ),
        ])),
        columns[0],
    );
    let clock = current_clock_label();
    let controls = if area.width < 100 {
        let compact_clock = clock.get(..5).unwrap_or(&clock).to_string();
        Line::from(vec![
            Span::styled("q Quit  ? Help  r Refresh  ", body_style().bg(THEME_BG)),
            Span::styled("● Auto ", Style::default().fg(THEME_GREEN).bg(THEME_BG)),
            Span::styled(compact_clock, body_style().bg(THEME_BG)),
        ])
    } else {
        Line::from(vec![
            keycap("q"),
            Span::styled(" Quit   ", body_style().bg(THEME_BG)),
            keycap("?"),
            Span::styled(" Help   ", body_style().bg(THEME_BG)),
            keycap("r"),
            Span::styled(" Refresh   ", body_style().bg(THEME_BG)),
            Span::styled("● Auto ", Style::default().fg(THEME_GREEN).bg(THEME_BG)),
            Span::styled(clock, body_style().bg(THEME_BG)),
        ])
    };
    frame.render_widget(
        Paragraph::new(controls).alignment(Alignment::Right),
        columns[1],
    );
}

/// Return a styled keycap for the status bar.
fn keycap(text: &'static str) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::default().fg(THEME_TEXT).bg(Color::DarkGray),
    )
}

/// Render a horizontal divider in a panel.
fn render_divider(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize))
            .style(Style::default().fg(THEME_BORDER).bg(THEME_PANEL)),
        area,
    );
}

/// Render a vertical separator in a panel.
fn render_vertical_separator(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "│",
                Style::default().fg(THEME_BORDER).bg(THEME_PANEL),
            )),
            Line::from(Span::styled(
                "│",
                Style::default().fg(THEME_BORDER).bg(THEME_PANEL),
            )),
            Line::from(Span::styled(
                "│",
                Style::default().fg(THEME_BORDER).bg(THEME_PANEL),
            )),
        ]),
        area,
    );
}

/// Return a segmented bar matching the reference dashboard.
fn block_bar(width: usize, ratio_value: f64, color: Color) -> Line<'static> {
    let filled = ((width as f64) * ratio_value.clamp(0.0, 1.0)).round() as usize;
    let empty = width.saturating_sub(filled);
    Line::from(vec![
        Span::styled(
            "━".repeat(filled),
            Style::default().fg(color).bg(THEME_PANEL),
        ),
        Span::styled(
            "·".repeat(empty),
            Style::default().fg(Color::DarkGray).bg(THEME_PANEL),
        ),
    ])
}

/// Signed and absolute token operands shown in the composition panel.
#[derive(Clone, Copy)]
struct TokenMix {
    /// Signed observed summary/slice savings.
    observed: isize,
    /// Signed deduped modeled navigation savings.
    modeled: isize,
    /// Absolute observed contribution magnitude.
    observed_abs: usize,
    /// Absolute modeled contribution magnitude.
    modeled_abs: usize,
}

impl TokenMix {
    /// Return the signed net total represented by the visible operands.
    fn net(self) -> isize {
        self.observed.saturating_add(self.modeled)
    }

    /// Return the absolute denominator used by composition bars.
    fn total_abs(self) -> usize {
        self.observed_abs.saturating_add(self.modeled_abs)
    }
}

/// Return the token operands that back the composition panel.
fn file_handling_token_mix(overview: &TokenOverview) -> TokenMix {
    TokenMix {
        observed: overview.measured_tokens_saved,
        modeled: overview.deduped_modeled_tokens_avoided,
        observed_abs: overview.measured_tokens_saved.unsigned_abs(),
        modeled_abs: overview.deduped_modeled_tokens_avoided.unsigned_abs(),
    }
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

/// Aggregate visible accounting with screenshot-aligned labels.
fn savings_source_rows_for_width(overview: &TokenOverview, compact: bool) -> Vec<SavingsSourceRow> {
    let mut rows = Vec::new();
    let observed_steps = observed_source_steps(overview);
    if observed_steps > 0 || overview.measured_tokens_saved != 0 {
        rows.push(SavingsSourceRow {
            label: if compact {
                "Summaries/slices"
            } else {
                "Summaries and slices"
            },
            steps: observed_steps,
            tokens: overview.measured_tokens_saved,
            meaning: if compact {
                "Files replaced"
            } else {
                "Compact output replaced file reads"
            },
            color: THEME_INK_WHITE,
        });
    }

    let modeled_groups = modeled_source_groups(overview);
    let modeled_weights = modeled_groups
        .iter()
        .map(|group| group.gross_tokens.unsigned_abs().max(group.steps))
        .collect::<Vec<_>>();
    let modeled_tokens =
        allocate_signed_total(overview.deduped_modeled_tokens_avoided, &modeled_weights);
    for (group, tokens) in modeled_groups.into_iter().zip(modeled_tokens) {
        if group.steps == 0 && tokens == 0 {
            continue;
        }
        rows.push(SavingsSourceRow {
            label: if compact {
                group.compact_label
            } else {
                group.label
            },
            steps: group.steps,
            tokens,
            meaning: if compact {
                group.compact_meaning
            } else {
                group.meaning
            },
            color: THEME_YELLOW,
        });
    }

    if rows.is_empty() {
        rows.push(SavingsSourceRow {
            label: "No telemetry",
            steps: 0,
            tokens: 0,
            meaning: "No token savings recorded",
            color: THEME_MUTED,
        });
    }
    rows
}

/// Real modeled source bucket aggregated for display.
struct ModeledSourceGroup {
    /// Full-width row label.
    label: &'static str,
    /// Narrow-width row label.
    compact_label: &'static str,
    /// Full-width row explanation.
    meaning: &'static str,
    /// Narrow-width row explanation.
    compact_meaning: &'static str,
    /// Number of telemetry calls in the group.
    steps: usize,
    /// Gross saved-token contribution before headline dedupe allocation.
    gross_tokens: isize,
}

impl ModeledSourceGroup {
    /// Build an empty display group.
    const fn new(
        label: &'static str,
        compact_label: &'static str,
        meaning: &'static str,
        compact_meaning: &'static str,
    ) -> Self {
        Self {
            label,
            compact_label,
            meaning,
            compact_meaning,
            steps: 0,
            gross_tokens: 0,
        }
    }

    /// Add one telemetry bucket to the group.
    fn add_bucket(&mut self, bucket: &TokenBucketOverview) {
        self.steps = self.steps.saturating_add(bucket.calls);
        self.gross_tokens = self.gross_tokens.saturating_add(bucket.estimated_saved);
    }
}

/// Return observed source steps from real observed buckets, with legacy fallback.
fn observed_source_steps(overview: &TokenOverview) -> usize {
    let bucket_steps = overview
        .buckets
        .iter()
        .filter(|bucket| is_observed_source_bucket(bucket))
        .map(|bucket| bucket.calls)
        .sum::<usize>();
    if bucket_steps == 0 {
        overview.observed_file_read_replacements
    } else {
        bucket_steps
    }
}

/// Return modeled source rows backed by actual telemetry buckets.
fn modeled_source_groups(overview: &TokenOverview) -> Vec<ModeledSourceGroup> {
    let mut groups = [
        ModeledSourceGroup::new(
            "Skipped broad folder walk",
            "Skipped folder walk",
            "Ranking skipped broad folders",
            "Folders skipped",
        ),
        ModeledSourceGroup::new(
            "Opened fewer candidates (A)",
            "Fewer candidates A",
            "Folder ranking narrowed files",
            "Folder shortlist",
        ),
        ModeledSourceGroup::new(
            "Opened fewer candidates (B)",
            "Fewer candidates B",
            "Search/ranking narrowed files",
            "Search shortlist",
        ),
        ModeledSourceGroup::new(
            "Other modeled narrowing",
            "Other narrowing",
            "Additional modeled avoidance",
            "Other modeled",
        ),
    ];
    for bucket in overview
        .buckets
        .iter()
        .filter(|bucket| !is_observed_source_bucket(bucket))
    {
        let index = modeled_group_index(bucket);
        groups[index].add_bucket(bucket);
    }
    groups
        .into_iter()
        .filter(|group| group.steps > 0 || group.gross_tokens != 0)
        .collect()
}

/// Pick the reference-style source row for one modeled bucket.
fn modeled_group_index(bucket: &TokenBucketOverview) -> usize {
    match (
        bucket.baseline_kind.as_str(),
        bucket.denominator_kind.as_str(),
    ) {
        (TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_DIRECTORY_WALK) => 0,
        (TOKEN_BASELINE_DIRECTORY_WALK, TOKEN_BASELINE_SELECTED_CANDIDATES) => 1,
        (TOKEN_BASELINE_SELECTED_CANDIDATES, _) | (_, TOKEN_BASELINE_SELECTED_CANDIDATES) => 2,
        _ => 3,
    }
}

/// Whether a bucket represents observed `ProjectAtlas` file handling.
fn is_observed_source_bucket(bucket: &TokenBucketOverview) -> bool {
    bucket.accounting_layer == TOKEN_ACCOUNTING_OBSERVED_DELTA
        || bucket.token_savings_bucket == TOKEN_BUCKET_FULL_FILE_COMPRESSION
        || bucket.baseline_kind == TOKEN_BASELINE_FULL_FILE
}

/// Allocate a signed display total across real source groups and preserve the exact sum.
fn allocate_signed_total(total: isize, weights: &[usize]) -> Vec<isize> {
    if weights.is_empty() {
        return Vec::new();
    }
    let total_weight = weights.iter().copied().sum::<usize>();
    let effective_weights = if total_weight == 0 {
        vec![1; weights.len()]
    } else {
        weights.to_vec()
    };
    let effective_total = effective_weights.iter().copied().sum::<usize>();
    let mut allocated = Vec::with_capacity(effective_weights.len());
    let mut assigned = 0isize;
    for (index, weight) in effective_weights.iter().copied().enumerate() {
        let value = if index + 1 == effective_weights.len() {
            total.saturating_sub(assigned)
        } else {
            split_signed_by_ratio(total, weight, effective_total)
        };
        assigned = assigned.saturating_add(value);
        allocated.push(value);
    }
    allocated
}

/// Split a signed value by a simple integer ratio.
fn split_signed_by_ratio(value: isize, part: usize, total: usize) -> isize {
    if total == 0 {
        return 0;
    }
    let magnitude = value.unsigned_abs();
    let split = magnitude.saturating_mul(part) / total;
    if value < 0 {
        -(isize::try_from(split).unwrap_or(isize::MAX))
    } else {
        isize::try_from(split).unwrap_or(isize::MAX)
    }
}

/// Return the screenshot hero's reconciled conservative baseline operand.
fn reconciled_without_projectatlas(overview: &TokenOverview) -> isize {
    usize_to_isize_saturating(overview.estimated_with_projectatlas)
        .saturating_add(overview.tokens_avoided)
}

/// Convert a `usize` to `isize` without panicking on unusually large values.
fn usize_to_isize_saturating(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

/// Return the color for a signed value.
fn signed_color(value: isize) -> Color {
    if value >= 0 { THEME_GREEN } else { THEME_RED }
}

/// Large positive/negative hero value style.
fn hero_value_style(value: isize) -> Style {
    Style::default()
        .fg(signed_color(value))
        .bg(THEME_PANEL)
        .add_modifier(Modifier::BOLD)
}

/// Header style used for panel titles.
fn header_style() -> Style {
    Style::default()
        .fg(THEME_BLUE)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

/// Mascot and identity label style.
fn identity_style() -> Style {
    Style::default()
        .fg(THEME_INK_WHITE)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

/// `ProjectAtlas` title identity style.
fn identity_title_style() -> Style {
    Style::default()
        .fg(THEME_INK_WHITE)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

/// Token Impact title style.
fn token_title_style() -> Style {
    Style::default()
        .fg(THEME_BLUE)
        .bg(THEME_BG)
        .add_modifier(Modifier::BOLD)
}

/// Body text style.
fn body_style() -> Style {
    Style::default().fg(THEME_TEXT).bg(THEME_BG)
}

/// Muted text style.
fn muted_style() -> Style {
    Style::default().fg(THEME_MUTED).bg(THEME_BG)
}

/// Muted bold label style.
fn muted_bold_style() -> Style {
    muted_style().add_modifier(Modifier::BOLD)
}

/// Format a percentage with one decimal place.
fn percentage_one_decimal(value: f64) -> String {
    format!("{:.1}%", value.clamp(0.0, 1.0) * 100.0)
}

/// Return a compact clock label for the footer status row.
fn current_clock_label() -> String {
    let seconds_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let seconds_today = seconds_since_epoch % 86_400;
    let hours = seconds_today / 3_600;
    let minutes = (seconds_today % 3_600) / 60;
    let seconds = seconds_today % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
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
        .unwrap_or(140)
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
        DASHBOARD_HEIGHT, THEME_BLUE, THEME_GREEN, THEME_INK_WHITE, THEME_YELLOW, block_bar,
        dashboard_width, grouped_count, reconciled_without_projectatlas,
        render_dashboard_to_string, render_overview_frame, render_token_dashboard,
        render_token_trend_dashboard, savings_source_rows_for_width, signed_count,
        signed_trend_points, signed_y_bounds,
    };
    use projectatlas_core::telemetry::{
        TOKEN_ACCOUNTING_MODELED_AVOIDANCE, TOKEN_BASELINE_DIRECTORY_WALK,
        TOKEN_BASELINE_SELECTED_CANDIDATES, TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
        TOKEN_CONFIDENCE_INFERRED, TOKEN_CONFIDENCE_POLICY_ESTIMATE, TOKEN_DEDUPE_SCOPE_SESSION,
        TokenOverview, TokenTrendPeriod, TokenTrendReport, TokenTrendWindow, usage_from_estimates,
        usage_from_estimates_with_accounting, usage_from_text,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};
    use ratatui::text::Line;

    #[test]
    fn overview_dashboard_matches_reference_sections_and_order() {
        let overview = sample_overview();
        let dashboard = render_token_dashboard(&overview, Some("s"));

        for text in [
            "Ani",
            "ProjectAtlas",
            "Token Impact",
            "Smarter context. Fewer tokens. Real savings.",
            "Session:",
            "Lookups:",
            "Estimate:",
            "TOTAL TOKENS AVOIDED",
            "tokens avoided",
            "Without ProjectAtlas",
            "With ProjectAtlas",
            "Saved by ProjectAtlas",
            "FILE READS AVOIDED",
            "file reads avoided",
            "Observed (summaries/slices)",
            "Search-modeled narrowing",
            "Confidence",
            "SAVINGS COMPOSITION",
            "Measured from summaries/slices",
            "Navigation narrowing",
            "SIGNAL",
            "Repeated baselines collapsed",
            "Estimate type: local model",
            "Tokenizer audit:",
            "WHERE THE SAVINGS CAME FROM",
            "Source",
            "Steps",
            "Tokens Avoided",
            "What it means",
            "Summaries and slices",
            "Skipped broad folder walk",
            "Opened fewer candidates (A)",
            "Opened fewer candidates (B)",
            "CALIBRATION & NOTES",
            "q  Quit",
            "?  Help",
            "r  Refresh",
            "Auto",
        ] {
            assert!(
                dashboard.contains(text),
                "dashboard should contain {text:?}"
            );
        }

        assert!(!dashboard.contains("ProjectAtlas Savings Overview"));
        assert!(!dashboard.contains("Saved-token trends"));
        assert!(!dashboard.contains("day trend"));
        assert!(!dashboard.contains("week trend"));
        assert!(!dashboard.contains("month trend"));
        assert!(!dashboard.contains("year trend"));
        assert!(dashboard_contains_time(&dashboard));

        assert_in_order(
            &dashboard,
            &[
                "ProjectAtlas",
                "TOTAL TOKENS AVOIDED",
                "FILE READS AVOIDED",
                "SAVINGS COMPOSITION",
                "WHERE THE SAVINGS CAME FROM",
                "CALIBRATION & NOTES",
            ],
        );
        assert_header_margin(&dashboard, "Source", "Summaries and slices");
    }

    #[test]
    fn overview_dashboard_uses_reference_ratatui_styles() {
        let overview = sample_overview();
        let buffer = render_overview_buffer(&overview, Some("s"));

        assert_cell_style(&buffer, "ProjectAtlas", THEME_INK_WHITE, Modifier::BOLD);
        assert_cell_style(&buffer, "Token Impact", THEME_BLUE, Modifier::BOLD);
        assert_cell_style(&buffer, "TOTAL TOKENS AVOIDED", THEME_BLUE, Modifier::BOLD);
        assert_cell_style(
            &buffer,
            &signed_count(overview.tokens_avoided),
            THEME_GREEN,
            Modifier::BOLD,
        );
        assert_cell_style(
            &buffer,
            &signed_count(reconciled_without_projectatlas(&overview)),
            THEME_BLUE,
            Modifier::BOLD,
        );
        assert_cell_style(
            &buffer,
            "Without ProjectAtlas",
            THEME_BLUE,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "With ProjectAtlas",
            THEME_INK_WHITE,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Saved by ProjectAtlas",
            THEME_GREEN,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Observed (summaries/slices)",
            THEME_INK_WHITE,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Measured from summaries/slices",
            THEME_INK_WHITE,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Navigation narrowing",
            THEME_YELLOW,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Summaries and slices",
            THEME_INK_WHITE,
            Modifier::empty(),
        );
        assert_cell_style(
            &buffer,
            "Skipped broad folder walk",
            THEME_YELLOW,
            Modifier::empty(),
        );
        assert_cell_style(&buffer, "Tokens Avoided", THEME_BLUE, Modifier::BOLD);
        assert_cell_style(
            &buffer,
            "Search-modeled narrowing",
            THEME_YELLOW,
            Modifier::empty(),
        );
    }

    #[test]
    fn overview_dashboard_uses_compact_reference_table_at_narrow_width() {
        let overview = sample_overview();
        let dashboard = render_dashboard_to_string(80, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame(frame, &overview, Some("s"));
        });

        assert!(dashboard.contains("ProjectAtlas"));
        assert!(dashboard.contains("Token Impact"));
        assert!(dashboard.contains("TOTAL TOKENS AVOIDED"));
        assert!(dashboard.contains("FILE READS AVOIDED"));
        assert!(dashboard.contains("WHERE THE SAVINGS CAME FROM"));
        assert!(dashboard.contains("Fewer candidates B"));
        assert!(dashboard.contains("CALIBRATION & NOTES"));
        assert!(!dashboard.contains("Saved-token trends"));
    }

    #[test]
    fn overview_dashboard_fields_use_consistent_accounting_layers() {
        let overview = sample_overview();
        let conservative_avoided = overview.tokens_avoided;
        let with_projectatlas = overview.estimated_with_projectatlas as isize;
        let without_projectatlas = reconciled_without_projectatlas(&overview);

        assert_eq!(
            without_projectatlas - with_projectatlas,
            conservative_avoided
        );
        assert_eq!(
            overview.measured_tokens_saved + overview.deduped_modeled_tokens_avoided,
            conservative_avoided
        );
        assert_eq!(
            overview.observed_file_read_replacements + overview.modeled_file_reads_avoided,
            overview.likely_file_reads_avoided
        );

        let dashboard = render_token_dashboard(&overview, Some("s"));
        let source_rows = savings_source_rows_for_width(&overview, false);
        let source_steps = source_rows.iter().map(|row| row.steps).sum::<usize>();
        let source_tokens = source_rows.iter().map(|row| row.tokens).sum::<isize>();

        assert_eq!(source_steps, overview.calls);
        assert_eq!(source_tokens, conservative_avoided);
        assert!(dashboard.contains(&signed_count(without_projectatlas)));
        assert!(dashboard.contains(&signed_count(with_projectatlas)));
        assert!(dashboard.contains(&signed_count(conservative_avoided)));
        assert!(dashboard.contains(&grouped_count(overview.likely_file_reads_avoided)));
        assert!(dashboard.contains(&grouped_count(overview.observed_file_read_replacements)));
        assert!(dashboard.contains(&grouped_count(overview.modeled_file_reads_avoided)));
    }

    #[test]
    fn overview_dashboard_token_mix_percentages_follow_saved_token_operands() {
        let overview = TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                &"x".repeat(400),
                &"x".repeat(320),
            ),
            usage_from_estimates("s", "search", None, Some("token".to_string()), 100, 20),
        ]);

        assert_eq!(overview.measured_tokens_saved, 20);
        assert_eq!(overview.deduped_modeled_tokens_avoided, 80);
        assert_eq!(overview.tokens_avoided, 100);

        let dashboard = render_token_dashboard(&overview, Some("s"));
        assert!(dashboard.contains("20.0%"));
        assert!(dashboard.contains("80.0%"));
        assert!(dashboard.contains("Measured from summaries/slices"));
        assert!(dashboard.contains("Navigation narrowing"));
    }

    #[test]
    fn overview_dashboard_bars_reflect_expected_ratios() {
        let full = block_bar(10, 1.0, THEME_BLUE);
        assert_bar_segments(&full, 10, 0, THEME_BLUE);

        let partial = block_bar(10, 0.52, THEME_GREEN);
        assert_bar_segments(&partial, 5, 5, THEME_GREEN);

        let clamped = block_bar(10, 2.0, THEME_YELLOW);
        assert_bar_segments(&clamped, 10, 0, THEME_YELLOW);

        let empty = block_bar(10, -1.0, THEME_BLUE);
        assert_bar_segments(&empty, 0, 10, THEME_BLUE);
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

        let dashboard = render_token_dashboard(&overview, Some("s"));
        assert!(dashboard.contains(&format!(
            "Signed mix: observed {} / modeled {}; net {}",
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

    fn sample_overview() -> TokenOverview {
        TokenOverview::from_events(&[
            usage_from_text(
                "s",
                "summary",
                Some("src/lib.rs".to_string()),
                None,
                &"x".repeat(400),
                &"x".repeat(320),
            ),
            usage_from_estimates_with_accounting(
                "s",
                "folders",
                None,
                Some("src".to_string()),
                120,
                20,
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                TOKEN_BASELINE_DIRECTORY_WALK,
                TOKEN_CONFIDENCE_POLICY_ESTIMATE,
                TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
                TOKEN_BASELINE_DIRECTORY_WALK,
                TOKEN_DEDUPE_SCOPE_SESSION,
            ),
            usage_from_estimates_with_accounting(
                "s",
                "search",
                None,
                Some("src".to_string()),
                80,
                16,
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                TOKEN_BASELINE_DIRECTORY_WALK,
                TOKEN_CONFIDENCE_POLICY_ESTIMATE,
                TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
                TOKEN_BASELINE_SELECTED_CANDIDATES,
                TOKEN_DEDUPE_SCOPE_SESSION,
            ),
            usage_from_estimates_with_accounting(
                "s",
                "search",
                None,
                Some("token".to_string()),
                100,
                20,
                TOKEN_BUCKET_NAVIGATION_AVOIDANCE,
                TOKEN_BASELINE_SELECTED_CANDIDATES,
                TOKEN_CONFIDENCE_INFERRED,
                TOKEN_ACCOUNTING_MODELED_AVOIDANCE,
                TOKEN_BASELINE_SELECTED_CANDIDATES,
                TOKEN_DEDUPE_SCOPE_SESSION,
            ),
        ])
    }

    fn render_overview_buffer(overview: &TokenOverview, session: Option<&str>) -> Buffer {
        let width = dashboard_width().clamp(80, 140) as u16;
        let backend = TestBackend::new(width, DASHBOARD_HEIGHT);
        let mut terminal =
            Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
        let frame = terminal
            .draw(|frame| render_overview_frame(frame, overview, session))
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

    fn assert_in_order(dashboard: &str, needles: &[&str]) {
        let mut previous = 0usize;
        for needle in needles {
            let Some(index) = dashboard.find(needle) else {
                assert!(
                    dashboard.contains(needle),
                    "dashboard should contain {needle:?}"
                );
                return;
            };
            assert!(
                index >= previous,
                "{needle:?} should appear after the previous section"
            );
            previous = index;
        }
    }

    fn dashboard_contains_time(dashboard: &str) -> bool {
        let bytes = dashboard.as_bytes();
        bytes.windows(8).any(|window| {
            window.len() == 8
                && window[2] == b':'
                && window[5] == b':'
                && window
                    .iter()
                    .enumerate()
                    .all(|(index, byte)| index == 2 || index == 5 || byte.is_ascii_digit())
        })
    }

    fn assert_bar_segments(line: &Line<'_>, filled: usize, empty: usize, color: Color) {
        assert_eq!(line.spans.len(), 2);
        assert_eq!(line.spans[0].content.chars().count(), filled);
        assert_eq!(line.spans[1].content.chars().count(), empty);
        assert!(
            line.spans[0]
                .content
                .chars()
                .all(|character| character == '━')
        );
        assert!(
            line.spans[1]
                .content
                .chars()
                .all(|character| character == '·')
        );
        assert_eq!(line.spans[0].style.fg, Some(color));
        assert_eq!(line.spans[1].style.fg, Some(Color::DarkGray));
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
