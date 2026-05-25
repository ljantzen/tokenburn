# tokenburn

Watch your Claude Code tokens burn — before you get burned.

A Rust port of [ccburn](https://github.com/JuanjoFuchs/ccburn). TUI with burn-up charts, compact mode for status bars, JSON for automation.

## Features

- **Real-time burn-up chart** — visualize session, weekly, and monthly usage with a live-updating terminal chart
- **Pace indicators** — 🧊 Cool. 🔥 On pace. 🚨 Too hot.
- **Multiple output modes** — full TUI, compact single-line for status bars, or JSON for scripting
- **Statusline integration** — `tokenburn collect` pipes into your Claude Code statusline for zero-API-call data
- **SQLite-backed history** — trend data and chart history persisted locally
- **Multi-profile support** — isolated data per Claude Code profile via `CLAUDE_CONFIG_DIR`

## Installation

```sh
cargo install --path .
```

Or build from source:

```sh
git clone ...
cd tokenburn
cargo build --release
# binary at target/release/tokenburn
```

Requires Claude Code to be installed and logged in (`claude`) before first use.

## Usage

```sh
# Full TUI (auto-detect limit type)
tokenburn

# Specific limit views
tokenburn session       # 5-hour rolling session
tokenburn weekly        # 7-day all models
tokenburn weekly-sonnet # 7-day Sonnet only
tokenburn monthly       # Monthly credits (enterprise)

# Compact single-line for tmux / status bars
tokenburn --compact
# → Session: 🔥 45% (2h14m) · Weekly: 🧊 12%

# JSON output for scripting
tokenburn --json

# Single snapshot, no live updates
tokenburn --once

# Custom refresh intervals
tokenburn --interval 10 --poll-interval 120
```

Press `q` or `Esc` to exit the TUI.

## Statusline integration

`tokenburn collect` reads the `rate_limits` JSON that Claude Code already emits to its statusline, saves it to SQLite, and passes stdin through unchanged. This avoids hitting the API on every statusline tick.

```json
// ~/.claude/settings.json
{
  "statusLine": {
    "command": "tokenburn collect | your-existing-statusline-command"
  }
}
```

`tokenburn collect` passes the original JSON through unchanged, so your existing statusline keeps working.

## Data storage

History is stored at `~/.tokenburn/history.db`. If `CLAUDE_CONFIG_DIR` is set (e.g. `~/.claude-work`), the database moves to `~/.tokenburn-work/history.db` so each profile gets isolated history.

## Credits

Inspired by [ccburn](https://github.com/JuanjoFuchs/ccburn) by JuanjoFuchs.
