use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitType {
    Session,
    Weekly,
    WeeklySonnet,
    Monthly,
}

impl LimitType {
    pub fn window_hours(&self) -> Option<i64> {
        match self {
            LimitType::Session => Some(5),
            LimitType::Weekly | LimitType::WeeklySonnet => Some(168),
            LimitType::Monthly => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            LimitType::Session => "Session (5h)",
            LimitType::Weekly => "Weekly",
            LimitType::WeeklySonnet => "Weekly Sonnet",
            LimitType::Monthly => "Monthly Credits",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitData {
    pub utilization: f64,
    pub resets_at: DateTime<Utc>,
    pub limit_type: LimitType,
}

impl LimitData {
    pub fn window_hours(&self) -> i64 {
        self.limit_type.window_hours().unwrap_or(168)
    }

    pub fn window_start(&self) -> DateTime<Utc> {
        self.resets_at - Duration::hours(self.window_hours())
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.resets_at
    }

    pub fn effective_utilization(&self) -> f64 {
        if self.is_expired() {
            0.0
        } else {
            self.utilization
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthlyLimitData {
    pub monthly_limit_cents: i64,
    pub used_credits_cents: f64,
    pub utilization: f64,
    pub resets_at: DateTime<Utc>,
}

impl MonthlyLimitData {
    pub fn monthly_limit_dollars(&self) -> f64 {
        self.monthly_limit_cents as f64 / 100.0
    }

    pub fn used_credits_dollars(&self) -> f64 {
        self.used_credits_cents / 100.0
    }

    pub fn remaining_dollars(&self) -> f64 {
        self.monthly_limit_dollars() - self.used_credits_dollars()
    }

    pub fn window_hours(&self) -> i64 {
        let now = Utc::now();
        let first_of_month = Utc
            .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .single()
            .unwrap_or(now);
        let days = (self.resets_at - first_of_month).num_days();
        days * 24
    }

    pub fn window_start(&self) -> DateTime<Utc> {
        let now = Utc::now();
        Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
            .single()
            .unwrap_or(now)
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() > self.resets_at
    }

    pub fn effective_utilization(&self) -> f64 {
        if self.is_expired() {
            0.0
        } else {
            self.utilization
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageSnapshot {
    pub timestamp: DateTime<Utc>,
    pub session: Option<LimitData>,
    pub weekly: Option<LimitData>,
    pub weekly_sonnet: Option<LimitData>,
    pub weekly_opus: Option<LimitData>,
    pub monthly: Option<MonthlyLimitData>,
    pub raw_response: Option<String>,
}

impl UsageSnapshot {
    pub fn get_limit(&self, limit_type: LimitType) -> Option<AnyLimit> {
        match limit_type {
            LimitType::Session => self.session.clone().map(AnyLimit::Limit),
            LimitType::Weekly => self.weekly.clone().map(AnyLimit::Limit),
            LimitType::WeeklySonnet => self.weekly_sonnet.clone().map(AnyLimit::Limit),
            LimitType::Monthly => self.monthly.clone().map(AnyLimit::Monthly),
        }
    }

    pub fn from_api_response(data: &serde_json::Value) -> Self {
        let timestamp = Utc::now();

        let parse_limit = |key: &str, lt: LimitType| -> Option<LimitData> {
            let block = data.get(key)?;
            let utilization = block.get("utilization")?.as_f64()?;
            let resets_at_str = block.get("resets_at")?.as_str()?;
            let resets_at = DateTime::parse_from_rfc3339(resets_at_str)
                .ok()?
                .with_timezone(&Utc);
            Some(LimitData {
                utilization: utilization / 100.0,
                resets_at,
                limit_type: lt,
            })
        };

        let weekly_opus = data.get("seven_day_opus").and_then(|b| {
            let util = b.get("utilization")?.as_f64()?;
            let resets_at_str = b.get("resets_at")?.as_str()?;
            let resets_at = DateTime::parse_from_rfc3339(resets_at_str)
                .ok()?
                .with_timezone(&Utc);
            Some(LimitData {
                utilization: util / 100.0,
                resets_at,
                limit_type: LimitType::Weekly,
            })
        });

        let monthly = data.get("extra_usage").and_then(|e| {
            if e.get("is_enabled")?.as_bool()? {
                let monthly_limit = e.get("monthly_limit")?.as_i64()?;
                let used_credits = e.get("used_credits")?.as_f64().unwrap_or(0.0);
                let utilization_pct = e.get("utilization")?.as_f64().unwrap_or(0.0);
                let now = Utc::now();
                let resets_at = if now.month() == 12 {
                    Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
                        .single()?
                } else {
                    Utc.with_ymd_and_hms(now.year(), now.month() + 1, 1, 0, 0, 0)
                        .single()?
                };
                Some(MonthlyLimitData {
                    monthly_limit_cents: monthly_limit,
                    used_credits_cents: used_credits,
                    utilization: utilization_pct / 100.0,
                    resets_at,
                })
            } else {
                None
            }
        });

        UsageSnapshot {
            timestamp,
            session: parse_limit("five_hour", LimitType::Session),
            weekly: parse_limit("seven_day", LimitType::Weekly),
            weekly_sonnet: parse_limit("seven_day_sonnet", LimitType::WeeklySonnet),
            weekly_opus,
            monthly,
            raw_response: Some(data.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AnyLimit {
    Limit(LimitData),
    Monthly(MonthlyLimitData),
}

impl AnyLimit {
    pub fn utilization(&self) -> f64 {
        match self {
            AnyLimit::Limit(l) => l.utilization,
            AnyLimit::Monthly(m) => m.utilization,
        }
    }

    pub fn effective_utilization(&self) -> f64 {
        match self {
            AnyLimit::Limit(l) => l.effective_utilization(),
            AnyLimit::Monthly(m) => m.effective_utilization(),
        }
    }

    pub fn resets_at(&self) -> DateTime<Utc> {
        match self {
            AnyLimit::Limit(l) => l.resets_at,
            AnyLimit::Monthly(m) => m.resets_at,
        }
    }

    pub fn window_hours(&self) -> i64 {
        match self {
            AnyLimit::Limit(l) => l.window_hours(),
            AnyLimit::Monthly(m) => m.window_hours(),
        }
    }

    pub fn window_start(&self) -> DateTime<Utc> {
        match self {
            AnyLimit::Limit(l) => l.window_start(),
            AnyLimit::Monthly(m) => m.window_start(),
        }
    }

    pub fn limit_type(&self) -> LimitType {
        match self {
            AnyLimit::Limit(l) => l.limit_type,
            AnyLimit::Monthly(_) => LimitType::Monthly,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BurnMetrics {
    pub percent_per_hour: f64,
    pub trend: &'static str,
    pub estimated_minutes_to_100: Option<i64>,
    pub budget_pace: f64,
    pub status: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use serde_json::json;

    fn future(hours: i64) -> DateTime<Utc> {
        Utc::now() + Duration::hours(hours)
    }

    fn past(hours: i64) -> DateTime<Utc> {
        Utc::now() - Duration::hours(hours)
    }

    // --- LimitType ---

    #[test]
    fn limit_type_window_hours() {
        assert_eq!(LimitType::Session.window_hours(), Some(5));
        assert_eq!(LimitType::Weekly.window_hours(), Some(168));
        assert_eq!(LimitType::WeeklySonnet.window_hours(), Some(168));
        assert_eq!(LimitType::Monthly.window_hours(), None);
    }

    #[test]
    fn limit_type_display_names_are_non_empty() {
        for lt in [
            LimitType::Session,
            LimitType::Weekly,
            LimitType::WeeklySonnet,
            LimitType::Monthly,
        ] {
            assert!(!lt.display_name().is_empty());
        }
    }

    // --- LimitData ---

    #[test]
    fn limit_data_window_start() {
        let resets_at = future(3);
        let ld = LimitData {
            utilization: 0.5,
            resets_at,
            limit_type: LimitType::Session,
        };
        let expected_start = resets_at - Duration::hours(5);
        assert!((ld.window_start() - expected_start).num_seconds().abs() < 2);
    }

    #[test]
    fn limit_data_not_expired() {
        let ld = LimitData {
            utilization: 0.5,
            resets_at: future(1),
            limit_type: LimitType::Session,
        };
        assert!(!ld.is_expired());
        assert_eq!(ld.effective_utilization(), 0.5);
    }

    #[test]
    fn limit_data_expired_returns_zero_utilization() {
        let ld = LimitData {
            utilization: 0.8,
            resets_at: past(1),
            limit_type: LimitType::Session,
        };
        assert!(ld.is_expired());
        assert_eq!(ld.effective_utilization(), 0.0);
    }

    // --- MonthlyLimitData ---

    #[test]
    fn monthly_dollar_conversions() {
        let m = MonthlyLimitData {
            monthly_limit_cents: 30000,
            used_credits_cents: 7475.0,
            utilization: 0.25,
            resets_at: future(24 * 15),
        };
        assert!((m.monthly_limit_dollars() - 300.0).abs() < 0.001);
        assert!((m.used_credits_dollars() - 74.75).abs() < 0.001);
        assert!((m.remaining_dollars() - 225.25).abs() < 0.001);
    }

    #[test]
    fn monthly_expired_returns_zero() {
        let m = MonthlyLimitData {
            monthly_limit_cents: 10000,
            used_credits_cents: 5000.0,
            utilization: 0.5,
            resets_at: past(1),
        };
        assert!(m.is_expired());
        assert_eq!(m.effective_utilization(), 0.0);
    }

    // --- UsageSnapshot::from_api_response ---

    #[test]
    fn parse_full_api_response() {
        let data = json!({
            "five_hour": { "utilization": 45.0, "resets_at": "2026-05-25T12:00:00Z" },
            "seven_day": { "utilization": 12.0, "resets_at": "2026-05-28T00:00:00Z" },
            "seven_day_sonnet": { "utilization": 8.0, "resets_at": "2026-05-28T00:00:00Z" },
        });
        let snap = UsageSnapshot::from_api_response(&data);
        let session = snap.session.expect("session should be present");
        assert!((session.utilization - 0.45).abs() < 0.001);
        let weekly = snap.weekly.expect("weekly should be present");
        assert!((weekly.utilization - 0.12).abs() < 0.001);
        let sonnet = snap.weekly_sonnet.expect("sonnet should be present");
        assert!((sonnet.utilization - 0.08).abs() < 0.001);
        assert!(snap.monthly.is_none());
    }

    #[test]
    fn parse_missing_fields_returns_none() {
        let data =
            json!({ "five_hour": { "utilization": 20.0, "resets_at": "2026-05-25T12:00:00Z" } });
        let snap = UsageSnapshot::from_api_response(&data);
        assert!(snap.session.is_some());
        assert!(snap.weekly.is_none());
        assert!(snap.weekly_sonnet.is_none());
        assert!(snap.monthly.is_none());
    }

    #[test]
    fn parse_empty_object_all_none() {
        let data = json!({});
        let snap = UsageSnapshot::from_api_response(&data);
        assert!(snap.session.is_none());
        assert!(snap.weekly.is_none());
        assert!(snap.monthly.is_none());
    }

    #[test]
    fn parse_monthly_extra_usage() {
        let data = json!({
            "extra_usage": {
                "is_enabled": true,
                "monthly_limit": 30000,
                "used_credits": 7475.0,
                "utilization": 24.9
            }
        });
        let snap = UsageSnapshot::from_api_response(&data);
        let monthly = snap.monthly.expect("monthly should be present");
        assert_eq!(monthly.monthly_limit_cents, 30000);
        assert!((monthly.used_credits_cents - 7475.0).abs() < 0.001);
        assert!((monthly.utilization - 0.249).abs() < 0.001);
    }

    #[test]
    fn parse_monthly_disabled_returns_none() {
        let data = json!({
            "extra_usage": { "is_enabled": false, "monthly_limit": 30000, "used_credits": 100.0, "utilization": 0.3 }
        });
        let snap = UsageSnapshot::from_api_response(&data);
        assert!(snap.monthly.is_none());
    }

    #[test]
    fn parse_normalizes_utilization_from_0_100_scale() {
        let data = json!({
            "five_hour": { "utilization": 100.0, "resets_at": "2026-05-25T12:00:00Z" }
        });
        let snap = UsageSnapshot::from_api_response(&data);
        assert!((snap.session.unwrap().utilization - 1.0).abs() < 0.001);
    }

    // --- UsageSnapshot::get_limit ---

    #[test]
    fn get_limit_returns_correct_variant() {
        let snap = UsageSnapshot {
            timestamp: Utc::now(),
            session: Some(LimitData {
                utilization: 0.3,
                resets_at: future(3),
                limit_type: LimitType::Session,
            }),
            weekly: None,
            weekly_sonnet: None,
            weekly_opus: None,
            monthly: None,
            raw_response: None,
        };
        assert!(snap.get_limit(LimitType::Session).is_some());
        assert!(snap.get_limit(LimitType::Weekly).is_none());
    }

    // --- AnyLimit ---

    #[test]
    fn any_limit_delegates_to_inner() {
        let ld = LimitData {
            utilization: 0.6,
            resets_at: future(2),
            limit_type: LimitType::Session,
        };
        let al = AnyLimit::Limit(ld.clone());
        assert_eq!(al.utilization(), 0.6);
        assert_eq!(al.limit_type(), LimitType::Session);
        assert!((al.resets_at() - ld.resets_at).num_seconds().abs() < 2);
    }
}
