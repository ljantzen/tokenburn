use chrono::{DateTime, Duration, Local, Utc};
use ratatui::{
    style::{Color, Modifier, Style},
    symbols,
    text::Span,
    widgets::{Axis, Chart, Dataset, GraphType},
};

use crate::models::{AnyLimit, UsageSnapshot};

pub struct ChartData {
    pub budget_pace_points: Vec<(f64, f64)>,
    pub usage_points: Vec<(f64, f64)>,
    pub projection_points: Vec<(f64, f64)>,
    pub now_line: Vec<(f64, f64)>,
    pub display_hours: f64,
    pub y_min: f64,
    pub y_max: f64,
    pub x_labels: Vec<(f64, String)>,
    pub usage_color: Color,
    pub proj_color: Color,
}

pub fn build_chart_data(
    limit: &AnyLimit,
    snapshots: &[UsageSnapshot],
    burn_rate: f64,
) -> ChartData {
    let now = Utc::now();
    let window_start = limit.window_start();
    let window_end = limit.resets_at();
    let window_hours = limit.window_hours() as f64;

    let display_start = window_start;
    let display_end = window_end;
    let display_hours = (display_end - display_start).num_seconds() as f64 / 3600.0;

    let to_hours =
        |dt: DateTime<Utc>| -> f64 { (dt - display_start).num_seconds() as f64 / 3600.0 };

    // Budget pace line (50 points)
    let budget_pace_points: Vec<(f64, f64)> = (0..50)
        .map(|i| {
            let x = i as f64 * display_hours / 49.0;
            let point_time = display_start + Duration::seconds((x * 3600.0) as i64);
            let elapsed = (point_time - window_start).num_seconds() as f64 / 3600.0;
            let y = (elapsed / window_hours * 100.0).min(100.0);
            (x, y)
        })
        .collect();

    // Actual usage points
    let limit_type = limit.limit_type();
    let usage_points: Vec<(f64, f64)> = snapshots
        .iter()
        .filter(|s| s.timestamp >= display_start && s.timestamp <= now)
        .filter_map(|s| {
            let l = s.get_limit(limit_type)?;
            let u = l.utilization();
            if u > 1.0 {
                return None;
            }
            Some((to_hours(s.timestamp), u * 100.0))
        })
        .collect();

    // Projection
    let current_pct = limit.effective_utilization() * 100.0;
    let now_hours = to_hours(now);
    let remaining_window_hours = (display_end - now).num_seconds() as f64 / 3600.0;

    let (projection_points, proj_color) = if burn_rate > 0.0 && current_pct < 100.0 {
        let hours_to_100 = (100.0 - current_pct) / burn_rate;
        if hours_to_100 <= remaining_window_hours {
            let end_hours = now_hours + hours_to_100;
            (
                vec![(now_hours, current_pct), (end_hours, 100.0)],
                Color::LightRed,
            )
        } else {
            let end_hours = now_hours + remaining_window_hours;
            let end_pct = (current_pct + burn_rate * remaining_window_hours).min(100.0);
            (
                vec![(now_hours, current_pct), (end_hours, end_pct)],
                Color::LightGreen,
            )
        }
    } else {
        (vec![], Color::Gray)
    };

    // "Now" vertical line
    let now_line: Vec<(f64, f64)> = if now_hours > 0.0 && now_hours < display_hours {
        (0..=20).map(|i| (now_hours, i as f64 * 5.0)).collect()
    } else {
        vec![]
    };

    // Determine Y range
    let all_y: Vec<f64> = budget_pace_points
        .iter()
        .map(|(_, y)| *y)
        .chain(usage_points.iter().map(|(_, y)| *y))
        .chain(projection_points.iter().map(|(_, y)| *y))
        .collect();
    let (y_min, y_max) = if all_y.is_empty() {
        (0.0_f64, 100.0_f64)
    } else {
        let data_min = all_y.iter().cloned().fold(f64::INFINITY, f64::min);
        let data_max = all_y.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let pad = ((data_max - data_min) * 0.1).max(1.0);
        let ymin = (data_min - pad).max(0.0);
        let ymax = (data_max + pad).min(100.0);
        let (ymin, ymax) = if ymax - ymin < 10.0 {
            let mid = (ymin + ymax) / 2.0;
            ((mid - 5.0).max(0.0), (mid + 5.0).min(100.0))
        } else {
            (ymin, ymax)
        };
        (ymin, ymax)
    };

    // X axis labels
    let num_ticks = 5usize;
    let x_labels: Vec<(f64, String)> = (0..num_ticks)
        .map(|i| {
            let hours = i as f64 * display_hours / (num_ticks - 1) as f64;
            let tick_time = display_start + Duration::seconds((hours * 3600.0) as i64);
            let local: DateTime<Local> = tick_time.into();
            let label = if display_hours > 168.0 {
                local.format("%-m/%-d").to_string()
            } else if display_hours > 24.0 {
                local.format("%a %Hh").to_string()
            } else {
                local.format("%H:%M").to_string()
            };
            (hours, label)
        })
        .collect();

    let usage_color = crate::formatting::utilization_color(
        limit.effective_utilization(),
        (now_hours / display_hours).clamp(0.0, 1.0),
    );

    ChartData {
        budget_pace_points,
        usage_points,
        projection_points,
        now_line,
        display_hours,
        y_min,
        y_max,
        x_labels,
        usage_color,
        proj_color,
    }
}

pub fn make_ratatui_chart<'a>(data: &'a ChartData, _height: u16) -> Chart<'a> {
    let mut datasets = vec![
        Dataset::default()
            .name("Budget Pace")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::DarkGray))
            .data(&data.budget_pace_points),
        Dataset::default()
            .name("Usage")
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(data.usage_color))
            .data(&data.usage_points),
    ];

    if !data.projection_points.is_empty() {
        datasets.push(
            Dataset::default()
                .name("Projection")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Line)
                .style(
                    Style::default()
                        .fg(data.proj_color)
                        .add_modifier(Modifier::DIM),
                )
                .data(&data.projection_points),
        );
    }

    if !data.now_line.is_empty() {
        datasets.push(
            Dataset::default()
                .name("Now")
                .marker(symbols::Marker::Braille)
                .graph_type(GraphType::Scatter)
                .style(Style::default().fg(Color::Blue))
                .data(&data.now_line),
        );
    }

    let x_bounds = [0.0, data.display_hours];
    let y_bounds = [data.y_min, data.y_max];

    let x_axis_labels: Vec<Span> = data
        .x_labels
        .iter()
        .map(|(_, label)| Span::raw(label.clone()))
        .collect();

    let y_labels = vec![
        Span::raw(format!("{:.0}%", data.y_min)),
        Span::raw(format!("{:.0}%", (data.y_min + data.y_max) / 2.0)),
        Span::raw(format!("{:.0}%", data.y_max)),
    ];

    Chart::new(datasets)
        .x_axis(
            Axis::default()
                .bounds(x_bounds)
                .labels(x_axis_labels)
                .style(Style::default().fg(Color::DarkGray)),
        )
        .y_axis(
            Axis::default()
                .bounds(y_bounds)
                .labels(y_labels)
                .style(Style::default().fg(Color::DarkGray)),
        )
}
