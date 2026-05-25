/// Fast stdin passthrough — reads Claude Code statusline JSON,
/// extracts rate_limits, persists to SQLite, passes stdin through to stdout.
use chrono::{Datelike, TimeZone, Utc};
use std::io::{Read, Write};

use crate::credentials::tokenburn_data_dir;

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
";

pub fn run() {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);

    // Pass through immediately
    let _ = std::io::stdout().write_all(&raw);
    let _ = std::io::stdout().flush();

    // Parse and persist in background — never fail the pipeline
    let _ = std::panic::catch_unwind(|| {
        let _ = persist(&raw);
    });
}

fn normalize_resets_at(val: &serde_json::Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = val.as_f64() {
        let secs = if n > 1e12 {
            (n / 1000.0) as i64
        } else {
            n as i64
        };
        return chrono::DateTime::from_timestamp(secs, 0)
            .map(|dt: chrono::DateTime<Utc>| dt.to_rfc3339());
    }
    None
}

fn persist(raw: &[u8]) -> Option<()> {
    let dir = tokenburn_data_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let db_path = dir.join("history.db");
    let conn = rusqlite::Connection::open(db_path).ok()?;
    conn.execute_batch(SCHEMA).ok()?;
    persist_to_conn(raw, &conn)
}

fn persist_to_conn(raw: &[u8], conn: &rusqlite::Connection) -> Option<()> {
    let data: serde_json::Value = serde_json::from_slice(raw).ok()?;
    let rate_limits = data.get("rate_limits")?;

    let now = Utc::now().to_rfc3339();

    let get_field = |key: &str| -> (Option<f64>, Option<String>) {
        let inner = || -> Option<(Option<f64>, Option<String>)> {
            let block = rate_limits.get(key)?;
            let util = block.get("used_percentage")?.as_f64().map(|u| u / 100.0);
            let resets = block.get("resets_at").and_then(normalize_resets_at);
            Some((util, resets))
        };
        inner().unwrap_or((None, None))
    };

    let (fh_util, fh_resets) = get_field("five_hour");
    let (sd_util, sd_resets) = get_field("seven_day");
    let (ss_util, ss_resets) = get_field("seven_day_sonnet");
    let (so_util, so_resets) = get_field("seven_day_opus");

    let (m_limit, m_used, m_util, m_resets) = rate_limits
        .get("extra_usage")
        .and_then(|e| {
            if e.get("is_enabled")?.as_bool()? {
                let limit = e.get("monthly_limit")?.as_i64();
                let used = e.get("used_credits")?.as_f64();
                let util = e.get("utilization")?.as_f64().map(|u| u / 100.0);
                let now = Utc::now();
                let resets = if now.month() == 12 {
                    Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
                        .single()
                        .map(|d| d.to_rfc3339())
                } else {
                    Utc.with_ymd_and_hms(now.year(), now.month() + 1, 1, 0, 0, 0)
                        .single()
                        .map(|d| d.to_rfc3339())
                };
                Some((limit, used, util, resets))
            } else {
                None
            }
        })
        .unwrap_or((None, None, None, None));

    conn.execute(
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
        rusqlite::params![
            now,
            fh_util,
            fh_resets,
            sd_util,
            sd_resets,
            ss_util,
            ss_resets,
            so_util,
            so_resets,
            m_limit,
            m_used,
            m_util,
            m_resets,
            serde_json::to_string(rate_limits).ok(),
        ],
    )
    .ok()?;

    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn
    }

    fn row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM usage_snapshots", [], |r| r.get(0))
            .unwrap()
    }

    fn get_row(conn: &Connection) -> (Option<f64>, Option<f64>, Option<f64>, Option<i64>) {
        conn.query_row(
            "SELECT five_hour_utilization, seven_day_all_utilization,
                    monthly_utilization, monthly_limit_cents
             FROM usage_snapshots LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
    }

    #[test]
    fn invalid_json_returns_none() {
        let conn = in_memory_conn();
        assert!(persist_to_conn(b"not json", &conn).is_none());
        assert_eq!(row_count(&conn), 0);
    }

    #[test]
    fn missing_rate_limits_key_returns_none() {
        let conn = in_memory_conn();
        let raw = br#"{"other_key": {}}"#;
        assert!(persist_to_conn(raw, &conn).is_none());
        assert_eq!(row_count(&conn), 0);
    }

    #[test]
    fn full_statusline_json_inserts_row() {
        let conn = in_memory_conn();
        let raw = br#"{
            "rate_limits": {
                "five_hour":  { "used_percentage": 45.0, "resets_at": "2026-05-25T12:00:00Z" },
                "seven_day":  { "used_percentage": 12.0, "resets_at": "2026-05-28T00:00:00Z" }
            }
        }"#;
        assert!(persist_to_conn(raw, &conn).is_some());
        assert_eq!(row_count(&conn), 1);
        let (fh, sd, m_util, m_limit) = get_row(&conn);
        assert!((fh.unwrap() - 0.45).abs() < 0.001);
        assert!((sd.unwrap() - 0.12).abs() < 0.001);
        assert!(m_util.is_none());
        assert!(m_limit.is_none());
    }

    #[test]
    fn used_percentage_normalised_to_0_1() {
        let conn = in_memory_conn();
        let raw = br#"{"rate_limits": {"five_hour": {"used_percentage": 100.0, "resets_at": "2026-05-25T12:00:00Z"}}}"#;
        persist_to_conn(raw, &conn).unwrap();
        let (fh, _, _, _) = get_row(&conn);
        assert!((fh.unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn resets_at_unix_ms_is_normalised() {
        // > 1e12 → treated as milliseconds
        let conn = in_memory_conn();
        let raw = br#"{"rate_limits": {"five_hour": {"used_percentage": 20.0, "resets_at": 1748170800000}}}"#;
        assert!(persist_to_conn(raw, &conn).is_some());
        let resets: Option<String> = conn
            .query_row(
                "SELECT five_hour_resets_at FROM usage_snapshots LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(resets.is_some(), "resets_at should be set");
        // Should be an RFC3339 string, not a raw number
        assert!(resets.unwrap().contains('T'));
    }

    #[test]
    fn resets_at_unix_seconds_is_normalised() {
        let conn = in_memory_conn();
        let raw = br#"{"rate_limits": {"five_hour": {"used_percentage": 20.0, "resets_at": 1748170800}}}"#;
        assert!(persist_to_conn(raw, &conn).is_some());
        let resets: Option<String> = conn
            .query_row(
                "SELECT five_hour_resets_at FROM usage_snapshots LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(resets.unwrap().contains('T'));
    }

    #[test]
    fn monthly_disabled_writes_nulls() {
        let conn = in_memory_conn();
        let raw = br#"{
            "rate_limits": {
                "five_hour": {"used_percentage": 10.0, "resets_at": "2026-05-25T12:00:00Z"},
                "extra_usage": {"is_enabled": false, "monthly_limit": 30000, "used_credits": 100.0, "utilization": 5.0}
            }
        }"#;
        persist_to_conn(raw, &conn).unwrap();
        let (_, _, m_util, m_limit) = get_row(&conn);
        assert!(m_util.is_none());
        assert!(m_limit.is_none());
    }

    #[test]
    fn monthly_enabled_writes_values() {
        let conn = in_memory_conn();
        let raw = br#"{
            "rate_limits": {
                "five_hour": {"used_percentage": 10.0, "resets_at": "2026-05-25T12:00:00Z"},
                "extra_usage": {"is_enabled": true, "monthly_limit": 30000, "used_credits": 7475.0, "utilization": 24.9}
            }
        }"#;
        persist_to_conn(raw, &conn).unwrap();
        let (_, _, m_util, m_limit) = get_row(&conn);
        assert!((m_util.unwrap() - 0.249).abs() < 0.001);
        assert_eq!(m_limit.unwrap(), 30000);
    }

    #[test]
    fn empty_rate_limits_inserts_row_with_nulls() {
        let conn = in_memory_conn();
        let raw = br#"{"rate_limits": {}}"#;
        assert!(persist_to_conn(raw, &conn).is_some());
        assert_eq!(row_count(&conn), 1);
        let (fh, sd, m_util, _) = get_row(&conn);
        assert!(fh.is_none());
        assert!(sd.is_none());
        assert!(m_util.is_none());
    }
}
