use chrono::Utc;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph},
};

use super::chart::{build_chart_data, make_ratatui_chart};
use crate::calculator::{calculate_budget_pace, calculate_burn_metrics};
use crate::formatting::{format_duration, format_reset_time, pace_icon, utilization_color};
use crate::models::{AnyLimit, LimitType, UsageSnapshot};

pub struct AppState<'a> {
    pub limit_type: LimitType,
    pub limit: Option<AnyLimit>,
    pub snapshots: &'a [UsageSnapshot],
    pub error: Option<&'a str>,
    pub stale_since: Option<chrono::DateTime<Utc>>,
}

pub fn render(f: &mut Frame, state: &AppState) {
    let area = f.area();

    // Determine layout sections
    let has_banner = state.error.is_some() || state.stale_since.is_some();
    let constraints = if has_banner {
        vec![
            Constraint::Length(1), // header
            Constraint::Length(2), // gauges (2 bars)
            Constraint::Length(1), // banner
            Constraint::Min(0),    // chart
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(0),
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    // Header
    render_header(f, chunks[0], state);
    // Gauges
    render_gauges(f, chunks[1], state);
    // Optional banner
    let chart_chunk = if has_banner {
        render_banner(f, chunks[2], state);
        chunks[3]
    } else {
        chunks[2]
    };
    // Chart
    render_chart(f, chart_chunk, state);
}

fn render_header(f: &mut Frame, area: Rect, state: &AppState) {
    let icon = if let Some(limit) = &state.limit {
        let p = calculate_budget_pace(limit.resets_at(), limit.window_hours());
        pace_icon(limit.effective_utilization(), p)
    } else {
        "🔥"
    };

    let reset_str = state
        .limit
        .as_ref()
        .map(|l| format_reset_time(l.resets_at()))
        .unwrap_or_else(|| "Loading...".to_string());

    let left = Line::from(vec![
        Span::raw(format!("{icon} ")),
        Span::styled(
            "tokenburn",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" - ", Style::default().add_modifier(Modifier::DIM)),
        Span::styled(
            state.limit_type.display_name(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let right = Line::from(vec![
        Span::raw("⏰ "),
        Span::styled(reset_str, Style::default().fg(Color::Yellow)),
    ]);

    // Use a two-column layout for left/right alignment
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(30)])
        .split(area);

    f.render_widget(Paragraph::new(left), cols[0]);
    f.render_widget(
        Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
        cols[1],
    );
}

fn render_gauges(f: &mut Frame, area: Rect, state: &AppState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(area);

    let (util_pct, pace_pct, usage_color) = if let Some(limit) = &state.limit {
        let pace = calculate_budget_pace(limit.resets_at(), limit.window_hours());
        let util = limit.effective_utilization();
        let color = utilization_color(util, pace);
        ((util * 100.0) as u16, (pace * 100.0) as u16, color)
    } else {
        (0, 0, Color::Gray)
    };

    // Usage bar
    let usage_label = format!("📊 Usage  {:>3}%", util_pct);
    let usage_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(usage_color).bg(Color::DarkGray))
        .percent(util_pct.min(100))
        .label(usage_label);
    f.render_widget(usage_gauge, rows[0]);

    // Elapsed bar
    let elapsed_label = format!("⏳ Elapsed {:>3}%", pace_pct);
    let elapsed_gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .percent(pace_pct.min(100))
        .label(elapsed_label);
    f.render_widget(elapsed_gauge, rows[1]);
}

fn render_banner(f: &mut Frame, area: Rect, state: &AppState) {
    let text = if let Some(err) = state.error {
        Line::from(Span::styled(
            format!("⚠ {err}"),
            Style::default().fg(Color::Yellow),
        ))
    } else if let Some(stale) = state.stale_since {
        let minutes = (Utc::now() - stale).num_minutes().max(0);
        Line::from(Span::styled(
            format!(
                "Using cached data (last updated {} ago)",
                format_duration(minutes)
            ),
            Style::default().fg(Color::Yellow),
        ))
    } else {
        Line::default()
    };
    f.render_widget(Paragraph::new(text), area);
}

fn render_chart(f: &mut Frame, area: Rect, state: &AppState) {
    if area.height < 4 {
        let msg = Paragraph::new("Terminal too small for chart. Expand window.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, area);
        return;
    }

    let Some(limit) = &state.limit else {
        let msg = Paragraph::new("Loading data...").style(Style::default().fg(Color::DarkGray));
        f.render_widget(msg, area);
        return;
    };

    let metrics = calculate_burn_metrics(limit, state.snapshots);
    let data = build_chart_data(limit, state.snapshots, metrics.percent_per_hour);
    let chart = make_ratatui_chart(&data, area.height);
    f.render_widget(chart, area);
}
