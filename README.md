# Crescent

**Playwright for terminals.** An MCP server that gives AI agents semantic access to terminal sessions — structured grid state, PNG screenshots, and input injection (keyboard, mouse, scroll).

## Why

AI agents interact with terminals via raw stdin/stdout text streams. When a TUI is running (btop, lazygit, vim, etc.), the AI gets a wall of ANSI escape sequences with no understanding of what's visually on screen. Crescent solves this by running a headless terminal emulator that exposes both **structured grid data** (cheap, fast, searchable) and **visual screenshots** (for spatial reasoning) — plus full input injection.

## Architecture

```
┌─────────────────────────────────────┐
│           MCP Server (rmcp)         │
│  10 tools over stdio JSON-RPC       │
├─────────────────────────────────────┤
│         crescent library            │
│  ┌──────────┐  ┌────────────────┐   │
│  │ SessionMgr│  │  Per-Session   │   │
│  │ HashMap   │  │ ┌─PTY─────┐   │   │
│  │ <id,      │──│ │portable │   │   │
│  │  Session> │  │ │  -pty   │   │   │
│  └──────────┘  │ └────┬────┘   │   │
│                │ ┌────▼────┐   │   │
│                │ │  vt100  │   │   │
│                │ │ Parser  │   │   │
│                │ └────┬────┘   │   │
│                │ ┌────▼────┐   │   │
│                │ │  Grid   │   │   │
│                │ │ + Render│   │   │
│                │ └─────────┘   │   │
│                └────────────────┘   │
└─────────────────────────────────────┘
```

## Tools

| Tool | Description |
|------|-------------|
| `terminal_launch` | Spawn a command in a new PTY session |
| `terminal_screenshot` | Capture a PNG screenshot (base64) |
| `terminal_grid` | Get structured cell grid + plain text |
| `terminal_click` | SGR mouse click at (row, col) |
| `terminal_type` | Type raw text into the terminal |
| `terminal_key` | Send a named key (Enter, Up, Ctrl+C, F1, etc.) |
| `terminal_resize` | Resize the terminal dimensions |
| `terminal_scroll` | Scroll up/down by N lines |
| `terminal_wait_for` | Wait for a regex pattern in grid text |
| `terminal_close` | Close a session and clean up |

## Quick Start

### Build

```bash
cargo build --release
```

The binary is at `target/release/crescent-mcp`.

### Add to Cursor

Add to your Cursor MCP config (`.cursor/mcp.json` in your project or `~/.cursor/mcp.json` globally):

```json
{
  "mcpServers": {
    "crescent": {
      "command": "/absolute/path/to/crescent-mcp"
    }
  }
}
```

Or using cargo:

```json
{
  "mcpServers": {
    "crescent": {
      "command": "cargo",
      "args": ["run", "--release", "-p", "crescent-mcp", "--manifest-path", "/absolute/path/to/crescent/Cargo.toml"]
    }
  }
}
```

### Font Configuration

Screenshots require a monospace font. Crescent auto-detects system fonts:

- **macOS**: Menlo, SF Mono
- **Linux**: DejaVu Sans Mono, Liberation Mono, Ubuntu Mono

Override with the `CRESCENT_FONT` environment variable:

```json
{
  "mcpServers": {
    "crescent": {
      "command": "/path/to/crescent-mcp",
      "env": {
        "CRESCENT_FONT": "/path/to/MyFont.ttf"
      }
    }
  }
}
```

## Usage Examples

### Launch a shell and run a command

```
terminal_launch(command: "bash")          → session_id
terminal_type(session_id, text: "ls -la\n")
terminal_wait_for(session_id, pattern: "\\$")  → wait for prompt
terminal_grid(session_id)                 → structured output
```

### Interact with a TUI

```
terminal_launch(command: "btop", cols: 120, rows: 40)
terminal_wait_for(session_id, pattern: "CPU")
terminal_screenshot(session_id)           → PNG of btop
terminal_key(session_id, key: "q")        → quit btop
```

### Mouse interaction

```
terminal_launch(command: "lazygit")
terminal_wait_for(session_id, pattern: ".*")
terminal_click(session_id, row: 5, col: 10, button: "left")
terminal_scroll(session_id, direction: "down", amount: 3)
```

## Project Structure

```
crescent/
├── Cargo.toml                  # workspace
├── crates/
│   ├── crescent/               # core library
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── grid.rs         # Cell, Grid, color conversion
│   │       ├── input.rs        # key/mouse/scroll escape sequences
│   │       ├── renderer.rs     # Grid → PNG
│   │       ├── session.rs      # Session, SessionManager
│   │       └── wait.rs         # wait_for polling
│   └── crescent-mcp/           # MCP server binary
│       └── src/
│           └── main.rs
└── README.md
```

## Design Decisions

- **vt100 over alacritty_terminal**: Lighter, sufficient for most TUIs. Can swap later if needed.
- **Multi-session from start**: Every tool takes a `session_id`. No extra complexity vs single-session.
- **Grid + screenshots**: Dual-modality — AI gets cheap text data AND visual screenshots when spatial reasoning is needed.
- **Blocking PTY I/O in background threads**: `portable-pty` is synchronous; a dedicated reader thread feeds the vt100 parser continuously.

## Limitations (MVP)

- macOS + Linux only (no Windows)
- Mouse click sends SGR encoding regardless of whether the TUI has enabled mouse support
- `wait_for` uses regex pattern matching only (no output-idle heuristic yet)
- No tmux control mode integration

## License

MIT
