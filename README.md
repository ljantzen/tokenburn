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

### macOS Setup

If you download a pre-built binary on macOS, you may see "Cannot open because the developer cannot be verified." Run this once to allow execution:

```sh
xattr -d com.apple.quarantine ./tokenburn
```

Then you can run the binary normally.

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

Claude Code calls your `statusLine` command after each assistant response, piping a JSON blob that includes rate limit data. `tokenburn collect` reads that JSON, saves the usage snapshot to SQLite, and passes stdin through to stdout unchanged — so your existing statusline keeps working and the history DB grows automatically without any extra API calls.

### Basic setup

If you don't have an existing statusline command, set `tokenburn collect` directly:

```json
// ~/.claude/settings.json
{
  "statusLine": {
    "type": "command",
    "command": "tokenburn collect"
  }
}
```

With no other consumer of the output this just silently records usage in the background. Run `tokenburn` at any time to see the chart with historical data.

### With an existing statusline command

Because `tokenburn collect` passes stdin through to stdout unchanged, you can insert it anywhere in a pipeline:

```json
{
  "statusLine": {
    "type": "command",
    "command": "tokenburn collect | your-existing-statusline-command"
  }
}
```

### Displaying compact output in your shell prompt

To also show live usage in your terminal prompt, pipe collect's output into `tokenburn --compact`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "tokenburn collect | tokenburn --compact"
  }
}
```

This prints a single line like `Session: 🔥 45% (2h14m) · Weekly: 🧊 12%` that your shell prompt or tmux statusline can pick up.

### Multi-profile

If you run multiple Claude Code profiles via `CLAUDE_CONFIG_DIR`, each profile gets its own isolated database. Set the env var in the command so tokenburn writes to the right place:

```json
{
  "statusLine": {
    "type": "command",
    "command": "CLAUDE_CONFIG_DIR=~/.claude-work tokenburn collect"
  }
}
```

History is stored at `~/.tokenburn-work/history.db` (derived from the config dir name).

## Data storage

History is stored at `~/.tokenburn/history.db`. If `CLAUDE_CONFIG_DIR` is set (e.g. `~/.claude-work`), the database moves to `~/.tokenburn-work/history.db` so each profile gets isolated history.

## Credits

Inspired by [ccburn](https://github.com/JuanjoFuchs/ccburn) by JuanjoFuchs.
