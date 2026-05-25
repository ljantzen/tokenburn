use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, Utc};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::calculator::calculate_burn_metrics;
use crate::display::{compact::render_compact, tui};
use crate::history::HistoryDB;
use crate::models::{AnyLimit, LimitType, UsageSnapshot};
use crate::usage_client::UsageClient;

type SharedState = Arc<Mutex<(UsageSnapshot, Vec<UsageSnapshot>, Option<String>)>>;

pub struct AppConfig {
    pub limit_type: Option<LimitType>,
    pub compact: bool,
    pub json: bool,
    pub once: bool,
    pub interval: u64,
    pub poll_interval: u64,
}

pub fn run(config: AppConfig) -> Result<()> {
    let mut db = HistoryDB::open().unwrap_or_else(|_| HistoryDB::in_memory().unwrap());

    // Initial fetch (try DB cache first, then API)
    let client = UsageClient::new();
    let snapshot = fetch_or_cache(&client, &mut db)?;

    // Resolve limit type
    let limit_type = config
        .limit_type
        .unwrap_or_else(|| auto_detect_limit(&snapshot));

    // Reload snapshots for this limit
    let snapshots = db
        .get_snapshots_for_limit(limit_type, None)
        .unwrap_or_default();

    let limit = snapshot.get_limit(limit_type);
    if limit.is_none() {
        print_unavailable(&snapshot, limit_type);
        return Ok(());
    }

    // Dispatch to output mode
    if config.json {
        print_json(&snapshot, &snapshots, limit_type);
        return Ok(());
    }
    if config.compact {
        println!(
            "{}",
            render_compact(
                snapshot.session.as_ref(),
                snapshot.weekly.as_ref(),
                snapshot.weekly_sonnet.as_ref(),
                snapshot.monthly.as_ref(),
            )
        );
        return Ok(());
    }
    if config.once {
        run_once(limit.as_ref().unwrap(), &snapshots, limit_type, None, None)?;
        return Ok(());
    }

    run_tui(config, client, db, snapshot, snapshots, limit_type)
}

fn fetch_or_cache(client: &UsageClient, db: &mut HistoryDB) -> Result<UsageSnapshot> {
    // Check DB cache freshness
    if db
        .get_latest_snapshot_age_seconds()
        .is_some_and(|age| age < 120.0)
        && let Ok(Some(cached)) = db.get_latest_snapshot()
    {
        return Ok(cached);
    }
    // Fetch from API
    match client.fetch_usage() {
        Ok(snapshot) => {
            let _ = db.save_snapshot(&snapshot);
            Ok(snapshot)
        }
        Err(api_err) => {
            // Fall back to any cached data
            if let Ok(Some(cached)) = db.get_latest_snapshot() {
                eprintln!("Warning: API failed ({api_err}), using cached data");
                return Ok(cached);
            }
            Err(api_err)
        }
    }
}

fn auto_detect_limit(snap: &UsageSnapshot) -> LimitType {
    if snap.session.is_some() {
        LimitType::Session
    } else if snap.monthly.is_some() {
        LimitType::Monthly
    } else if snap.weekly.is_some() {
        LimitType::Weekly
    } else {
        LimitType::Session
    }
}

fn run_once(
    limit: &AnyLimit,
    snapshots: &[UsageSnapshot],
    limit_type: LimitType,
    error: Option<&str>,
    stale_since: Option<DateTime<Utc>>,
) -> Result<()> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|f| {
        tui::render(
            f,
            &tui::AppState {
                limit_type,
                limit: Some(limit.clone()),
                snapshots,
                error,
                stale_since,
            },
        );
    })?;
    Ok(())
}

fn run_tui(
    config: AppConfig,
    client: UsageClient,
    db: HistoryDB,
    initial_snapshot: UsageSnapshot,
    initial_snapshots: Vec<UsageSnapshot>,
    limit_type: LimitType,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Shared state
    let snapshot_arc: SharedState =
        Arc::new(Mutex::new((initial_snapshot, initial_snapshots, None)));

    let arc_clone = Arc::clone(&snapshot_arc);
    let poll_ms = config.poll_interval * 1000;
    let interval_ms = config.interval * 1000;

    // Background polling thread
    thread::spawn(move || {
        let mut last_poll = Instant::now() - Duration::from_millis(poll_ms);
        loop {
            thread::sleep(Duration::from_secs(1));
            if last_poll.elapsed().as_millis() >= poll_ms as u128 {
                match client.fetch_usage() {
                    Ok(snap) => {
                        let snaps = db
                            .get_snapshots_for_limit(limit_type, None)
                            .unwrap_or_default();
                        let _ = db.save_snapshot(&snap);
                        let mut state = arc_clone.lock().unwrap();
                        *state = (snap, snaps, None);
                    }
                    Err(e) => {
                        let mut state = arc_clone.lock().unwrap();
                        state.2 = Some(e.to_string());
                    }
                }
                last_poll = Instant::now();
            }
        }
    });

    let tick = Duration::from_millis(interval_ms);
    let result = (|| -> Result<()> {
        loop {
            let state_guard = snapshot_arc.lock().unwrap();
            let (snap, snaps, err) = &*state_guard;
            let limit = snap.get_limit(limit_type);
            let error_str: Option<String> = err.clone();
            let snaps_clone = snaps.clone();
            drop(state_guard);

            terminal.draw(|f| {
                tui::render(
                    f,
                    &tui::AppState {
                        limit_type,
                        limit: limit.clone(),
                        snapshots: &snaps_clone,
                        error: error_str.as_deref(),
                        stale_since: None,
                    },
                );
            })?;

            if event::poll(tick)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
        Ok(())
    })();

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    result
}

fn print_unavailable(snap: &UsageSnapshot, limit_type: LimitType) {
    eprintln!("{} data not available.", limit_type.display_name());
    eprintln!("\nAvailable:");
    if snap.session.is_some() {
        eprintln!("  - session");
    }
    if snap.weekly.is_some() {
        eprintln!("  - weekly");
    }
    if snap.weekly_sonnet.is_some() {
        eprintln!("  - weekly-sonnet");
    }
    if snap.monthly.is_some() {
        eprintln!("  - monthly");
    }
    eprintln!("\nRun 'tokenburn' to auto-detect.");
}

fn print_json(snap: &UsageSnapshot, snapshots: &[UsageSnapshot], limit_type: LimitType) {
    use chrono::Utc;
    use serde_json::{Value, json};

    let mut limits = serde_json::Map::new();
    for lt in [
        LimitType::Session,
        LimitType::Weekly,
        LimitType::WeeklySonnet,
    ] {
        if let Some(limit) = snap.get_limit(lt) {
            let metrics = calculate_burn_metrics(&limit, snapshots);
            let minutes_left = (limit.resets_at() - Utc::now()).num_minutes();
            limits.insert(
                format!("{lt:?}").to_lowercase(),
                json!({
                    "utilization": limit.effective_utilization(),
                    "budget_pace": metrics.budget_pace,
                    "resets_at": limit.resets_at().to_rfc3339(),
                    "window_hours": limit.window_hours(),
                    "status": metrics.status,
                    "minutes_left": minutes_left,
                }),
            );
        }
    }
    if let Some(m) = &snap.monthly {
        let limit = AnyLimit::Monthly(m.clone());
        let metrics = calculate_burn_metrics(&limit, snapshots);
        let days_left = (m.resets_at - Utc::now()).num_days();
        limits.insert(
            "monthly".to_string(),
            json!({
                "utilization": m.effective_utilization(),
                "budget_pace": metrics.budget_pace,
                "resets_at": m.resets_at.to_rfc3339(),
                "window_hours": m.window_hours(),
                "status": metrics.status,
                "days_left": days_left,
                "used_credits_dollars": m.used_credits_dollars(),
                "monthly_limit_dollars": m.monthly_limit_dollars(),
                "remaining_dollars": m.remaining_dollars(),
            }),
        );
    }

    let burn_rate: Value = if let Some(limit) = snap.get_limit(limit_type) {
        let metrics = calculate_burn_metrics(&limit, snapshots);
        json!({
            "limit": format!("{limit_type:?}").to_lowercase(),
            "percent_per_hour": metrics.percent_per_hour,
            "trend": metrics.trend,
            "estimated_minutes_to_100": metrics.estimated_minutes_to_100,
        })
    } else {
        Value::Null
    };

    let output = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "limits": limits,
        "burn_rate": burn_rate,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).unwrap_or_default()
    );
}
