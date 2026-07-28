//! Purpose: Render token telemetry as package-backed terminal dashboards.

use projectatlas_core::graph::{ExtendedRelationKind, GraphRelationKind, LogicalRelation};
use projectatlas_core::symbols::RelationKind;
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
use ratatui::widgets::canvas::{Canvas, Circle, Line as CanvasLine, Points};
use ratatui::widgets::{Axis, Block, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table, Wrap};
use ratatui::{Frame, Terminal};
use std::cell::Cell as StdCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::f64::consts::TAU;
use std::time::{SystemTime, UNIX_EPOCH};

/// Fixed terminal height for the token overview dashboard snapshot.
const DASHBOARD_HEIGHT: u16 = 48;
/// Width at which the human dashboard can show the atlas without crowding impact data.
const ATLAS_DASHBOARD_MIN_WIDTH: usize = 190;
/// Maximum human dashboard width.
const DASHBOARD_MAX_WIDTH: usize = 200;
/// Stable width reserved for the token-impact column in the wide dashboard.
const TOKEN_IMPACT_COLUMN_WIDTH: u16 = 140;
/// Maximum real resolved nodes retained by the decorative atlas preview.
const ATLAS_PREVIEW_MAX_NODES: usize = 48;
/// Maximum real resolved links retained by the decorative atlas preview.
const ATLAS_PREVIEW_MAX_EDGES: usize = 64;
/// Maximum links one visual hub may consume in the decorative preview.
const ATLAS_PREVIEW_MAX_NODE_DEGREE: usize = 12;
/// Maximum links one relation family may consume in the decorative preview.
const ATLAS_PREVIEW_MAX_EDGES_PER_KIND: usize = 16;
/// Horizontal Canvas bound retaining a margin inside the atlas panel.
const ATLAS_CANVAS_X_BOUND: f64 = 33.0;
/// Vertical Canvas bound retaining a margin above the atlas footer.
const ATLAS_CANVAS_Y_BOUND: f64 = 21.0;
/// Fixed terminal height for the token trend dashboard snapshot.
const TREND_DASHBOARD_HEIGHT: u16 = 30;
/// Reserved terminal-canvas color; overview frames leave the shell background visible.
const THEME_BG: Color = Color::Rgb(4, 10, 18);
/// Token dashboard panel background.
const THEME_PANEL: Color = Color::Rgb(5, 16, 25);
/// Token dashboard primary warm text.
const THEME_TEXT: Color = Color::Rgb(224, 198, 164);
/// Token dashboard muted label text.
const THEME_MUTED: Color = Color::Rgb(170, 143, 116);
/// Token dashboard identity ivory.
const THEME_INK_WHITE: Color = Color::Rgb(238, 234, 224);
/// Counterfactual/original-baseline blue.
const THEME_BLUE: Color = Color::Rgb(93, 143, 255);
/// Net saved/success green.
const THEME_GREEN: Color = Color::Rgb(111, 216, 100);
/// Modeled/search/estimate yellow.
const THEME_YELLOW: Color = Color::Rgb(230, 179, 55);
/// Token dashboard subtle warm panel border.
const THEME_BORDER: Color = Color::Rgb(92, 74, 55);
/// Token dashboard inactive bar cells.
const THEME_BAR_EMPTY: Color = Color::Rgb(49, 56, 57);
/// Token dashboard loss red.
const THEME_RED: Color = Color::Rgb(235, 95, 95);
/// Repository-graph test and route accent.
const THEME_PURPLE: Color = Color::Rgb(173, 127, 255);
/// Human token dashboard color mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenDashboardTheme {
    /// Reference dark dashboard theme.
    Dark,
    /// Light dashboard theme for light terminal backgrounds.
    Light,
    /// Preserve the terminal background while retaining semantic accents.
    Terminal,
}

impl TokenDashboardTheme {
    /// Parse a token dashboard theme value.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            "terminal" => Some(Self::Terminal),
            _ => None,
        }
    }
}

/// Semantic color palette used when serializing Ratatui cells to ANSI.
#[derive(Clone, Copy)]
struct ThemePalette {
    /// Full-screen background.
    bg: Color,
    /// Panel background.
    panel: Color,
    /// Primary text.
    text: Color,
    /// Muted text.
    muted: Color,
    /// Product identity color.
    ink_white: Color,
    /// Counterfactual baseline blue.
    blue: Color,
    /// Saved/success green.
    green: Color,
    /// Modeled/estimate yellow.
    yellow: Color,
    /// Panel border.
    border: Color,
    /// Empty bar fill.
    bar_empty: Color,
    /// Negative/loss red.
    red: Color,
}

/// One real resolved relation retained by the bounded atlas preview.
#[derive(Clone, Debug, Eq, PartialEq)]
struct AtlasPreviewEdge {
    /// Stable compact source identity.
    source: String,
    /// Stable compact target identity.
    target: String,
    /// Typed relation family used for semantic color.
    kind: GraphRelationKind,
}

/// Bounded, non-interactive projection of resolved relations from the active project database.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TokenAtlasPreview {
    /// Real resolved relations admitted within the node and edge ceilings.
    edges: Vec<AtlasPreviewEdge>,
    /// Whether a source page or a preview ceiling omitted additional relations.
    truncated: bool,
    /// Whether the optional graph read completed successfully.
    available: bool,
}

impl TokenAtlasPreview {
    /// Build an empty but available graph snapshot.
    #[must_use]
    pub(crate) const fn empty() -> Self {
        Self {
            edges: Vec::new(),
            truncated: false,
            available: true,
        }
    }

    /// Build the explicit state used when the optional graph read fails.
    #[must_use]
    pub(crate) const fn unavailable() -> Self {
        Self {
            edges: Vec::new(),
            truncated: false,
            available: false,
        }
    }

    /// Retain only exact local resolutions from bounded database relation pages.
    #[must_use]
    pub(crate) fn from_relations(relations: &[LogicalRelation], source_truncated: bool) -> Self {
        Self::from_resolved_edges(
            relations.iter().filter_map(|relation| {
                relation.resolution().resolved_target().map(|target| {
                    (
                        relation.source().digest().to_string(),
                        target.digest().to_string(),
                        relation.kind(),
                    )
                })
            }),
            source_truncated,
        )
    }

    /// Build the bounded projection from already resolved stable identities.
    fn from_resolved_edges(
        relations: impl IntoIterator<Item = (String, String, GraphRelationKind)>,
        source_truncated: bool,
    ) -> Self {
        let mut candidates = BTreeMap::new();
        for (source, target, kind) in relations {
            candidates
                .entry((source.clone(), target.clone(), kind.as_str()))
                .or_insert(AtlasPreviewEdge {
                    source,
                    target,
                    kind,
                });
        }
        let mut degrees = BTreeMap::<String, usize>::new();
        for edge in candidates.values() {
            *degrees.entry(edge.source.clone()).or_default() += 1;
            *degrees.entry(edge.target.clone()).or_default() += 1;
        }
        let mut candidates = candidates.into_values().collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            let score = |edge: &AtlasPreviewEdge| {
                degrees.get(&edge.source).copied().unwrap_or_default()
                    + degrees.get(&edge.target).copied().unwrap_or_default()
            };
            score(right)
                .cmp(&score(left))
                .then_with(|| left.source.cmp(&right.source))
                .then_with(|| left.target.cmp(&right.target))
                .then_with(|| left.kind.as_str().cmp(right.kind.as_str()))
        });

        let candidate_count = candidates.len();
        let mut nodes = BTreeSet::new();
        let mut edges = Vec::new();
        let mut selected_degrees = BTreeMap::<String, usize>::new();
        let mut selected_kinds = BTreeMap::<String, usize>::new();
        let mut truncated = source_truncated || candidate_count > ATLAS_PREVIEW_MAX_EDGES;
        for edge in candidates {
            if selected_degrees
                .get(&edge.source)
                .copied()
                .unwrap_or_default()
                == ATLAS_PREVIEW_MAX_NODE_DEGREE
                || selected_degrees
                    .get(&edge.target)
                    .copied()
                    .unwrap_or_default()
                    == ATLAS_PREVIEW_MAX_NODE_DEGREE
                || selected_kinds
                    .get(edge.kind.as_str())
                    .copied()
                    .unwrap_or_default()
                    == ATLAS_PREVIEW_MAX_EDGES_PER_KIND
            {
                truncated = true;
                continue;
            }
            let additional_nodes = usize::from(!nodes.contains(&edge.source))
                + usize::from(!nodes.contains(&edge.target));
            if nodes.len() + additional_nodes > ATLAS_PREVIEW_MAX_NODES {
                truncated = true;
                continue;
            }
            nodes.insert(edge.source.clone());
            nodes.insert(edge.target.clone());
            *selected_degrees.entry(edge.source.clone()).or_default() += 1;
            *selected_degrees.entry(edge.target.clone()).or_default() += 1;
            *selected_kinds
                .entry(edge.kind.as_str().to_string())
                .or_default() += 1;
            edges.push(edge);
            if edges.len() == ATLAS_PREVIEW_MAX_EDGES {
                break;
            }
        }
        Self {
            edges,
            truncated,
            available: true,
        }
    }

    /// Return the exact number of distinct nodes drawn by this preview.
    fn node_count(&self) -> usize {
        self.edges
            .iter()
            .flat_map(|edge| [&edge.source, &edge.target])
            .collect::<BTreeSet<_>>()
            .len()
    }
}

/// Light terminal palette preserving the same semantic color roles.
const LIGHT_THEME: ThemePalette = ThemePalette {
    bg: Color::Rgb(252, 249, 241),
    panel: Color::Rgb(246, 242, 232),
    text: Color::Rgb(34, 32, 28),
    muted: Color::Rgb(96, 88, 76),
    ink_white: Color::Rgb(22, 22, 20),
    blue: Color::Rgb(37, 99, 235),
    green: Color::Rgb(22, 128, 72),
    yellow: Color::Rgb(178, 116, 0),
    border: Color::Rgb(175, 151, 111),
    bar_empty: Color::Rgb(218, 210, 196),
    red: Color::Rgb(190, 52, 52),
};

thread_local! {
    /// Active token dashboard theme for the current render pass.
    static ACTIVE_TOKEN_THEME: StdCell<TokenDashboardTheme> = const { StdCell::new(TokenDashboardTheme::Dark) };
}

/// Render the token overview as a human terminal dashboard.
#[cfg(test)]
pub(crate) fn render_token_dashboard(overview: &TokenOverview, session: Option<&str>) -> String {
    render_token_dashboard_with_theme(overview, session, TokenDashboardTheme::Dark)
}

/// Render the token overview as a human terminal dashboard with the selected theme.
#[cfg(test)]
pub(crate) fn render_token_dashboard_with_theme(
    overview: &TokenOverview,
    session: Option<&str>,
    theme: TokenDashboardTheme,
) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    with_token_theme(theme, || {
        render_dashboard_to_ansi_string(width, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame(frame, overview, session);
        })
    })
}

/// Render the human token dashboard with its optional bounded live atlas.
pub(crate) fn render_token_dashboard_with_atlas(
    overview: &TokenOverview,
    session: Option<&str>,
    atlas: &TokenAtlasPreview,
    theme: TokenDashboardTheme,
) -> String {
    let width = dashboard_width().clamp(80, DASHBOARD_MAX_WIDTH) as u16;
    with_token_theme(theme, || {
        render_dashboard_to_ansi_string(width, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame_with_atlas(frame, overview, session, Some(atlas));
        })
    })
}

/// Return whether the current terminal is wide enough for the optional atlas read.
#[must_use]
pub(crate) fn token_dashboard_wants_atlas() -> bool {
    dashboard_width() >= ATLAS_DASHBOARD_MIN_WIDTH
}

/// Render the token overview as a plain terminal chart for agent payloads.
pub(crate) fn render_token_dashboard_plain_with_theme(
    overview: &TokenOverview,
    session: Option<&str>,
    theme: TokenDashboardTheme,
) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    with_token_theme(theme, || {
        render_dashboard_to_string(width, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame(frame, overview, session);
        })
    })
}

/// Render token trends as a human terminal dashboard.
#[cfg(test)]
pub(crate) fn render_token_trend_dashboard(report: &TokenTrendReport) -> String {
    render_token_trend_dashboard_with_theme(report, TokenDashboardTheme::Dark)
}

/// Render token trends as a human terminal dashboard with the selected theme.
pub(crate) fn render_token_trend_dashboard_with_theme(
    report: &TokenTrendReport,
    theme: TokenDashboardTheme,
) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    with_token_theme(theme, || {
        render_dashboard_to_ansi_string(width, TREND_DASHBOARD_HEIGHT, |frame| {
            render_trend_frame(frame, report);
        })
    })
}

/// Render token trends as a plain terminal chart for agent payloads.
pub(crate) fn render_token_trend_dashboard_plain_with_theme(
    report: &TokenTrendReport,
    theme: TokenDashboardTheme,
) -> String {
    let width = dashboard_width().clamp(80, 140) as u16;
    with_token_theme(theme, || {
        render_dashboard_to_string(width, TREND_DASHBOARD_HEIGHT, |frame| {
            render_trend_frame(frame, report);
        })
    })
}

/// Run one render closure with the selected token dashboard theme.
fn with_token_theme<R>(theme: TokenDashboardTheme, render: impl FnOnce() -> R) -> R {
    ACTIVE_TOKEN_THEME.with(|active| {
        let previous = active.replace(theme);
        let result = render();
        active.set(previous);
        result
    })
}

/// Return the active token dashboard theme.
fn active_token_theme() -> TokenDashboardTheme {
    ACTIVE_TOKEN_THEME.with(StdCell::get)
}

/// Render one Ratatui frame into a deterministic ANSI terminal buffer.
fn render_dashboard_to_ansi_string<F>(width: u16, height: u16, render: F) -> String
where
    F: FnOnce(&mut Frame<'_>),
{
    let backend = TestBackend::new(width, height);
    let mut terminal =
        Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
    let frame = terminal
        .draw(render)
        .expect("in-memory token dashboard should render");
    buffer_to_ansi_string(frame.buffer)
}

/// Render one Ratatui frame into a deterministic plain string buffer.
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
    render_overview_frame_with_atlas(frame, overview, session, None);
}

/// Draw the overview and, when requested and wide enough, its static live atlas.
fn render_overview_frame_with_atlas(
    frame: &mut Frame<'_>,
    overview: &TokenOverview,
    session: Option<&str>,
    atlas: Option<&TokenAtlasPreview>,
) {
    let area = frame.area();
    let outer = Block::bordered()
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(THEME_BORDER))
        .style(Style::default().fg(THEME_TEXT));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    render_window_title_bar(frame, area);

    if area.width < u16::try_from(ATLAS_DASHBOARD_MIN_WIDTH).unwrap_or(u16::MAX) || atlas.is_none()
    {
        render_overview_main(frame, inner, overview, session);
        return;
    }
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(TOKEN_IMPACT_COLUMN_WIDTH),
            Constraint::Min(48),
        ])
        .split(inner);
    render_overview_main(frame, columns[0], overview, session);
    if let Some(atlas) = atlas {
        render_atlas_map(frame, columns[1], atlas);
    }
}

/// Draw the proven one-screen savings overview.
fn render_overview_main(
    frame: &mut Frame<'_>,
    area: Rect,
    overview: &TokenOverview,
    session: Option<&str>,
) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(13),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Min(8),
            Constraint::Length(4),
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
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(THEME_BORDER))
        .style(Style::default().fg(THEME_TEXT).bg(THEME_PANEL));
    if title.is_empty() {
        block
    } else {
        block.title(Span::styled(
            format!(" {} ", reference_title(title)),
            section_title_style().bg(THEME_PANEL),
        ))
    }
}

/// Draw the reference-style app title bar and window controls.
fn render_window_title_bar(frame: &mut Frame<'_>, area: Rect) {
    if area.width < 8 {
        return;
    }
    let top = Rect {
        x: area.x.saturating_add(1),
        y: area.y,
        width: area.width.saturating_sub(2),
        height: 1,
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(10),
            Constraint::Min(12),
            Constraint::Length(10),
        ])
        .split(top);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ● ", Style::default().fg(THEME_RED)),
            Span::styled("● ", Style::default().fg(THEME_YELLOW)),
            Span::styled("●", Style::default().fg(THEME_GREEN)),
        ])),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new("projectatlas -- savings-overview")
            .style(body_style())
            .alignment(Alignment::Center),
        columns[1],
    );
}

/// Draw the title band.
fn render_token_header(
    frame: &mut Frame<'_>,
    area: Rect,
    overview: &TokenOverview,
    session: Option<&str>,
) {
    frame.render_widget(
        Block::default().style(Style::default().bg(THEME_PANEL)),
        area,
    );
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(42),
            Constraint::Length(if area.width >= 110 { 46 } else { 34 }),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("ProjectAtlas", identity_title_style()),
                Span::raw(" "),
                Span::styled("Token Impact", token_title_style()),
            ]),
            Line::from(vec![
                Span::styled("Smarter context. Fewer tokens. ", body_style()),
                Span::styled("Real savings.", Style::default().fg(THEME_GREEN)),
            ]),
        ])
        .style(Style::default().bg(THEME_PANEL))
        .wrap(Wrap { trim: true }),
        columns[0],
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
        .style(Style::default().bg(THEME_PANEL))
        .alignment(Alignment::Right)
        .wrap(Wrap { trim: true }),
        columns[1],
    );
}

/// Draw the dominant saved-token hero panel.
fn render_token_hero(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let block = panel("").border_style(Style::default().fg(THEME_BORDER));
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
        Paragraph::new(reference_title("TOTAL TOKENS AVOIDED"))
            .style(section_title_style().bg(THEME_PANEL))
            .alignment(Alignment::Center),
        rows[0],
    );
    render_hero_value(frame, rows[1], overview.tokens_avoided);
    frame.render_widget(
        Paragraph::new("tokens avoided")
            .style(body_style().bg(THEME_PANEL))
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

/// Draw the saved-token headline as readable terminal text.
fn render_hero_value(frame: &mut Frame<'_>, area: Rect, value: isize) {
    let text = signed_count(value);
    let style = hero_value_style(value);
    let marker = hero_state_marker(value);
    let line = if area.width >= 48 {
        let mut spans = vec![Span::styled(text, style)];
        if let Some(marker) = marker {
            spans.push(Span::styled(format!("  {marker}"), style));
        }
        Line::from(spans)
    } else {
        Line::from(Span::styled(text, style))
    };
    frame.render_widget(
        Paragraph::new(line)
            .style(style)
            .alignment(Alignment::Center),
        area,
    );
}

/// Return the semantic marker used beside the saved-token headline.
fn hero_state_marker(value: isize) -> Option<&'static str> {
    match value.cmp(&0) {
        std::cmp::Ordering::Greater => Some("✓"),
        std::cmp::Ordering::Less => Some("!"),
        std::cmp::Ordering::Equal => None,
    }
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
    let block = panel("");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let compact = inner.width < 100;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(reference_title("LIKELY FILE-READ CALLS AVOIDED"))
            .style(section_title_style().bg(THEME_PANEL)),
        rows[0],
    );

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
        .split(rows[1]);
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

    render_file_read_total(frame, columns[0], total_reads);
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
        ])
        .style(body_style().bg(THEME_PANEL)),
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
        ])
        .style(body_style().bg(THEME_PANEL)),
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
        .style(body_style().bg(THEME_PANEL))
        .alignment(Alignment::Center),
        columns[6],
    );
}

/// Draw the left file-read total with the reference document-icon hierarchy.
fn render_file_read_total(frame: &mut Frame<'_>, area: Rect, total_reads: usize) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(7), Constraint::Min(8)])
        .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("╭──╮", identity_style().bg(THEME_PANEL))),
            Line::from(Span::styled("│≡ │", identity_style().bg(THEME_PANEL))),
            Line::from(Span::styled("╰──╯", identity_style().bg(THEME_PANEL))),
        ])
        .alignment(Alignment::Center)
        .style(body_style().bg(THEME_PANEL)),
        columns[0],
    );
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
                "file-read calls avoided",
                muted_style().bg(THEME_PANEL),
            )),
        ])
        .style(body_style().bg(THEME_PANEL)),
        columns[1],
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
        || "not run".to_string(),
        |calibration| calibration.tokenizer.clone(),
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("▣  ", Style::default().fg(THEME_INK_WHITE).bg(THEME_PANEL)),
                Span::styled("Impact scope: ", body_style().bg(THEME_PANEL)),
                Span::styled(
                    "tokens + file-read calls",
                    Style::default().fg(THEME_INK_WHITE).bg(THEME_PANEL),
                ),
            ]),
            Line::from(vec![
                Span::styled("⌁  ", Style::default().fg(THEME_INK_WHITE).bg(THEME_PANEL)),
                Span::styled("Estimate type: ", body_style().bg(THEME_PANEL)),
                Span::styled(
                    "local model",
                    Style::default().fg(THEME_YELLOW).bg(THEME_PANEL),
                ),
            ]),
            Line::from(vec![
                Span::styled("◇  ", Style::default().fg(THEME_INK_WHITE).bg(THEME_PANEL)),
                Span::styled("Tokenizer audit: ", body_style().bg(THEME_PANEL)),
                Span::styled(tokenizer, body_style().bg(THEME_PANEL)),
            ]),
        ])
        .block(panel("SIGNAL"))
        .style(body_style().bg(THEME_PANEL))
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
            Constraint::Min(14),
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
                Cell::from(format!("{}  {}", source.icon, source.label)),
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

/// Draw the bounded live repository constellation in the wide dashboard column.
fn render_atlas_map(frame: &mut Frame<'_>, area: Rect, atlas: &TokenAtlasPreview) {
    let block = panel("ATLAS MAP");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Length(2)])
        .split(inner);

    if !atlas.available {
        render_atlas_message(
            frame,
            rows[0],
            "Graph preview unavailable",
            "Token-impact data remains available",
        );
        return;
    }
    if atlas.edges.is_empty() {
        render_atlas_message(
            frame,
            rows[0],
            "No resolved graph links",
            "Run projectatlas scan to refresh",
        );
        return;
    }

    let layout = atlas_layout(&atlas.edges);
    let mut node_colors = BTreeMap::new();
    for edge in &atlas.edges {
        let color = atlas_relation_color(edge.kind);
        node_colors.entry(edge.source.as_str()).or_insert(color);
        node_colors.entry(edge.target.as_str()).or_insert(color);
    }
    let canvas = Canvas::default()
        .background_color(THEME_PANEL)
        .marker(symbols::Marker::Braille)
        .x_bounds([-ATLAS_CANVAS_X_BOUND, ATLAS_CANVAS_X_BOUND])
        .y_bounds([-ATLAS_CANVAS_Y_BOUND, ATLAS_CANVAS_Y_BOUND])
        .paint(|context| {
            for edge in &atlas.edges {
                let (Some(source), Some(target)) = (
                    layout.positions.get(&edge.source),
                    layout.positions.get(&edge.target),
                ) else {
                    continue;
                };
                context.draw(&CanvasLine::new(
                    source.0,
                    source.1,
                    target.0,
                    target.1,
                    atlas_relation_color(edge.kind),
                ));
            }
            context.layer();
            for (node, (x, y)) in &layout.positions {
                let color = if *node == layout.hub {
                    THEME_INK_WHITE
                } else {
                    node_colors
                        .get(node.as_str())
                        .copied()
                        .unwrap_or(THEME_MUTED)
                };
                if *node == layout.hub {
                    context.draw(&Circle::new(*x, *y, 1.45, THEME_YELLOW));
                    let center = [(*x, *y)];
                    context.draw(&Points::new(&center, THEME_INK_WHITE));
                } else {
                    context.draw(&Circle::new(*x, *y, 0.72, color));
                }
            }
        });
    frame.render_widget(canvas, rows[0]);

    let state = if atlas.truncated {
        "bounded live graph • sampled snapshot"
    } else {
        "bounded live graph • static snapshot"
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{} nodes • {} links",
                    grouped_count(atlas.node_count()),
                    grouped_count(atlas.edges.len())
                ),
                body_style().bg(THEME_PANEL),
            )),
            Line::from(Span::styled(state, muted_style().bg(THEME_PANEL))),
        ])
        .alignment(Alignment::Center),
        rows[1],
    );
}

/// Draw an explicit centered atlas state without substituting decorative data.
fn render_atlas_message(frame: &mut Frame<'_>, area: Rect, title: &str, detail: &str) {
    let height = 2_u16.min(area.height);
    let message_area = Rect {
        x: area.x,
        y: area
            .y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width: area.width,
        height,
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                title.to_string(),
                Style::default()
                    .fg(THEME_INK_WHITE)
                    .bg(THEME_PANEL)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                detail.to_string(),
                muted_style().bg(THEME_PANEL),
            )),
        ])
        .alignment(Alignment::Center),
        message_area,
    );
}

/// Deterministic centered layout for the small resolved-relation projection.
struct AtlasLayout {
    /// Stable node positions inside the fixed Canvas safety margin.
    positions: BTreeMap<String, (f64, f64)>,
    /// Highest-connectivity node anchored at the geometric center.
    hub: String,
}

/// Place the strongest connected component at center and distribute satellites radially.
fn atlas_layout(edges: &[AtlasPreviewEdge]) -> AtlasLayout {
    let mut adjacency = BTreeMap::<String, BTreeSet<String>>::new();
    for edge in edges {
        adjacency
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
        adjacency
            .entry(edge.target.clone())
            .or_default()
            .insert(edge.source.clone());
    }
    let hub = adjacency
        .iter()
        .max_by(|left, right| {
            left.1
                .len()
                .cmp(&right.1.len())
                .then_with(|| right.0.cmp(left.0))
        })
        .map(|(node, _)| node.clone())
        .unwrap_or_default();
    let mut remaining = adjacency.keys().cloned().collect::<BTreeSet<_>>();
    let mut components = Vec::<Vec<String>>::new();
    while let Some(start) = remaining.first().cloned() {
        remaining.remove(&start);
        let mut frontier = VecDeque::from([start]);
        let mut component = Vec::new();
        while let Some(node) = frontier.pop_front() {
            component.push(node.clone());
            if let Some(neighbors) = adjacency.get(&node) {
                for neighbor in neighbors {
                    if remaining.remove(neighbor) {
                        frontier.push_back(neighbor.clone());
                    }
                }
            }
        }
        component.sort();
        components.push(component);
    }
    components.sort_by(|left, right| {
        let left_has_hub = left.contains(&hub);
        let right_has_hub = right.contains(&hub);
        right_has_hub
            .cmp(&left_has_hub)
            .then_with(|| right.len().cmp(&left.len()))
            .then_with(|| left.first().cmp(&right.first()))
    });

    let satellite_count = components.len().saturating_sub(1).max(1);
    let mut positions = BTreeMap::new();
    for (component_index, component) in components.iter().enumerate() {
        let (center_x, center_y) = if component_index == 0 {
            (0.0, 0.0)
        } else {
            let angle = TAU * (component_index - 1) as f64 / satellite_count as f64;
            (24.0 * angle.cos(), 15.0 * angle.sin())
        };
        let component_hub = if component_index == 0 {
            hub.as_str()
        } else {
            component
                .iter()
                .max_by(|left, right| {
                    adjacency
                        .get(*left)
                        .map_or(0, BTreeSet::len)
                        .cmp(&adjacency.get(*right).map_or(0, BTreeSet::len))
                        .then_with(|| right.cmp(left))
                })
                .map_or("", String::as_str)
        };
        positions.insert(component_hub.to_string(), (center_x, center_y));
        let others = component
            .iter()
            .filter(|node| node.as_str() != component_hub)
            .collect::<Vec<_>>();
        for (node_index, node) in others.iter().enumerate() {
            let ring = node_index / 12;
            let ring_start = ring * 12;
            let ring_members = (others.len() - ring_start).min(12);
            let angle = TAU * (node_index - ring_start) as f64 / ring_members.max(1) as f64;
            let radius = if component_index == 0 {
                4.8 + ring as f64 * 4.0
            } else {
                3.2 + ring as f64 * 1.2
            };
            positions.insert(
                (*node).clone(),
                (
                    (center_x + radius * 1.35 * angle.cos())
                        .clamp(-ATLAS_CANVAS_X_BOUND + 1.5, ATLAS_CANVAS_X_BOUND - 1.5),
                    (center_y + radius * angle.sin())
                        .clamp(-ATLAS_CANVAS_Y_BOUND + 1.5, ATLAS_CANVAS_Y_BOUND - 1.5),
                ),
            );
        }
    }
    AtlasLayout { positions, hub }
}

/// Map typed relation families to stable semantic constellation colors.
const fn atlas_relation_color(kind: GraphRelationKind) -> Color {
    match kind {
        GraphRelationKind::Legacy(RelationKind::Contains) => THEME_YELLOW,
        GraphRelationKind::Legacy(RelationKind::Imports | RelationKind::DependsOn) => THEME_BLUE,
        GraphRelationKind::Legacy(RelationKind::Calls)
        | GraphRelationKind::Extended(
            ExtendedRelationKind::References
            | ExtendedRelationKind::Reads
            | ExtendedRelationKind::Writes,
        ) => THEME_GREEN,
        GraphRelationKind::Extended(
            ExtendedRelationKind::Tests
            | ExtendedRelationKind::RoutesTo
            | ExtendedRelationKind::Configures
            | ExtendedRelationKind::Deploys,
        ) => THEME_PURPLE,
    }
}

/// Draw calibration notes without duplicating headline totals.
fn render_calibration_notes(frame: &mut Frame<'_>, area: Rect, overview: &TokenOverview) {
    let block = panel("CALIBRATION & NOTES");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = vec![
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
    ];
    if let Some(value) = overview.calibration.as_ref() {
        lines.push(Line::from(Span::styled(
            format!(
                "• Tokenizer audit: {} over {} files",
                value.tokenizer,
                grouped_count(value.files)
            ),
            body_style().bg(THEME_PANEL),
        )));
    }
    if inner.width < 100 {
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        return;
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Draw the compact footer/status row from the reference dashboard.
fn render_status_bar(frame: &mut Frame<'_>, area: Rect) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("ProjectAtlas v", Style::default().fg(THEME_INK_WHITE)),
            Span::styled(
                env!("CARGO_PKG_VERSION"),
                Style::default().fg(THEME_INK_WHITE),
            ),
        ])),
        columns[0],
    );
    let clock = current_clock_label();
    let status = if area.width < 100 {
        format!(
            "Snapshot {} • rerun to refresh",
            clock.get(..5).unwrap_or(&clock)
        )
    } else {
        format!("Snapshot {clock} • rerun command to refresh")
    };
    frame.render_widget(
        Paragraph::new(Span::styled(status, body_style())).alignment(Alignment::Right),
        columns[1],
    );
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
            "█".repeat(filled),
            Style::default().fg(color).bg(THEME_PANEL),
        ),
        Span::styled(
            "░".repeat(empty),
            Style::default().fg(THEME_BAR_EMPTY).bg(THEME_PANEL),
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
    /// Compact row icon.
    icon: &'static str,
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
            icon: "⌁",
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
            icon: group.icon,
            color: THEME_YELLOW,
        });
    }

    let displayed_steps = rows.iter().map(|row| row.steps).sum::<usize>();
    let displayed_tokens = rows.iter().map(|row| row.tokens).sum::<isize>();
    let step_remainder = overview.calls.saturating_sub(displayed_steps);
    let token_remainder = overview.tokens_avoided.saturating_sub(displayed_tokens);
    if step_remainder > 0 || token_remainder != 0 {
        rows.push(SavingsSourceRow {
            label: if compact {
                "Other savings"
            } else {
                "Unattributed savings"
            },
            steps: step_remainder,
            tokens: token_remainder,
            meaning: if compact {
                "Real remainder"
            } else {
                "Real remainder not tied to visible buckets"
            },
            icon: "•",
            color: THEME_MUTED,
        });
    }

    if rows.is_empty() {
        rows.push(SavingsSourceRow {
            label: "No telemetry",
            steps: 0,
            tokens: 0,
            meaning: "No token savings recorded",
            icon: " ",
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
    /// Compact source icon.
    icon: &'static str,
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
        icon: &'static str,
    ) -> Self {
        Self {
            label,
            compact_label,
            meaning,
            compact_meaning,
            icon,
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
            "□",
        ),
        ModeledSourceGroup::new(
            "Opened fewer candidates (A)",
            "Fewer candidates A",
            "Folder ranking narrowed files",
            "Folder shortlist",
            "▤",
        ),
        ModeledSourceGroup::new(
            "Opened fewer candidates (B)",
            "Fewer candidates B",
            "Search/ranking narrowed files",
            "Search shortlist",
            "▥",
        ),
        ModeledSourceGroup::new(
            "Other modeled narrowing",
            "Other narrowing",
            "Additional modeled avoidance",
            "Other modeled",
            "◇",
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
    section_title_style()
}

/// Section title style used for dashboard chrome.
fn section_title_style() -> Style {
    Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD)
}

/// Return the reference-like spaced title treatment used for dominant section labels.
fn reference_title(title: &str) -> String {
    let mut output = String::with_capacity(title.len().saturating_mul(2));
    let mut previous_was_space = false;
    for character in title.chars() {
        if character == ' ' {
            if !previous_was_space {
                output.push_str("   ");
            }
            previous_was_space = true;
        } else {
            if !output.is_empty() && !previous_was_space {
                output.push(' ');
            }
            output.push(character);
            previous_was_space = false;
        }
    }
    output
}

/// Identity label style.
fn identity_style() -> Style {
    Style::default()
        .fg(THEME_INK_WHITE)
        .add_modifier(Modifier::BOLD)
}

/// `ProjectAtlas` title identity style.
fn identity_title_style() -> Style {
    Style::default()
        .fg(THEME_INK_WHITE)
        .add_modifier(Modifier::BOLD)
}

/// Token Impact title style.
fn token_title_style() -> Style {
    Style::default().fg(THEME_BLUE).add_modifier(Modifier::BOLD)
}

/// Body text style.
fn body_style() -> Style {
    Style::default().fg(THEME_TEXT)
}

/// Muted text style.
fn muted_style() -> Style {
    Style::default().fg(THEME_MUTED)
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
        (true, true) => THEME_YELLOW,
        (false, true) => THEME_RED,
        _ => THEME_GREEN,
    }
}

/// Draw the full trend dashboard frame.
fn render_trend_frame(frame: &mut Frame<'_>, report: &TokenTrendReport) {
    let area = frame.area();
    let outer = Block::bordered()
        .border_set(symbols::border::ROUNDED)
        .title(Line::from(vec![
            Span::styled(" ProjectAtlas Token Trends ", identity_title_style()),
            Span::styled(format!("{} ", report.window), body_style()),
        ]))
        .border_style(Style::default().fg(THEME_BORDER))
        .style(Style::default().fg(THEME_TEXT));
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
        .block(panel("SAVED TOKENS TREND"))
        .x_axis(Axis::default().bounds([0.0, (trend_points.len().saturating_sub(1)) as f64]))
        .y_axis(Axis::default().bounds([lower, upper])),
        sections[1],
    );

    render_trend_table(frame, sections[2], report);
    frame.render_widget(
        Paragraph::new(
            "Trend rows are period gross estimates. Use overview mode for deduped tokens avoided.",
        )
        .style(body_style().bg(THEME_PANEL))
        .alignment(Alignment::Center)
        .block(panel("NOTE")),
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
        .style(Style::default().fg(THEME_TEXT).add_modifier(Modifier::BOLD)),
    )
    .block(panel("PERIODS"));
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

/// Convert a Ratatui buffer into ANSI-styled terminal text.
fn buffer_to_ansi_string(buffer: &Buffer) -> String {
    let width = buffer.area.width;
    let height = buffer.area.height;
    let mut output = String::new();
    let mut active_style: Option<CellAnsiStyle> = None;
    for y in 0..height {
        for x in 0..width {
            let Some(cell) = buffer.cell((x, y)) else {
                continue;
            };
            let style = CellAnsiStyle::from_cell(cell);
            if active_style != Some(style) {
                output.push_str("\x1b[0m");
                output.push_str(&style.to_ansi());
                active_style = Some(style);
            }
            output.push_str(cell.symbol());
        }
        output.push_str("\x1b[0m\n");
        active_style = None;
    }
    output
}

/// Minimal style projection used by the ANSI serializer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CellAnsiStyle {
    /// Cell foreground color.
    fg: Color,
    /// Cell background color.
    bg: Color,
    /// Cell modifiers.
    modifier: Modifier,
}

impl CellAnsiStyle {
    /// Build a style projection from one rendered Ratatui cell.
    fn from_cell(cell: &ratatui::buffer::Cell) -> Self {
        Self {
            fg: themed_color(cell.fg),
            bg: themed_color(cell.bg),
            modifier: cell.modifier,
        }
    }

    /// Convert the style to ANSI Select Graphic Rendition escapes.
    fn to_ansi(self) -> String {
        let mut codes = Vec::new();
        if self.modifier.contains(Modifier::BOLD) {
            codes.push("1".to_string());
        }
        if self.modifier.contains(Modifier::ITALIC) {
            codes.push("3".to_string());
        }
        if self.modifier.contains(Modifier::UNDERLINED) {
            codes.push("4".to_string());
        }
        if let Some(code) = color_to_ansi(self.fg, false) {
            codes.push(code);
        }
        if let Some(code) = color_to_ansi(self.bg, true) {
            codes.push(code);
        }
        if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes.join(";"))
        }
    }
}

/// Convert one Ratatui color into foreground/background ANSI code.
fn color_to_ansi(color: Color, background: bool) -> Option<String> {
    let offset = if background { 10 } else { 0 };
    let code = match color {
        Color::Reset => return None,
        Color::Black => 30 + offset,
        Color::Red => 31 + offset,
        Color::Green => 32 + offset,
        Color::Yellow => 33 + offset,
        Color::Blue => 34 + offset,
        Color::Magenta => 35 + offset,
        Color::Cyan => 36 + offset,
        Color::Gray | Color::White => 37 + offset,
        Color::DarkGray => 90 + offset,
        Color::LightRed => 91 + offset,
        Color::LightGreen => 92 + offset,
        Color::LightYellow => 93 + offset,
        Color::LightBlue => 94 + offset,
        Color::LightMagenta => 95 + offset,
        Color::LightCyan => 96 + offset,
        Color::Rgb(red, green, blue) => {
            let prefix = if background { 48 } else { 38 };
            return Some(format!("{prefix};2;{red};{green};{blue}"));
        }
        Color::Indexed(index) => {
            let prefix = if background { 48 } else { 38 };
            return Some(format!("{prefix};5;{index}"));
        }
    };
    Some(code.to_string())
}

/// Remap the dark reference palette to the selected output palette.
fn themed_color(color: Color) -> Color {
    match active_token_theme() {
        TokenDashboardTheme::Dark => color,
        TokenDashboardTheme::Light => remap_to_light_theme(color),
        TokenDashboardTheme::Terminal => match color {
            THEME_BG | THEME_PANEL => Color::Reset,
            _ => color,
        },
    }
}

/// Convert one dark semantic role color into its light-theme counterpart.
fn remap_to_light_theme(color: Color) -> Color {
    match color {
        THEME_BG => LIGHT_THEME.bg,
        THEME_PANEL => LIGHT_THEME.panel,
        THEME_TEXT => LIGHT_THEME.text,
        THEME_MUTED => LIGHT_THEME.muted,
        THEME_INK_WHITE => LIGHT_THEME.ink_white,
        THEME_BLUE => LIGHT_THEME.blue,
        THEME_GREEN => LIGHT_THEME.green,
        THEME_YELLOW => LIGHT_THEME.yellow,
        THEME_BORDER => LIGHT_THEME.border,
        THEME_BAR_EMPTY => LIGHT_THEME.bar_empty,
        THEME_RED => LIGHT_THEME.red,
        _ => color,
    }
}

/// Styled field label span.
fn label(text: &str) -> Span<'static> {
    Span::styled(format!("{text}: "), muted_bold_style())
}

/// Styled unsigned value span.
fn value(value: usize) -> Span<'static> {
    Span::styled(grouped_count(value), identity_style())
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
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok());
    let terminal_width = ratatui::crossterm::terminal::size()
        .ok()
        .map(|(width, _)| width);
    resolve_dashboard_width(columns, terminal_width)
}

/// Resolve an explicit dashboard width before using the detected terminal width.
fn resolve_dashboard_width(columns: Option<usize>, terminal_width: Option<u16>) -> usize {
    columns
        .or_else(|| terminal_width.map(usize::from))
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
        ATLAS_CANVAS_X_BOUND, ATLAS_CANVAS_Y_BOUND, ATLAS_PREVIEW_MAX_EDGES,
        ATLAS_PREVIEW_MAX_NODE_DEGREE, ATLAS_PREVIEW_MAX_NODES, DASHBOARD_HEIGHT, THEME_BAR_EMPTY,
        THEME_BG, THEME_BLUE, THEME_GREEN, THEME_INK_WHITE, THEME_YELLOW,
        TOKEN_IMPACT_COLUMN_WIDTH, TokenAtlasPreview, TokenDashboardTheme, atlas_layout, block_bar,
        buffer_to_string, dashboard_width, grouped_count, reconciled_without_projectatlas,
        reference_title, render_dashboard_to_string, render_overview_frame,
        render_overview_frame_with_atlas, render_token_dashboard,
        render_token_dashboard_with_theme, render_token_trend_dashboard,
        render_token_trend_dashboard_with_theme, resolve_dashboard_width,
        savings_source_rows_for_width, signed_count, signed_trend_points, signed_y_bounds,
    };
    use projectatlas_core::graph::GraphRelationKind;
    use projectatlas_core::symbols::RelationKind;
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
    use std::collections::BTreeMap;

    #[test]
    fn dashboard_width_prefers_override_then_terminal_then_fallback() {
        assert_eq!(resolve_dashboard_width(Some(200), Some(190)), 200);
        assert_eq!(resolve_dashboard_width(None, Some(190)), 190);
        assert_eq!(resolve_dashboard_width(None, None), 140);
    }

    #[test]
    fn overview_dashboard_matches_reference_sections_and_order() {
        let overview = sample_overview();
        let dashboard = strip_ansi(&render_token_dashboard(&overview, Some("s")));

        for text in [
            "ProjectAtlas",
            "Token Impact",
            "Smarter context. Fewer tokens. Real savings.",
            "Session:",
            "Lookups:",
            "Estimate:",
            "tokens avoided",
            "Without ProjectAtlas",
            "With ProjectAtlas",
            "Saved by ProjectAtlas",
            "file-read calls avoided",
            "Observed (summaries/slices)",
            "Search-modeled narrowing",
            "Confidence",
            "Measured from summaries/slices",
            "Navigation narrowing",
            "Impact scope:",
            "tokens + file-read calls",
            "Estimate type: local model",
            "Tokenizer audit:",
            "Source",
            "Steps",
            "Tokens Avoided",
            "What it means",
            "Summaries and slices",
            "Skipped broad folder walk",
            "Opened fewer candidates (A)",
            "Opened fewer candidates (B)",
            "Snapshot",
            "rerun command to refresh",
        ] {
            assert!(
                dashboard.contains(text),
                "dashboard should contain {text:?}"
            );
        }
        for title in [
            "TOTAL TOKENS AVOIDED",
            "LIKELY FILE-READ CALLS AVOIDED",
            "SAVINGS COMPOSITION",
            "SIGNAL",
            "WHERE THE SAVINGS CAME FROM",
            "CALIBRATION & NOTES",
        ] {
            assert!(dashboard.contains(&reference_title(title)));
        }

        assert!(!dashboard.contains("q  Quit"));
        assert!(!dashboard.contains("?  Help"));
        assert!(!dashboard.contains("r  Refresh"));
        assert!(!dashboard.contains("Auto"));
        assert!(!dashboard.contains(&reference_title("REPEATED-WORK BENCHMARK")));
        assert!(!dashboard.contains("ProjectAtlas Savings Overview"));
        assert!(!dashboard.contains("Saved-token trends"));
        assert!(!dashboard.contains("Calibration optional"));
        assert!(!dashboard.contains("--tokenizer o200k_base"));
        assert!(!dashboard.contains("day trend"));
        assert!(!dashboard.contains("week trend"));
        assert!(!dashboard.contains("month trend"));
        assert!(!dashboard.contains("year trend"));
        assert!(dashboard_contains_time(&dashboard));

        assert_in_order(
            &dashboard,
            &[
                "ProjectAtlas",
                &reference_title("TOTAL TOKENS AVOIDED"),
                &reference_title("LIKELY FILE-READ CALLS AVOIDED"),
                &reference_title("SAVINGS COMPOSITION"),
                &reference_title("WHERE THE SAVINGS CAME FROM"),
                &reference_title("CALIBRATION & NOTES"),
            ],
        );
        assert_header_margin(&dashboard, "Source", "Summaries and slices");
    }

    #[test]
    fn overview_dashboard_light_theme_remaps_semantic_palette() {
        let overview = sample_overview();
        let dashboard =
            render_token_dashboard_with_theme(&overview, Some("s"), TokenDashboardTheme::Light);

        assert!(dashboard.contains("\x1b["));
        assert!(
            dashboard.contains("48;2;246;242;232"),
            "light theme should use the light panel background"
        );
        assert!(
            dashboard.contains("38;2;37;99;235"),
            "baseline blue should be remapped for light terminals"
        );
        assert!(
            dashboard.contains("38;2;22;128;72"),
            "saved green should be remapped for light terminals"
        );
        assert!(
            dashboard.contains("38;2;178;116;0"),
            "modeled yellow should be remapped for light terminals"
        );
        assert!(
            !dashboard.contains("48;2;5;16;25"),
            "light theme should not serialize the dark panel background"
        );
    }

    #[test]
    fn trend_dashboard_light_theme_remaps_semantic_palette() {
        let report = sample_trend_report();
        let dashboard =
            render_token_trend_dashboard_with_theme(&report, TokenDashboardTheme::Light);

        assert!(dashboard.contains("\x1b["));
        assert!(
            dashboard.contains("48;2;246;242;232"),
            "light trend theme should use the light panel background"
        );
        assert!(
            dashboard.contains("38;2;22;128;72"),
            "positive trend line should use the light saved green"
        );
        assert!(
            dashboard.contains("38;2;22;22;20"),
            "ProjectAtlas trend title should use the light identity color"
        );
        assert!(
            !dashboard.contains("38;5;14") && !dashboard.contains("38;5;6"),
            "trend theme should not serialize hard-coded cyan"
        );
        assert!(
            !dashboard.contains("48;2;5;16;25"),
            "light trend theme should not serialize the dark panel background"
        );
    }

    #[test]
    fn overview_dashboard_uses_reference_ratatui_styles() {
        let overview = sample_overview();
        let buffer = render_overview_buffer(&overview, Some("s"));

        let Some((title_x, title_y)) = find_text(&buffer, "ProjectAtlas") else {
            unreachable!("ProjectAtlas title should render");
        };
        assert!(
            title_x <= 4,
            "title should start at the left of the header; title started at x={title_x}"
        );
        assert!(
            title_y <= 4,
            "title should stay in the upper header band; title started at y={title_y}"
        );
        assert_cell_style(&buffer, "ProjectAtlas", THEME_INK_WHITE, Modifier::BOLD);
        assert_cell_style(&buffer, "Token Impact", THEME_BLUE, Modifier::BOLD);
        assert_cell_style(
            &buffer,
            &reference_title("TOTAL TOKENS AVOIDED"),
            super::THEME_TEXT,
            Modifier::BOLD,
        );
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
        assert_cell_style(&buffer, "Tokens Avoided", super::THEME_TEXT, Modifier::BOLD);
        assert_cell_style(
            &buffer,
            "Search-modeled narrowing",
            THEME_YELLOW,
            Modifier::empty(),
        );
    }

    #[test]
    fn dashboards_preserve_terminal_background_outside_panels() {
        let overview = sample_overview();
        let overview_buffer = render_overview_buffer(&overview, Some("s"));
        assert_no_terminal_canvas_fill(&overview_buffer);
        assert_eq!(
            overview_buffer.cell((0, 0)).map(|cell| cell.bg),
            Some(Color::Reset),
            "outer overview border must not force a dashboard background color"
        );

        let overview_dark =
            render_token_dashboard_with_theme(&overview, Some("s"), TokenDashboardTheme::Dark);
        assert!(
            !overview_dark.contains("48;2;4;10;18"),
            "dark overview output must not paint the terminal canvas"
        );

        let overview_light =
            render_token_dashboard_with_theme(&overview, Some("s"), TokenDashboardTheme::Light);
        assert!(
            !overview_light.contains("48;2;252;249;241"),
            "light overview output must not paint the terminal canvas"
        );
        let overview_terminal =
            render_token_dashboard_with_theme(&overview, Some("s"), TokenDashboardTheme::Terminal);
        assert!(
            !overview_terminal.contains("48;2;5;16;25"),
            "terminal overview theme must preserve the terminal background inside panels"
        );

        let report = sample_trend_report();
        let trend_buffer = render_trend_buffer(&report);
        assert_no_terminal_canvas_fill(&trend_buffer);
        assert_eq!(
            trend_buffer.cell((0, 0)).map(|cell| cell.bg),
            Some(Color::Reset),
            "outer trend border must not force a dashboard background color"
        );

        let trend_dark =
            render_token_trend_dashboard_with_theme(&report, TokenDashboardTheme::Dark);
        assert!(
            !trend_dark.contains("48;2;4;10;18"),
            "dark trend output must not paint the terminal canvas"
        );

        let trend_light =
            render_token_trend_dashboard_with_theme(&report, TokenDashboardTheme::Light);
        assert!(
            !trend_light.contains("48;2;252;249;241"),
            "light trend output must not paint the terminal canvas"
        );
        let trend_terminal =
            render_token_trend_dashboard_with_theme(&report, TokenDashboardTheme::Terminal);
        assert!(
            !trend_terminal.contains("48;2;5;16;25"),
            "terminal trend theme must preserve the terminal background inside panels"
        );
    }

    #[test]
    fn overview_dashboard_hero_value_is_readable_terminal_text() {
        let overview = TokenOverview::from_estimated_totals(3, 241_563_877, 4_749_368);
        let narrow_buffer = render_overview_buffer_at_width(&overview, Some("s"), 100);
        let Some((_, narrow_title_y)) =
            find_text(&narrow_buffer, &reference_title("TOTAL TOKENS AVOIDED"))
        else {
            unreachable!("hero title should render");
        };
        let narrow_value_line = line_symbols(&narrow_buffer, narrow_title_y + 1);

        assert!(
            narrow_value_line.contains(&signed_count(overview.tokens_avoided)),
            "narrow hero value should fall back to the exact saved-token number as normal terminal text"
        );
        assert!(
            narrow_value_line.contains('✓'),
            "narrow hero value should keep the reference-style saved-state marker"
        );

        let buffer = render_overview_buffer_at_width(&overview, Some("s"), 140);
        let Some((_, title_y)) = find_text(&buffer, &reference_title("TOTAL TOKENS AVOIDED"))
        else {
            unreachable!("hero title should render");
        };
        let hero_rows = ((title_y + 1)..=(title_y + 2).min(buffer.area.height.saturating_sub(1)))
            .map(|y| line_symbols(&buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            hero_rows.contains(&signed_count(overview.tokens_avoided)),
            "wide hero value should render the exact saved-token number as normal terminal text"
        );
        assert!(
            hero_rows.contains('✓'),
            "wide hero should draw the saved-state marker beside the readable total"
        );
        assert!(
            !hero_rows.chars().any(|character| {
                ('\u{1cc00}'..='\u{1cfff}').contains(&character)
                    || ('\u{1fb00}'..='\u{1fbff}').contains(&character)
            }),
            "wide hero value should avoid dense segmented glyphs that render inconsistently across terminals"
        );
        let caption_line = line_symbols(&buffer, title_y + 3);
        assert!(caption_line.contains("tokens avoided"));
        assert!(
            !caption_line.contains(&signed_count(overview.tokens_avoided)),
            "caption should label the hero without duplicating the numeric value"
        );

        let dashboard = render_dashboard_to_string(140, DASHBOARD_HEIGHT, |frame| {
            render_overview_frame(frame, &overview, Some("s"));
        });
        assert!(dashboard.contains("tokens avoided"));
        assert!(
            !dashboard.contains(&format!(
                "{} tokens avoided",
                signed_count(overview.tokens_avoided)
            )),
            "caption should not duplicate the saved-token number already shown as the hero and saved operand"
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
        assert!(dashboard.contains(&reference_title("TOTAL TOKENS AVOIDED")));
        assert!(dashboard.contains(&reference_title("LIKELY FILE-READ CALLS AVOIDED")));
        assert!(dashboard.contains(&reference_title("WHERE THE SAVINGS CAME FROM")));
        assert!(dashboard.contains("Fewer candidates"));
        assert!(dashboard.contains(&reference_title("CALIBRATION & NOTES")));
        assert!(!dashboard.contains(&reference_title("REPEATED-WORK BENCHMARK")));
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

        let dashboard = strip_ansi(&render_token_dashboard(&overview, Some("s")));
        let source_rows = savings_source_rows_for_width(&overview, false);
        let source_steps = source_rows.iter().map(|row| row.steps).sum::<usize>();
        let source_tokens = source_rows.iter().map(|row| row.tokens).sum::<isize>();

        assert_eq!(source_steps, overview.calls);
        assert_eq!(source_tokens, conservative_avoided);
        assert!(dashboard.contains(&signed_count(without_projectatlas)));
        assert!(dashboard.contains(&signed_count(with_projectatlas)));
        assert!(dashboard.contains(&signed_count(conservative_avoided)));
        assert_eq!(
            dashboard
                .matches(&signed_count(conservative_avoided))
                .count(),
            2,
            "wide dashboard should show the saved total as readable hero text and as the equation result"
        );
        assert!(dashboard.contains(&grouped_count(overview.likely_file_reads_avoided)));
        assert!(dashboard.contains(&grouped_count(overview.observed_file_read_replacements)));
        assert!(dashboard.contains(&grouped_count(overview.modeled_file_reads_avoided)));
    }

    #[test]
    fn overview_dashboard_source_table_reconciles_unattributed_remainder() {
        let mut overview = sample_overview();
        overview.buckets.clear();
        overview.calls = 7;
        overview.measured_tokens_saved = 11;
        overview.deduped_modeled_tokens_avoided = 29;
        overview.tokens_avoided = 40;

        let rows = savings_source_rows_for_width(&overview, false);
        let source_steps = rows.iter().map(|row| row.steps).sum::<usize>();
        let source_tokens = rows.iter().map(|row| row.tokens).sum::<isize>();

        assert_eq!(source_steps, overview.calls);
        assert_eq!(source_tokens, overview.tokens_avoided);
        assert!(rows.iter().any(|row| row.label == "Unattributed savings"));

        let dashboard = strip_ansi(&render_token_dashboard(&overview, Some("s")));
        assert!(dashboard.contains("Unattributed savings"));
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

        let dashboard = strip_ansi(&render_token_dashboard(&overview, Some("s")));
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
        assert_eq!(line_text(&partial), "█████░░░░░");

        let clamped = block_bar(10, 2.0, THEME_YELLOW);
        assert_bar_segments(&clamped, 10, 0, THEME_YELLOW);

        let empty = block_bar(10, -1.0, THEME_BLUE);
        assert_bar_segments(&empty, 0, 10, THEME_BLUE);
    }

    #[test]
    fn atlas_preview_is_bounded_and_centers_the_strongest_hub() {
        let kinds = [
            GraphRelationKind::Legacy(RelationKind::Contains),
            GraphRelationKind::Legacy(RelationKind::Imports),
            GraphRelationKind::Legacy(RelationKind::Calls),
            GraphRelationKind::Legacy(RelationKind::DependsOn),
        ];
        let mut relations = Vec::new();
        for (kind_index, kind) in kinds.into_iter().enumerate() {
            let base = kind_index * 12;
            for source in 0..12 {
                for distance in 1..=3 {
                    relations.push((
                        format!("node-{:02}", base + source),
                        format!("node-{:02}", base + (source + distance) % 12),
                        kind,
                    ));
                }
            }
        }

        let atlas = TokenAtlasPreview::from_resolved_edges(relations, false);
        let layout = atlas_layout(&atlas.edges);

        assert!(atlas.available);
        assert!(atlas.truncated);
        assert!(atlas.node_count() <= ATLAS_PREVIEW_MAX_NODES);
        assert_eq!(atlas.edges.len(), ATLAS_PREVIEW_MAX_EDGES);
        assert_eq!(layout.positions.get(&layout.hub), Some(&(0.0, 0.0)));
        let mut selected_degrees = BTreeMap::<&str, usize>::new();
        for edge in &atlas.edges {
            *selected_degrees.entry(&edge.source).or_default() += 1;
            *selected_degrees.entry(&edge.target).or_default() += 1;
        }
        assert!(
            selected_degrees
                .values()
                .all(|degree| *degree <= ATLAS_PREVIEW_MAX_NODE_DEGREE)
        );
        assert!(
            layout
                .positions
                .values()
                .all(|(x, y)| x.abs() < ATLAS_CANVAS_X_BOUND && y.abs() < ATLAS_CANVAS_Y_BOUND)
        );
    }

    #[test]
    fn wide_atlas_map_renders_real_counts_and_stays_inside_its_panel() {
        let kind = GraphRelationKind::Legacy(RelationKind::Calls);
        let mut relations = (0..6)
            .map(|index| ("hub".to_string(), format!("node-{index}"), kind))
            .collect::<Vec<_>>();
        relations.extend([
            ("satellite-a".to_string(), "satellite-b".to_string(), kind),
            ("satellite-b".to_string(), "satellite-c".to_string(), kind),
        ]);
        let atlas = TokenAtlasPreview::from_resolved_edges(relations, false);
        let buffer =
            render_overview_buffer_with_atlas_at_width(&sample_overview(), Some("s"), &atlas, 200);
        let dashboard = buffer_to_string(&buffer);

        assert!(dashboard.contains(&reference_title("ATLAS MAP")));
        assert!(dashboard.contains("10 nodes • 8 links"));
        assert!(dashboard.contains("bounded live graph • static snapshot"));
        assert!(buffer_contains_braille(
            &buffer,
            TOKEN_IMPACT_COLUMN_WIDTH + 2
        ));
        assert!(!dashboard.contains("Frozen v0.3.26"));
        assert!(!dashboard.contains("Plain Codex"));
        assert!(!dashboard.contains(&reference_title("REPEATED-WORK BENCHMARK")));
        for y in 2..(DASHBOARD_HEIGHT - 2) {
            assert_eq!(
                buffer.cell((198, y)).map(ratatui::buffer::Cell::symbol),
                Some("│"),
                "atlas panel right border should remain intact at row {y}"
            );
        }
    }

    #[test]
    fn atlas_map_hides_when_narrow_and_never_invents_empty_state_data() {
        let kind = GraphRelationKind::Legacy(RelationKind::Calls);
        let atlas = TokenAtlasPreview::from_resolved_edges(
            [("source".to_string(), "target".to_string(), kind)],
            false,
        );
        let narrow =
            render_overview_buffer_with_atlas_at_width(&sample_overview(), Some("s"), &atlas, 189);
        assert!(!buffer_to_string(&narrow).contains(&reference_title("ATLAS MAP")));

        for (atlas, message) in [
            (TokenAtlasPreview::empty(), "No resolved graph links"),
            (
                TokenAtlasPreview::unavailable(),
                "Graph preview unavailable",
            ),
        ] {
            let buffer = render_overview_buffer_with_atlas_at_width(
                &sample_overview(),
                Some("s"),
                &atlas,
                200,
            );
            let dashboard = buffer_to_string(&buffer);
            assert!(dashboard.contains(message));
            assert!(!dashboard.contains("nodes •"));
            assert!(!buffer_contains_braille(
                &buffer,
                TOKEN_IMPACT_COLUMN_WIDTH + 2
            ));
        }
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

        let dashboard = strip_ansi(&render_token_dashboard(&overview, Some("s")));
        assert!(dashboard.contains(&format!(
            "Signed mix: observed {} / modeled {}; net {}",
            signed_count(overview.measured_tokens_saved),
            signed_count(overview.deduped_modeled_tokens_avoided),
            signed_count(overview.tokens_avoided)
        )));
        assert!(!dashboard.contains("% / modeled"));
        let wide_buffer = render_overview_buffer_at_width(&overview, Some("s"), 140);
        let Some((_, wide_title_y)) =
            find_text(&wide_buffer, &reference_title("TOTAL TOKENS AVOIDED"))
        else {
            unreachable!("hero title should render");
        };
        let wide_hero_rows = ((wide_title_y + 1)
            ..=(wide_title_y + 4).min(wide_buffer.area.height.saturating_sub(1)))
            .map(|y| line_symbols(&wide_buffer, y))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            wide_hero_rows.contains('!'),
            "wide negative hero should use a warning marker instead of a success check"
        );
        assert!(
            !wide_hero_rows.contains('✓'),
            "wide negative hero must not imply success with a check marker"
        );

        let narrow_buffer = render_overview_buffer_at_width(&overview, Some("s"), 100);
        let Some((_, narrow_title_y)) =
            find_text(&narrow_buffer, &reference_title("TOTAL TOKENS AVOIDED"))
        else {
            unreachable!("hero title should render");
        };
        let narrow_value_line = line_symbols(&narrow_buffer, narrow_title_y + 1);
        assert!(
            narrow_value_line.contains('!'),
            "narrow negative hero should use a warning marker instead of a success check"
        );
        assert!(
            !narrow_value_line.contains('✓'),
            "narrow negative hero must not imply success with a check marker"
        );

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
        let report = sample_trend_report();
        let dashboard = strip_ansi(&render_token_trend_dashboard(&report));

        assert!(dashboard.contains("ProjectAtlas Token Trends"));
        assert!(dashboard.contains(&reference_title("SAVED TOKENS TREND")));
        assert!(dashboard.contains("2026-06"));
        assert!(dashboard.contains("2026-07"));
        assert!(dashboard.contains("period"));
        assert!(dashboard_contains_chart_glyph(&dashboard));
    }

    fn sample_trend_report() -> TokenTrendReport {
        TokenTrendReport::new(
            Some("s".to_string()),
            TokenTrendWindow::Month,
            vec![
                TokenTrendPeriod::from_totals("2026-06".to_string(), 2, 200, 50),
                TokenTrendPeriod::from_totals("2026-07".to_string(), 1, 100, 80),
            ],
        )
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
        render_overview_buffer_at_width(overview, session, width)
    }

    fn render_overview_buffer_at_width(
        overview: &TokenOverview,
        session: Option<&str>,
        width: u16,
    ) -> Buffer {
        let backend = TestBackend::new(width, DASHBOARD_HEIGHT);
        let mut terminal =
            Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
        let frame = terminal
            .draw(|frame| render_overview_frame(frame, overview, session))
            .expect("in-memory token dashboard should render");
        frame.buffer.clone()
    }

    fn render_overview_buffer_with_atlas_at_width(
        overview: &TokenOverview,
        session: Option<&str>,
        atlas: &TokenAtlasPreview,
        width: u16,
    ) -> Buffer {
        let backend = TestBackend::new(width, DASHBOARD_HEIGHT);
        let mut terminal =
            Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
        let frame = terminal
            .draw(|frame| render_overview_frame_with_atlas(frame, overview, session, Some(atlas)))
            .expect("in-memory token dashboard with atlas should render");
        frame.buffer.clone()
    }

    fn buffer_contains_braille(buffer: &Buffer, start_x: u16) -> bool {
        (0..buffer.area.height).any(|y| {
            (start_x..buffer.area.width).any(|x| {
                buffer.cell((x, y)).is_some_and(|cell| {
                    cell.symbol()
                        .chars()
                        .any(|character| ('\u{2801}'..='\u{28ff}').contains(&character))
                })
            })
        })
    }

    fn render_trend_buffer(report: &TokenTrendReport) -> Buffer {
        let width = dashboard_width().clamp(80, 140) as u16;
        let backend = TestBackend::new(width, super::TREND_DASHBOARD_HEIGHT);
        let mut terminal =
            Terminal::new(backend).expect("in-memory token dashboard backend should initialize");
        let frame = terminal
            .draw(|frame| super::render_trend_frame(frame, report))
            .expect("in-memory token dashboard should render");
        frame.buffer.clone()
    }

    fn line_symbols(buffer: &Buffer, y: u16) -> String {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        line
    }

    fn assert_no_terminal_canvas_fill(buffer: &Buffer) {
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let Some(cell) = buffer.cell((x, y)) else {
                    continue;
                };
                assert_ne!(
                    cell.bg, THEME_BG,
                    "dashboard should not force the terminal canvas background at ({x},{y})"
                );
            }
        }
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
                .all(|character| character == '█')
        );
        assert!(
            line.spans[1]
                .content
                .chars()
                .all(|character| character == '░')
        );
        assert_eq!(line.spans[0].style.fg, Some(color));
        assert_eq!(line.spans[1].style.fg, Some(THEME_BAR_EMPTY));
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
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

    fn strip_ansi(input: &str) -> String {
        let mut output = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\u{1b}' && chars.peek() == Some(&'[') {
                chars.next();
                for code in chars.by_ref() {
                    if code.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                output.push(character);
            }
        }
        output
    }

    fn find_text(buffer: &Buffer, text: &str) -> Option<(u16, u16)> {
        assert!(
            text.is_ascii(),
            "use direct cell assertions for non-ASCII symbols"
        );
        for y in 0..buffer.area.height {
            let mut cells = Vec::new();
            let mut line = String::new();
            for x in 0..buffer.area.width {
                let symbol = buffer.cell((x, y))?.symbol();
                if symbol.is_ascii() {
                    line.push_str(symbol);
                } else {
                    line.push(' ');
                }
                cells.push((x, y));
            }
            if let Some(index) = line.find(text) {
                return cells.get(index).copied();
            }
        }
        None
    }
}
