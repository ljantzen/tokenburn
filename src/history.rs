use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::credentials::tokenburn_data_dir;
use crate::models::{LimitData, LimitType, MonthlyLimitData, UsageSnapshot};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS usage_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    five_hour_utilization REAL,
    five_hour_resets_at TEXT,
    seven_day_all_utilization REAL,
    seven_day_all_resets_at TEXT,
    seven_day_sonnet_utilization REAL,
    seven_day_sonnet_resets_at TEXT,
    seven_day_opus_utilization REAL,
    seven_day_opus_resets_at TEXT,
    monthly_limit_cents INTEGER,
    monthly_used_credits_cents REAL,
    monthly_utilization REAL,
    monthly_resets_at TEXT,
    raw_response TEXT
);
CREATE INDEX IF NOT EXISTS idx_snapshots_timestamp ON usage_snapshots(timestamp);
CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT);
";

pub struct HistoryDB {
    conn: Connection,
}

impl HistoryDB {
    pub fn open() -> Result<Self> {
        let dir = tokenburn_data_dir();
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("history.db");
        Self::open_at(db_path)
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Self::migrate(&conn)?;
        Ok(HistoryDB { conn })
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(HistoryDB { conn })
    }

    fn migrate(conn: &Connection) -> Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(usage_snapshots)")?;
        let columns: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(|r| r.ok())
            .collect();
        if !columns.iter().any(|c| c == "monthly_utilization") {
            conn.execute_batch(
                "
                ALTER TABLE usage_snapshots ADD COLUMN monthly_limit_cents INTEGER;
                ALTER TABLE usage_snapshots ADD COLUMN monthly_used_credits_cents REAL;
                ALTER TABLE usage_snapshots ADD COLUMN monthly_utilization REAL;
                ALTER TABLE usage_snapshots ADD COLUMN monthly_resets_at TEXT;
            ",
            )?;
        }
        Ok(())
    }

    pub fn save_snapshot(&self, snapshot: &UsageSnapshot) -> Result<()> {
        let five_hour_util = snapshot.session.as_ref().map(|s| s.utilization);
        // Deduplicate: skip if same utilization within 5 seconds
        let recent_cutoff = (snapshot.timestamp - chrono::Duration::seconds(5)).to_rfc3339();
        let exists: bool = self.conn.query_row(
            "SELECT 1 FROM usage_snapshots WHERE timestamp > ? AND five_hour_utilization = ? LIMIT 1",
            params![recent_cutoff, five_hour_util],
            |_| Ok(true),
        ).unwrap_or(false);
        if exists {
            return Ok(());
        }

        self.conn.execute(
            "INSERT INTO usage_snapshots (
                timestamp,
                five_hour_utilization, five_hour_resets_at,
                seven_day_all_utilization, seven_day_all_resets_at,
                seven_day_sonnet_utilization, seven_day_sonnet_resets_at,
                seven_day_opus_utilization, seven_day_opus_resets_at,
                monthly_limit_cents, monthly_used_credits_cents,
                monthly_utilization, monthly_resets_at,
                raw_response
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                snapshot.timestamp.to_rfc3339(),
                snapshot.session.as_ref().map(|s| s.utilization),
                snapshot.session.as_ref().map(|s| s.resets_at.to_rfc3339()),
                snapshot.weekly.as_ref().map(|s| s.utilization),
                snapshot.weekly.as_ref().map(|s| s.resets_at.to_rfc3339()),
                snapshot.weekly_sonnet.as_ref().map(|s| s.utilization),
                snapshot
                    .weekly_sonnet
                    .as_ref()
                    .map(|s| s.resets_at.to_rfc3339()),
                snapshot.weekly_opus.as_ref().map(|s| s.utilization),
                snapshot
                    .weekly_opus
                    .as_ref()
                    .map(|s| s.resets_at.to_rfc3339()),
                snapshot.monthly.as_ref().map(|m| m.monthly_limit_cents),
                snapshot.monthly.as_ref().map(|m| m.used_credits_cents),
                snapshot.monthly.as_ref().map(|m| m.utilization),
                snapshot.monthly.as_ref().map(|m| m.resets_at.to_rfc3339()),
                snapshot.raw_response.as_deref(),
            ],
        )?;
        Ok(())
    }

    pub fn get_latest_snapshot(&self) -> Result<Option<UsageSnapshot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM usage_snapshots ORDER BY timestamp DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(row_to_snapshot(row))
        } else {
            Ok(None)
        }
    }

    pub fn get_latest_snapshot_age_seconds(&self) -> Option<f64> {
        let latest = self.get_latest_snapshot().ok()??;
        let age = (Utc::now() - latest.timestamp).num_milliseconds() as f64 / 1000.0;
        Some(age)
    }

    pub fn get_snapshots(&self, since: Option<DateTime<Utc>>) -> Result<Vec<UsageSnapshot>> {
        let snapshots = if let Some(since) = since {
            let mut stmt = self.conn.prepare(
                "SELECT * FROM usage_snapshots WHERE timestamp >= ? ORDER BY timestamp ASC",
            )?;
            let rows = stmt.query_map(params![since.to_rfc3339()], |r| Ok(row_to_snapshot(r)))?;
            rows.filter_map(|r| r.ok().flatten()).collect()
        } else {
            let mut stmt = self
                .conn
                .prepare("SELECT * FROM usage_snapshots ORDER BY timestamp ASC")?;
            let rows = stmt.query_map([], |r| Ok(row_to_snapshot(r)))?;
            rows.filter_map(|r| r.ok().flatten()).collect()
        };
        Ok(snapshots)
    }

    pub fn get_snapshots_for_limit(
        &self,
        limit_type: LimitType,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<UsageSnapshot>> {
        let all = self.get_snapshots(since)?;
        Ok(all
            .into_iter()
            .filter(|s| s.get_limit(limit_type).is_some())
            .collect())
    }
}

fn parse_dt(s: Option<&str>) -> Option<DateTime<Utc>> {
    s.and_then(|s| {
        DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
    })
}

fn row_to_snapshot(row: &rusqlite::Row) -> Option<UsageSnapshot> {
    let ts_str: String = row.get(1).ok()?;
    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
        .ok()?
        .with_timezone(&Utc);

    let session = {
        let util: Option<f64> = row.get(2).ok()?;
        let resets: Option<String> = row.get(3).ok()?;
        util.zip(resets.as_deref().and_then(|s| parse_dt(Some(s))))
            .map(|(u, r)| LimitData {
                utilization: u,
                resets_at: r,
                limit_type: LimitType::Session,
            })
    };

    let weekly = {
        let util: Option<f64> = row.get(4).ok()?;
        let resets: Option<String> = row.get(5).ok()?;
        util.zip(resets.as_deref().and_then(|s| parse_dt(Some(s))))
            .map(|(u, r)| LimitData {
                utilization: u,
                resets_at: r,
                limit_type: LimitType::Weekly,
            })
    };

    let weekly_sonnet = {
        let util: Option<f64> = row.get(6).ok()?;
        let resets: Option<String> = row.get(7).ok()?;
        util.zip(resets.as_deref().and_then(|s| parse_dt(Some(s))))
            .map(|(u, r)| LimitData {
                utilization: u,
                resets_at: r,
                limit_type: LimitType::WeeklySonnet,
            })
    };

    let weekly_opus = {
        let util: Option<f64> = row.get(8).ok()?;
        let resets: Option<String> = row.get(9).ok()?;
        util.zip(resets.as_deref().and_then(|s| parse_dt(Some(s))))
            .map(|(u, r)| LimitData {
                utilization: u,
                resets_at: r,
                limit_type: LimitType::Weekly,
            })
    };

    let monthly = {
        let util: Option<f64> = row.get(12).ok()?;
        if let Some(util) = util {
            let limit: Option<i64> = row.get(10).ok()?;
            let used: Option<f64> = row.get(11).ok()?;
            let resets: Option<String> = row.get(13).ok()?;
            if let (Some(limit), Some(used), Some(resets)) = (
                limit,
                used,
                resets.as_deref().and_then(|s| parse_dt(Some(s))),
            ) {
                Some(MonthlyLimitData {
                    monthly_limit_cents: limit,
                    used_credits_cents: used,
                    utilization: util,
                    resets_at: resets,
                })
            } else {
                None
            }
        } else {
            None
        }
    };

    let raw: Option<String> = row.get(14).ok()?;

    Some(UsageSnapshot {
        timestamp,
        session,
        weekly,
        weekly_sonnet,
        weekly_opus,
        monthly,
        raw_response: raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn session_snapshot(util: f64, hours_ago: i64) -> UsageSnapshot {
        UsageSnapshot {
            timestamp: Utc::now() - Duration::hours(hours_ago),
            session: Some(LimitData {
                utilization: util,
                resets_at: Utc::now() + Duration::hours(5 - hours_ago),
                limit_type: LimitType::Session,
            }),
            weekly: None,
            weekly_sonnet: None,
            weekly_opus: None,
            monthly: None,
            raw_response: Some(r#"{"test":true}"#.to_string()),
        }
    }

    fn weekly_snapshot(util: f64) -> UsageSnapshot {
        UsageSnapshot {
            timestamp: Utc::now(),
            session: None,
            weekly: Some(LimitData {
                utilization: util,
                resets_at: Utc::now() + Duration::hours(100),
                limit_type: LimitType::Weekly,
            }),
            weekly_sonnet: None,
            weekly_opus: None,
            monthly: None,
            raw_response: None,
        }
    }

    #[test]
    fn empty_db_returns_none() {
        let db = HistoryDB::in_memory().unwrap();
        assert!(db.get_latest_snapshot().unwrap().is_none());
        assert!(db.get_snapshots(None).unwrap().is_empty());
    }

    #[test]
    fn round_trip_session_snapshot() {
        let db = HistoryDB::in_memory().unwrap();
        let snap = session_snapshot(0.42, 1);
        db.save_snapshot(&snap).unwrap();

        let loaded = db
            .get_latest_snapshot()
            .unwrap()
            .expect("should have a snapshot");
        let session = loaded.session.expect("session should survive round-trip");
        assert!((session.utilization - 0.42).abs() < 0.0001);
        assert_eq!(session.limit_type, LimitType::Session);
    }

    #[test]
    fn latest_snapshot_returns_most_recent() {
        let db = HistoryDB::in_memory().unwrap();
        db.save_snapshot(&session_snapshot(0.1, 3)).unwrap();
        db.save_snapshot(&session_snapshot(0.5, 1)).unwrap();

        let latest = db.get_latest_snapshot().unwrap().unwrap();
        let util = latest.session.unwrap().utilization;
        assert!((util - 0.5).abs() < 0.0001);
    }

    #[test]
    fn get_snapshots_returns_all_in_order() {
        let db = HistoryDB::in_memory().unwrap();
        db.save_snapshot(&session_snapshot(0.1, 4)).unwrap();
        db.save_snapshot(&session_snapshot(0.2, 2)).unwrap();
        db.save_snapshot(&session_snapshot(0.3, 0)).unwrap();

        let snaps = db.get_snapshots(None).unwrap();
        assert_eq!(snaps.len(), 3);
        // Ascending order
        let utils: Vec<f64> = snaps
            .iter()
            .map(|s| s.session.as_ref().unwrap().utilization)
            .collect();
        assert!(utils[0] < utils[1] && utils[1] < utils[2]);
    }

    #[test]
    fn get_snapshots_since_filters_old() {
        let db = HistoryDB::in_memory().unwrap();
        db.save_snapshot(&session_snapshot(0.1, 10)).unwrap();
        db.save_snapshot(&session_snapshot(0.5, 1)).unwrap();

        let since = Utc::now() - Duration::hours(3);
        let snaps = db.get_snapshots(Some(since)).unwrap();
        assert_eq!(snaps.len(), 1);
        assert!((snaps[0].session.as_ref().unwrap().utilization - 0.5).abs() < 0.0001);
    }

    #[test]
    fn get_snapshots_for_limit_filters_by_type() {
        let db = HistoryDB::in_memory().unwrap();
        db.save_snapshot(&session_snapshot(0.3, 1)).unwrap();
        db.save_snapshot(&weekly_snapshot(0.15)).unwrap();

        let session_snaps = db
            .get_snapshots_for_limit(LimitType::Session, None)
            .unwrap();
        let weekly_snaps = db.get_snapshots_for_limit(LimitType::Weekly, None).unwrap();

        assert_eq!(session_snaps.len(), 1);
        assert_eq!(weekly_snaps.len(), 1);
    }

    #[test]
    fn deduplication_skips_same_utilization_within_5s() {
        let db = HistoryDB::in_memory().unwrap();
        let snap = session_snapshot(0.42, 0);
        db.save_snapshot(&snap).unwrap();
        db.save_snapshot(&snap).unwrap(); // same utilization, same timestamp

        let count = db.get_snapshots(None).unwrap().len();
        assert_eq!(count, 1, "duplicate within 5s should be skipped");
    }

    #[test]
    fn different_utilization_is_not_deduplicated() {
        let db = HistoryDB::in_memory().unwrap();
        db.save_snapshot(&session_snapshot(0.4, 0)).unwrap();
        db.save_snapshot(&session_snapshot(0.6, 0)).unwrap();

        let count = db.get_snapshots(None).unwrap().len();
        assert_eq!(count, 2);
    }

    #[test]
    fn snapshot_age_reflects_recency() {
        let db = HistoryDB::in_memory().unwrap();
        assert!(db.get_latest_snapshot_age_seconds().is_none());
        db.save_snapshot(&session_snapshot(0.5, 0)).unwrap();
        let age = db
            .get_latest_snapshot_age_seconds()
            .expect("age should be Some");
        assert!(
            age < 5.0,
            "freshly saved snapshot should be < 5s old, got {age}"
        );
    }

    #[test]
    fn monthly_data_round_trips() {
        let db = HistoryDB::in_memory().unwrap();
        let snap = UsageSnapshot {
            timestamp: Utc::now(),
            session: None,
            weekly: None,
            weekly_sonnet: None,
            weekly_opus: None,
            monthly: Some(MonthlyLimitData {
                monthly_limit_cents: 30000,
                used_credits_cents: 7475.0,
                utilization: 0.249,
                resets_at: Utc::now() + Duration::days(15),
            }),
            raw_response: None,
        };
        db.save_snapshot(&snap).unwrap();
        let loaded = db.get_latest_snapshot().unwrap().unwrap();
        let m = loaded.monthly.expect("monthly should survive round-trip");
        assert_eq!(m.monthly_limit_cents, 30000);
        assert!((m.used_credits_cents - 7475.0).abs() < 0.001);
        assert!((m.utilization - 0.249).abs() < 0.0001);
    }
}
