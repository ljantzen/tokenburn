use chrono::{DateTime, Local, Utc};

pub fn format_duration(minutes: i64) -> String {
    if minutes < 0 {
        return "0m".to_string();
    }
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours < 24 {
        if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        }
    } else {
        let days = hours / 24;
        let rem_h = hours % 24;
        if rem_h > 0 {
            format!("{days}d {rem_h}h")
        } else {
            format!("{days}d")
        }
    }
}

pub fn format_reset_time(resets_at: DateTime<Utc>) -> String {
    let now = Utc::now();
    let delta = resets_at - now;
    let total_minutes = delta.num_minutes();
    if total_minutes < 0 {
        return "Reset pending".to_string();
    }
    if total_minutes < 24 * 60 {
        return format!("Resets in {}", format_duration(total_minutes));
    }
    let local: DateTime<Local> = resets_at.into();
    let day = local.format("%a").to_string();
    let time_str = local
        .format("%I:%M %p")
        .to_string()
        .trim_start_matches('0')
        .to_string();
    if total_minutes >= 7 * 24 * 60 {
        let date = local.format("%-m/%-d").to_string();
        format!("Resets {day} {date} {time_str}")
    } else {
        format!("Resets {day} {time_str}")
    }
}

pub fn format_credits(dollars: f64) -> String {
    if dollars >= 100.0 {
        format!("${dollars:.0}")
    } else {
        format!("${dollars:.2}")
    }
}

pub fn utilization_color(utilization: f64, budget_pace: f64) -> ratatui::style::Color {
    use ratatui::style::Color;
    if utilization >= 0.9 {
        return Color::Red;
    }
    let burn_ratio = if budget_pace >= 0.05 && utilization >= 0.01 {
        utilization / budget_pace
    } else {
        1.0
    };

    if utilization >= 0.75 {
        if burn_ratio > 1.5 {
            Color::Red
        } else {
            Color::LightRed
        }
    } else if utilization >= 0.5 {
        if burn_ratio > 2.0 {
            Color::Red
        } else if burn_ratio > 1.5 {
            Color::LightRed
        } else {
            Color::Yellow
        }
    } else if burn_ratio > 3.0 {
        Color::LightRed
    } else if burn_ratio > 2.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

pub fn pace_icon(utilization: f64, budget_pace: f64) -> &'static str {
    if budget_pace == 0.0 {
        return "🔥";
    }
    let ratio = utilization / budget_pace;
    if ratio < 0.85 {
        "🧊"
    } else if ratio > 1.15 {
        "🚨"
    } else {
        "🔥"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    // --- format_duration ---

    #[test]
    fn duration_negative() {
        assert_eq!(format_duration(-5), "0m");
    }

    #[test]
    fn duration_zero() {
        assert_eq!(format_duration(0), "0m");
    }

    #[test]
    fn duration_minutes_only() {
        assert_eq!(format_duration(45), "45m");
        assert_eq!(format_duration(59), "59m");
    }

    #[test]
    fn duration_exact_hours() {
        assert_eq!(format_duration(60), "1h");
        assert_eq!(format_duration(120), "2h");
    }

    #[test]
    fn duration_hours_and_minutes() {
        assert_eq!(format_duration(90), "1h 30m");
        assert_eq!(format_duration(135), "2h 15m");
    }

    #[test]
    fn duration_exact_days() {
        assert_eq!(format_duration(24 * 60), "1d");
        assert_eq!(format_duration(7 * 24 * 60), "7d");
    }

    #[test]
    fn duration_days_and_hours() {
        assert_eq!(format_duration(25 * 60), "1d 1h");
        assert_eq!(format_duration(50 * 60), "2d 2h");
    }

    // --- format_credits ---

    #[test]
    fn credits_small() {
        assert_eq!(format_credits(0.0), "$0.00");
        assert_eq!(format_credits(74.75), "$74.75");
        assert_eq!(format_credits(99.99), "$99.99");
    }

    #[test]
    fn credits_hundreds() {
        assert_eq!(format_credits(100.0), "$100");
        assert_eq!(format_credits(299.9), "$300");
    }

    #[test]
    fn credits_thousands() {
        // >= 100 branch rounds to integer
        assert_eq!(format_credits(1000.0), "$1000");
        assert_eq!(format_credits(1500.0), "$1500");
        assert_eq!(format_credits(9999.0), "$9999");
    }

    // --- pace_icon ---

    #[test]
    fn icon_zero_pace_is_fire() {
        assert_eq!(pace_icon(0.5, 0.0), "🔥");
    }

    #[test]
    fn icon_well_under_budget_is_ice() {
        // utilization 10%, pace 50% → ratio 0.2 → under 0.85
        assert_eq!(pace_icon(0.1, 0.5), "🧊");
    }

    #[test]
    fn icon_on_pace_is_fire() {
        // equal → ratio 1.0
        assert_eq!(pace_icon(0.5, 0.5), "🔥");
        // within 15%
        assert_eq!(pace_icon(0.55, 0.5), "🔥");
    }

    #[test]
    fn icon_over_budget_is_alarm() {
        // utilization 60%, pace 40% → ratio 1.5 → over 1.15
        assert_eq!(pace_icon(0.6, 0.4), "🚨");
    }

    // --- utilization_color ---

    #[test]
    fn color_critical_utilization() {
        assert_eq!(utilization_color(0.95, 0.5), Color::Red);
        assert_eq!(utilization_color(0.9, 0.0), Color::Red);
    }

    #[test]
    fn color_low_utilization_normal_pace() {
        assert_eq!(utilization_color(0.2, 0.2), Color::Green);
    }

    #[test]
    fn color_high_utilization_fast_burn() {
        // 80% utilized, only 40% elapsed → burn_ratio 2.0 → Red
        assert_eq!(utilization_color(0.8, 0.4), Color::Red);
    }

    #[test]
    fn color_moderate_utilization_on_pace() {
        // 60% utilized, 60% elapsed → ratio 1.0 → Yellow
        assert_eq!(utilization_color(0.6, 0.6), Color::Yellow);
    }
}
