use chrono::Utc;

use crate::calculator::calculate_budget_pace;
use crate::formatting::{format_credits, format_duration, pace_icon};
use crate::models::{LimitData, MonthlyLimitData};

pub fn render_compact(
    session: Option<&LimitData>,
    weekly: Option<&LimitData>,
    weekly_sonnet: Option<&LimitData>,
    monthly: Option<&MonthlyLimitData>,
) -> String {
    let mut parts = Vec::new();

    if let Some(s) = session {
        let pace = calculate_budget_pace(s.resets_at, s.window_hours());
        let icon = pace_icon(s.effective_utilization(), pace);
        let util = (s.effective_utilization() * 100.0) as u64;
        let minutes_left = (s.resets_at - Utc::now()).num_minutes().max(0);
        let time_str = if minutes_left > 0 {
            format!(" ({})", format_duration(minutes_left))
        } else {
            String::new()
        };
        parts.push(format!("Session: {icon} {util}%{time_str}"));
    }

    if let Some(w) = weekly {
        let pace = calculate_budget_pace(w.resets_at, w.window_hours());
        let icon = pace_icon(w.effective_utilization(), pace);
        let util = (w.effective_utilization() * 100.0) as u64;
        parts.push(format!("Weekly: {icon} {util}%"));
    }

    if let Some(ws) = weekly_sonnet {
        let pace = calculate_budget_pace(ws.resets_at, ws.window_hours());
        let icon = pace_icon(ws.effective_utilization(), pace);
        let util = (ws.effective_utilization() * 100.0) as u64;
        parts.push(format!("Sonnet: {icon} {util}%"));
    }

    if let Some(m) = monthly {
        let pace = calculate_budget_pace(m.resets_at, m.window_hours());
        let icon = pace_icon(m.effective_utilization(), pace);
        let dollars = format_credits(m.used_credits_dollars());
        parts.push(format!("Monthly: {icon} {dollars}"));
    }

    if parts.is_empty() {
        "No data available".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LimitData, LimitType, MonthlyLimitData};
    use chrono::{Duration, Utc};

    fn session(util: f64) -> LimitData {
        LimitData {
            utilization: util,
            resets_at: Utc::now() + Duration::hours(3),
            limit_type: LimitType::Session,
        }
    }

    fn weekly(util: f64) -> LimitData {
        LimitData {
            utilization: util,
            resets_at: Utc::now() + Duration::hours(100),
            limit_type: LimitType::Weekly,
        }
    }

    fn sonnet(util: f64) -> LimitData {
        LimitData {
            utilization: util,
            resets_at: Utc::now() + Duration::hours(100),
            limit_type: LimitType::WeeklySonnet,
        }
    }

    fn monthly(limit_cents: i64, used_cents: f64, util: f64) -> MonthlyLimitData {
        MonthlyLimitData {
            monthly_limit_cents: limit_cents,
            used_credits_cents: used_cents,
            utilization: util,
            resets_at: Utc::now() + Duration::days(15),
        }
    }

    #[test]
    fn all_none_returns_placeholder() {
        assert_eq!(render_compact(None, None, None, None), "No data available");
    }

    #[test]
    fn session_only_includes_percentage_and_time() {
        let out = render_compact(Some(&session(0.45)), None, None, None);
        assert!(out.starts_with("Session:"), "got: {out}");
        assert!(out.contains("45%"), "got: {out}");
        assert!(out.contains('('), "should have time remaining: {out}");
    }

    #[test]
    fn weekly_only_format() {
        let out = render_compact(None, Some(&weekly(0.12)), None, None);
        assert!(out.starts_with("Weekly:"), "got: {out}");
        assert!(out.contains("12%"), "got: {out}");
    }

    #[test]
    fn parts_joined_with_dot_separator() {
        let out = render_compact(Some(&session(0.3)), Some(&weekly(0.1)), None, None);
        assert!(
            out.contains(" · "),
            "parts should be separated by ' · ', got: {out}"
        );
    }

    #[test]
    fn all_four_limits_present() {
        let out = render_compact(
            Some(&session(0.4)),
            Some(&weekly(0.1)),
            Some(&sonnet(0.05)),
            Some(&monthly(30000, 7475.0, 0.25)),
        );
        assert!(out.contains("Session:"), "got: {out}");
        assert!(out.contains("Weekly:"), "got: {out}");
        assert!(out.contains("Sonnet:"), "got: {out}");
        assert!(out.contains("Monthly:"), "got: {out}");
        assert_eq!(out.matches(" · ").count(), 3);
    }

    #[test]
    fn expired_session_shows_zero_percent() {
        let expired = LimitData {
            utilization: 0.9,
            resets_at: Utc::now() - Duration::hours(1),
            limit_type: LimitType::Session,
        };
        let out = render_compact(Some(&expired), None, None, None);
        assert!(
            out.contains("0%"),
            "expired session should show 0%, got: {out}"
        );
    }

    #[test]
    fn monthly_shows_dollar_amount() {
        let out = render_compact(None, None, None, Some(&monthly(30000, 7475.0, 0.25)));
        assert!(out.contains("Monthly:"), "got: {out}");
        assert!(
            out.contains("$74.75"),
            "should format credits in dollars, got: {out}"
        );
    }

    #[test]
    fn pace_icon_embedded_in_output() {
        // A session well under budget (very low utilization vs high elapsed) → 🧊
        // A session well over budget → 🚨
        let under = LimitData {
            utilization: 0.01,
            resets_at: Utc::now() + Duration::minutes(1), // nearly expired → pace ~1.0
            limit_type: LimitType::Session,
        };
        let out = render_compact(Some(&under), None, None, None);
        assert!(
            out.contains("🧊"),
            "under-budget should show 🧊, got: {out}"
        );
    }
}
