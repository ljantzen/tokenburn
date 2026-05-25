use chrono::{DateTime, Duration, Utc};

use crate::models::{AnyLimit, BurnMetrics, LimitType, UsageSnapshot};

pub fn calculate_budget_pace(resets_at: DateTime<Utc>, window_hours: i64) -> f64 {
    let now = Utc::now();
    let window_start = resets_at - Duration::hours(window_hours);
    let elapsed = (now - window_start).num_seconds() as f64;
    let window_secs = (window_hours * 3600) as f64;
    if window_secs <= 0.0 {
        return 0.0;
    }
    (elapsed / window_secs).clamp(0.0, 1.0)
}

pub fn calculate_burn_rate(
    snapshots: &[UsageSnapshot],
    limit_type: LimitType,
    window_start: DateTime<Utc>,
    window_hours: i64,
) -> f64 {
    if snapshots.len() < 2 {
        return 0.0;
    }

    let points: Vec<(f64, f64)> = snapshots
        .iter()
        .filter(|s| s.timestamp >= window_start)
        .filter_map(|s| {
            let limit = s.get_limit(limit_type)?;
            let util = limit.utilization();
            if util > 1.0 {
                return None;
            }
            Some((s.timestamp, util * 100.0))
        })
        .enumerate()
        .scan(None::<DateTime<Utc>>, |first, (_, (ts, util))| {
            if first.is_none() {
                *first = Some(ts);
            }
            let hours = (ts - first.unwrap()).num_seconds() as f64 / 3600.0;
            Some((hours, util))
        })
        .collect();

    if points.len() < 3 {
        return 0.0;
    }

    // Check minimum span (10% of window, capped at 6h)
    let span_hours = points.last().map(|(h, _)| *h).unwrap_or(0.0);
    let min_span = (window_hours as f64 * 0.10).min(6.0);
    if span_hours < min_span {
        return 0.0;
    }

    if points.len() == 2 {
        let dx = points[1].0 - points[0].0;
        if dx <= 0.0 {
            return 0.0;
        }
        return (points[1].1 - points[0].1) / dx;
    }

    // Linear regression
    let n = points.len() as f64;
    let sum_x: f64 = points.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = points.iter().map(|(_, y)| y).sum();
    let sum_xy: f64 = points.iter().map(|(x, y)| x * y).sum();
    let sum_x2: f64 = points.iter().map(|(x, _)| x * x).sum();
    let denom = n * sum_x2 - sum_x * sum_x;
    if denom.abs() < 1e-10 {
        return 0.0;
    }
    (n * sum_xy - sum_x * sum_y) / denom
}

pub fn estimate_minutes_to_100(utilization: f64, burn_rate: f64) -> Option<i64> {
    if burn_rate <= 0.0 {
        return None;
    }
    let remaining = 100.0 - utilization * 100.0;
    if remaining <= 0.0 {
        return Some(0);
    }
    Some((remaining / burn_rate * 60.0) as i64)
}

fn classify_trend(rate: f64) -> &'static str {
    if rate < 5.0 {
        "low"
    } else if rate < 15.0 {
        "moderate"
    } else if rate < 30.0 {
        "high"
    } else {
        "critical"
    }
}

fn get_status(utilization: f64, budget_pace: f64) -> &'static str {
    let diff = utilization - budget_pace;
    if diff.abs() < 0.05 {
        "on_pace"
    } else if diff > 0.0 {
        "ahead_of_pace"
    } else {
        "behind_pace"
    }
}

#[cfg(test)]
pub(crate) fn classify_trend_pub(rate: f64) -> &'static str {
    classify_trend(rate)
}
#[cfg(test)]
pub(crate) fn get_status_pub(u: f64, p: f64) -> &'static str {
    get_status(u, p)
}
pub fn calculate_burn_metrics(limit: &AnyLimit, snapshots: &[UsageSnapshot]) -> BurnMetrics {
    let budget_pace = calculate_budget_pace(limit.resets_at(), limit.window_hours());
    let burn_rate = calculate_burn_rate(
        snapshots,
        limit.limit_type(),
        limit.window_start(),
        limit.window_hours(),
    );
    let time_to_100 = estimate_minutes_to_100(limit.utilization(), burn_rate);
    BurnMetrics {
        percent_per_hour: burn_rate,
        trend: classify_trend(burn_rate),
        estimated_minutes_to_100: time_to_100,
        budget_pace,
        status: get_status(limit.utilization(), budget_pace),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LimitData;
    use chrono::Duration;

    #[allow(dead_code)]
    fn make_session(utilization: f64, resets_in_hours: i64) -> LimitData {
        LimitData {
            utilization,
            resets_at: Utc::now() + Duration::hours(resets_in_hours),
            limit_type: LimitType::Session,
        }
    }

    fn make_snapshot(
        util: f64,
        hours_ago: i64,
        limit_type: LimitType,
        resets_in: i64,
    ) -> UsageSnapshot {
        UsageSnapshot {
            timestamp: Utc::now() - Duration::hours(hours_ago),
            session: if limit_type == LimitType::Session {
                Some(LimitData {
                    utilization: util,
                    resets_at: Utc::now() + Duration::hours(resets_in),
                    limit_type: LimitType::Session,
                })
            } else {
                None
            },
            weekly: if limit_type == LimitType::Weekly {
                Some(LimitData {
                    utilization: util,
                    resets_at: Utc::now() + Duration::hours(resets_in),
                    limit_type: LimitType::Weekly,
                })
            } else {
                None
            },
            weekly_sonnet: None,
            weekly_opus: None,
            monthly: None,
            raw_response: None,
        }
    }

    // --- calculate_budget_pace ---

    #[test]
    fn pace_at_window_start() {
        // resets_at is exactly window_hours in the future → window just started
        let resets_at = Utc::now() + Duration::hours(5);
        let pace = calculate_budget_pace(resets_at, 5);
        assert!(pace < 0.01, "pace should be ~0 at window start, got {pace}");
    }

    #[test]
    fn pace_at_window_midpoint() {
        // 2.5h into a 5h window
        let resets_at = Utc::now() + Duration::minutes(150);
        let pace = calculate_budget_pace(resets_at, 5);
        assert!(
            (pace - 0.5).abs() < 0.01,
            "pace should be ~0.5 at midpoint, got {pace}"
        );
    }

    #[test]
    fn pace_at_window_end() {
        let resets_at = Utc::now();
        let pace = calculate_budget_pace(resets_at, 5);
        assert!(
            (pace - 1.0).abs() < 0.01,
            "pace should be ~1.0 at window end, got {pace}"
        );
    }

    #[test]
    fn pace_clamped_to_one_past_end() {
        let resets_at = Utc::now() - Duration::hours(1);
        let pace = calculate_budget_pace(resets_at, 5);
        assert_eq!(pace, 1.0);
    }

    #[test]
    fn pace_zero_window_returns_zero() {
        let resets_at = Utc::now() + Duration::hours(1);
        let pace = calculate_budget_pace(resets_at, 0);
        assert_eq!(pace, 0.0);
    }

    // --- estimate_minutes_to_100 ---

    #[test]
    fn minutes_to_100_zero_rate() {
        assert_eq!(estimate_minutes_to_100(0.5, 0.0), None);
    }

    #[test]
    fn minutes_to_100_negative_rate() {
        assert_eq!(estimate_minutes_to_100(0.5, -5.0), None);
    }

    #[test]
    fn minutes_to_100_already_full() {
        assert_eq!(estimate_minutes_to_100(1.0, 10.0), Some(0));
    }

    #[test]
    fn minutes_to_100_half_used_10pct_per_hour() {
        // 50% remaining at 10%/h → 5 hours = 300 minutes
        assert_eq!(estimate_minutes_to_100(0.5, 10.0), Some(300));
    }

    #[test]
    fn minutes_to_100_zero_used() {
        // 100% remaining at 20%/h → 5 hours = 300 minutes
        assert_eq!(estimate_minutes_to_100(0.0, 20.0), Some(300));
    }

    // --- calculate_burn_rate ---

    #[test]
    fn burn_rate_too_few_snapshots() {
        let window_start = Utc::now() - Duration::hours(5);
        let snaps = vec![make_snapshot(0.1, 4, LimitType::Session, 1)];
        assert_eq!(
            calculate_burn_rate(&snaps, LimitType::Session, window_start, 5),
            0.0
        );
    }

    #[test]
    fn burn_rate_insufficient_span() {
        // 3 points but only 10 minutes apart — below the 30-minute minimum for a 5h window
        let window_start = Utc::now() - Duration::hours(5);
        let snaps: Vec<UsageSnapshot> = (0..3)
            .map(|i| UsageSnapshot {
                timestamp: Utc::now() - Duration::minutes(10 * (2 - i)),
                session: Some(LimitData {
                    utilization: 0.1 + i as f64 * 0.01,
                    resets_at: Utc::now() + Duration::hours(1),
                    limit_type: LimitType::Session,
                }),
                weekly: None,
                weekly_sonnet: None,
                weekly_opus: None,
                monthly: None,
                raw_response: None,
            })
            .collect();
        assert_eq!(
            calculate_burn_rate(&snaps, LimitType::Session, window_start, 5),
            0.0
        );
    }

    #[test]
    fn burn_rate_linear_increase() {
        // 4 points evenly spaced over 2 hours, utilization rising by 10% per hour
        let window_start = Utc::now() - Duration::hours(5);
        let snaps: Vec<UsageSnapshot> = (0..4)
            .map(|i| {
                UsageSnapshot {
                    timestamp: Utc::now() - Duration::minutes(120 - i * 40),
                    session: Some(LimitData {
                        utilization: 0.10 + i as f64 * 0.10 / 3.0, // 10% → 20% over 2h
                        resets_at: Utc::now() + Duration::hours(3),
                        limit_type: LimitType::Session,
                    }),
                    weekly: None,
                    weekly_sonnet: None,
                    weekly_opus: None,
                    monthly: None,
                    raw_response: None,
                }
            })
            .collect();
        let rate = calculate_burn_rate(&snaps, LimitType::Session, window_start, 5);
        // Expected: ~5 %/hour
        assert!(rate > 4.0 && rate < 6.0, "expected ~5 %/h, got {rate}");
    }

    #[test]
    fn burn_rate_skips_values_over_one() {
        let window_start = Utc::now() - Duration::hours(5);
        let mut snaps: Vec<UsageSnapshot> = (0..4)
            .map(|i| UsageSnapshot {
                timestamp: Utc::now() - Duration::minutes(120 - i * 40),
                session: Some(LimitData {
                    utilization: 0.1 + i as f64 * 0.05,
                    resets_at: Utc::now() + Duration::hours(3),
                    limit_type: LimitType::Session,
                }),
                weekly: None,
                weekly_sonnet: None,
                weekly_opus: None,
                monthly: None,
                raw_response: None,
            })
            .collect();
        // Inject a bad data point (unnormalized value > 1)
        snaps[1].session.as_mut().unwrap().utilization = 1.5;
        // Should not panic and should produce a sensible (possibly 0) result
        let rate = calculate_burn_rate(&snaps, LimitType::Session, window_start, 5);
        assert!(rate >= 0.0);
    }

    // --- classify_trend / get_status / get_recommendation ---

    #[test]
    fn trend_classification() {
        assert_eq!(classify_trend_pub(0.0), "low");
        assert_eq!(classify_trend_pub(4.9), "low");
        assert_eq!(classify_trend_pub(5.0), "moderate");
        assert_eq!(classify_trend_pub(14.9), "moderate");
        assert_eq!(classify_trend_pub(15.0), "high");
        assert_eq!(classify_trend_pub(29.9), "high");
        assert_eq!(classify_trend_pub(30.0), "critical");
    }

    #[test]
    fn status_on_pace() {
        assert_eq!(get_status_pub(0.5, 0.5), "on_pace");
        assert_eq!(get_status_pub(0.5, 0.52), "on_pace"); // within 5% tolerance
    }

    #[test]
    fn status_ahead_and_behind() {
        assert_eq!(get_status_pub(0.7, 0.4), "ahead_of_pace");
        assert_eq!(get_status_pub(0.3, 0.7), "behind_pace");
    }
}
