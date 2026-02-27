<div align="center">
    <h3>Crescent</h3>
    <p>Playwright for terminals — programmatic terminal control and inspection for AI agents</p>
    <br/>
    <br/>
</div>

A Rust library and MCP server that gives AI agents full semantic access to terminal sessions. Launch processes, send keystrokes and mouse events, read structured grid state, take screenshots, and wait for output patterns — all through a clean async API or the Model Context Protocol.

## Features

- **Session Management**: Launch, resize, and close multiple concurrent terminal sessions backed by real PTYs
- **Structured Grid Inspection**: Read the terminal as a 2D grid of cells with characters, colors, and attributes (bold, italic, underline)
- **Screenshot Rendering**: Generate PNG screenshots with automatic system font detection
- **Full Input Support**: Type text, send keys with modifiers (ctrl/alt/shift), click (left/middle/right), and scroll
- **Pattern Waiting**: Async wait for regex patterns to appear in terminal output with configurable timeouts
- **MCP Server**: 10-tool MCP server over stdio for direct AI agent integration
- **Cross-Platform**: Works on macOS and Linux via `portable-pty`

## Install

```bash
# Build from source
git clone https://github.com/plyght/crescent.git
cd crescent
cargo build --release
```

The MCP server binary will be at `target/release/crescent-mcp`.

## Usage

### As an MCP Server

Crescent exposes 10 tools over stdio transport. Point your MCP client at the binary:

```json
{
  "command": "/path/to/crescent-mcp"
}
```

Tools:

| Tool | Description |
|------|-------------|
| `terminal_launch` | Start a session (command, cols, rows) |
| `terminal_type` | Type UTF-8 text into a session |
| `terminal_key` | Send a key press with optional modifiers |
| `terminal_click` | Send a mouse click at row/col |
| `terminal_scroll` | Scroll up or down by N lines |
| `terminal_resize` | Resize a session |
| `terminal_grid` | Get structured cell data, cursor, and dimensions |
| `terminal_screenshot` | Capture a base64-encoded PNG |
| `terminal_wait_for` | Block until a regex pattern appears |
| `terminal_close` | End a session |

### As a Library

```rust
use crescent::{Session, SessionManager};

let manager = SessionManager::new();
let id = manager.launch("bash", 80, 24).await?;
let session = manager.get(&id).await?;

session.type_text("echo hello\n")?;
session.wait_for("hello", Some(5000)).await?;

let grid = session.grid()?;
let png = session.screenshot(&RendererConfig::default())?;
```

## Configuration

| Variable | Purpose |
|----------|---------|
| `CRESCENT_FONT` | Path to a custom monospace font (.ttf/.otf). Falls back to system fonts (Menlo, SF Mono, DejaVu Sans Mono, etc.) |
| `RUST_LOG` | Controls log verbosity for the MCP server (default: `info`, logs to stderr) |

## Architecture

```
crates/
  crescent/          Core library
    src/
      session.rs     PTY lifecycle, background reader, session manager
      grid.rs        2D grid extraction from vt100 screen state
      input.rs       Key/mouse/scroll encoding to escape sequences
      renderer.rs    PNG rendering with system font detection
      wait.rs        Async regex pattern polling
  crescent-mcp/      MCP server
    src/
      main.rs        Tool definitions, stdio transport, session routing
```

## Development

```bash
cargo build
cargo test
```

Requires Rust 1.70+. Key dependencies: portable-pty, vt100, rmcp, ab_glyph, image, tokio, ratatui.

## License

MIT License
