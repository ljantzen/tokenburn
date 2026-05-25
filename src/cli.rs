use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tokenburn",
    about = "Claude Code usage limits — TUI with burn-up charts"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Output compact single-line for status bars
    #[arg(long, short = 'c')]
    pub compact: bool,

    /// Output JSON for scripting
    #[arg(long, short = 'j')]
    pub json: bool,

    /// Print once and exit (no live updates)
    #[arg(long, short = '1')]
    pub once: bool,

    /// Render interval in seconds
    #[arg(long, default_value = "5")]
    pub interval: u64,

    /// API poll interval in seconds
    #[arg(long, default_value = "60")]
    pub poll_interval: u64,
}

#[derive(Subcommand)]
pub enum Command {
    /// 5-hour rolling session limit
    Session {
        #[arg(long)]
        compact: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, short = '1')]
        once: bool,
    },
    /// 7-day weekly limit
    Weekly {
        #[arg(long)]
        compact: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, short = '1')]
        once: bool,
    },
    /// 7-day weekly Sonnet limit
    WeeklySonnet {
        #[arg(long)]
        compact: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, short = '1')]
        once: bool,
    },
    /// Monthly credits (enterprise)
    Monthly {
        #[arg(long)]
        compact: bool,
        #[arg(long)]
        json: bool,
        #[arg(long, short = '1')]
        once: bool,
    },
    /// Read statusline JSON from stdin, save to DB, pass through to stdout
    Collect,
}
