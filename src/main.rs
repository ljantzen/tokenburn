mod app;
mod calculator;
mod cli;
mod collect;
mod credentials;
mod display;
mod formatting;
mod history;
mod models;
mod usage_client;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    // Fast path for collect — bypass all heavy init
    if std::env::args().nth(1).as_deref() == Some("collect") {
        collect::run();
        return;
    }

    let cli = Cli::parse();

    let (limit_type, compact, json, once) = match &cli.command {
        Some(Command::Collect) => {
            collect::run();
            return;
        }
        Some(Command::Session {
            compact,
            json,
            once,
        }) => (Some(models::LimitType::Session), *compact, *json, *once),
        Some(Command::Weekly {
            compact,
            json,
            once,
        }) => (Some(models::LimitType::Weekly), *compact, *json, *once),
        Some(Command::WeeklySonnet {
            compact,
            json,
            once,
        }) => (
            Some(models::LimitType::WeeklySonnet),
            *compact,
            *json,
            *once,
        ),
        Some(Command::Monthly {
            compact,
            json,
            once,
        }) => (Some(models::LimitType::Monthly), *compact, *json, *once),
        None => (None, cli.compact, cli.json, cli.once),
    };

    let config = app::AppConfig {
        limit_type,
        compact: compact || cli.compact,
        json: json || cli.json,
        once: once || cli.once,
        interval: cli.interval,
        poll_interval: cli.poll_interval,
    };

    if let Err(e) = app::run(config) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
