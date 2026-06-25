# ZXTracker

Window focus time tracker for the [niri](https://github.com/YaLTeR/niri) Wayland compositor.

Tracks how long each application window is focused, detects idle time (AFK), supports manual labels/tags, and provides interactive multi-view analysis reports.

## Architecture

```
┌──────────────┐     Unix Socket      ┌──────────────────────┐
│  status      │◄───────────────────► │  daemon               │
│  (htop TUI)  │    JSON              │  ┌────────────────┐   │
└──────────────┘                      │  │ FocusTracker   │   │
                                      │  │ (event-stream  │   │
┌──────────────┐     Direct SQLite    │  │  → polling)    │   │
│  today       │◄───────────────────► │  └───────┬────────┘   │
│  report      │                      │  ┌───────▼────────┐   │
│  analyze     │                      │  │ SQLite DB      │   │
│  tag / label │                      │  └───────┬────────┘   │
└──────────────┘                      │  ┌───────▼────────┐   │
                                      │  │ IdleDetector   │   │
                                      │  │ (xprintidle    │   │
                                      │  │  → dbus → 0)   │   │
                                      │  └────────────────┘   │
                                      └──────────────────────┘
```

### Daemon + Client

- **Daemon** (`track daemon`): runs in background, collects focus data every tick via niri's event stream (push mode) with automatic fallback to 1s polling
- **Client**: all other subcommands either talk to the daemon via Unix socket for real-time data, or read SQLite directly for historical queries

### Focus Tracking Strategy

1. **Primary**: subscribe to `niri msg event-stream` — zero-overhead push events, only fires on focus change
2. **Fallback**: poll `niri msg --json focused-window` every second and diff with previous state

### Idle Detection (5 min default)

1. `xprintidle` (X11 / XWayland)
2. `dbus-send` to `org.gnome.ScreenSaver.GetSessionIdleTime`
3. Returns 0 if neither is available (idle detection disabled)

### Data Model

```sql
-- Core tracking: one row per continuous focus period
CREATE TABLE focus_sessions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    date           TEXT NOT NULL,       -- "2026-06-23"
    app_id         TEXT NOT NULL,       -- e.g. "firefox", "code-oss"
    started_at     INTEGER NOT NULL,    -- Unix timestamp
    ended_at       INTEGER,             -- NULL = still active
    duration_secs  INTEGER DEFAULT 0
);

-- Tag system
CREATE TABLE tags (name TEXT PRIMARY KEY);
CREATE TABLE app_tags (
    app_id   TEXT NOT NULL,
    tag_name TEXT NOT NULL REFERENCES tags(name),
    PRIMARY KEY (app_id, tag_name)
);
```

All analytics (duration, activation count, heatmap, daily first-seen) are derived from `focus_sessions` via SQL aggregation.

### Directory Layout

```
src/
  main.rs           CLI entry point (clap derive)
  daemon.rs         Daemon main loop with state machine
  db.rs             SQLite layer (schema, CRUD, analytics queries)
  tracker/
    mod.rs          FocusTracker trait
    niri.rs         NiriTracker (event-stream + polling fallback)
  idle/
    mod.rs          Linux idle detection (xprintidle → dbus → 0)
  ipc/
    mod.rs          IPC data types
    unix.rs         Unix socket server
  render.rs         TUI rendering (alternate buffer, htop-style)
  analyze.rs        Interactive 4-view analysis TUI
```

### Extensibility

The `FocusTracker` trait is designed for multi-compositor / cross-platform support:

```rust
pub trait FocusTracker: Send {
    fn start(&mut self, tx: Sender<FocusEvent>) -> anyhow::Result<()>;
    fn stop(&mut self);
}
```

Current implementation: `NiriTracker`.  
Extension points: `SwayTracker`, `X11Tracker`, `WinTracker`, `GNOMETracker`.

## Installation

```bash
git clone <repo>
cd ZXTracker
cargo build --release
```

Binary is at `./target/release/track`.

### Dependencies

- **niri** Wayland compositor (for focus tracking)
- **xprintidle** (optional, for X11 idle detection)
- **dbus** (optional, for Wayland idle detection)

## Usage

### 1. Start the daemon

```bash
track daemon
```

Data stored at `~/.local/share/track/track.db`.  
Socket at `$XDG_RUNTIME_DIR/track.sock` or `/tmp/track.sock`.

### 2. Real-time dashboard

```bash
track status
```

Htop-style alternate buffer display. Shows:
- Current focused window (app_id + title)
- Idle status indicator `[IDLE]`
- Today's usage per app, sorted by time descending

Press `Ctrl+C` to exit.

### 3. Today's report

```bash
track today
```

### 4. Weekly / Monthly report

```bash
track report          # defaults to --week
track report --week
track report --month
```

### 5. Interactive analysis

```bash
track analyze         # defaults to 7 days
track analyze --days 14
```

Four views, switch with `←` `→` or `1-4`:

| View | Content |
|------|---------|
| **Duration** | Total time per app, sorted by duration |
| **Activations** | Focus-switch count per app, sorted by frequency |
| **Heatmap** | 2-hour bucket usage heatmap (top 6 apps), █/▓/▒/░ density |
| **Schedule** | Average daily first-seen time per app |

Press `q` or `Esc` to exit.

### 6. Tag system

```bash
# Tag an application
track tag firefox 摸鱼
track tag code-oss 写代码
track tag discord 摸鱼

# List all tags
track tags

# Query usage by label (today)
track label 摸鱼

# Query usage by label (this week)
track label 摸鱼 --week

# Remove a tag
track untag discord 摸鱼
```

## Output Examples

### `track today`

```
Today
------------------------------------------------------------
APP                                 TIME    TIMES
------------------------------------------------------------
  code-oss                      2h 30m 15s       5
  firefox                       1h 20m  8s       3
  alacritty                     0h 45m 22s       7
------------------------------------------------------------
  Total:                        4h 35m 45s
```

### `track label 摸鱼`

```
[摸鱼] label usage
--------------------------------------------------
  firefox                          1h 30m 22s
  discord                          0h 20m  5s
--------------------------------------------------
  Total:                           1h 50m 27s
```

### `track analyze --7d` (interactive TUI)

```
ZXTracker — 7-Day Analysis  |  ← → switch view  |  q quit
 [Duration]   Activations    Heatmap    Schedule
------------------------------------------------------------
APP                                  TIME  ACTIVATIONS
------------------------------------------------------------
  code-oss                       42h 15m         156
  firefox                        35h 10m          89
  alacritty                      12h 05m         234
```

```
           0-2 2-4 4-6 6-8 8-10 10-12 12-14 14-16 16-18 18-20 20-22 22-24
  firefox                        █    ██          ██   ████  ███  ██
  code-os                        ░    ██   ████   ████  ███  █
  steam                                                    ███  ████  █

  █ heavy  ▓ medium  ▒ light  ░ trace
```

## Data Storage

| Item | Path |
|------|------|
| SQLite database | `~/.local/share/track/track.db` |
| Unix socket | `$XDG_RUNTIME_DIR/track.sock` or `/tmp/track.sock` |

The database uses WAL journal mode for concurrent read access (daemon writes, client reads simultaneously).
