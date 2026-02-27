use std::sync::Arc;

use crescent::input::{self, MouseButton, ScrollDirection};
use crescent::renderer::RendererConfig;
use crescent::session::SessionManager;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tool parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
struct LaunchParams {
    #[schemars(description = "The command to run (e.g. \"bash\", \"btop\", \"lazygit\")")]
    command: String,
    #[schemars(description = "Terminal width in columns (default 80)")]
    cols: Option<u16>,
    #[schemars(description = "Terminal height in rows (default 24)")]
    rows: Option<u16>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SessionId {
    #[schemars(description = "The session ID returned by terminal_launch")]
    session_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ClickParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "Row (0-indexed)")]
    row: u16,
    #[schemars(description = "Column (0-indexed)")]
    col: u16,
    #[schemars(description = "Mouse button: \"left\", \"middle\", or \"right\" (default \"left\")")]
    button: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TypeParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "Text to type into the terminal")]
    text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct KeyParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "Key name: Enter, Tab, Escape, Up, Down, Left, Right, Backspace, Home, End, PageUp, PageDown, Delete, F1-F12, or a single character")]
    key: String,
    #[schemars(description = "Modifier keys: \"ctrl\", \"alt\", \"shift\"")]
    modifiers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ResizeParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "New width in columns")]
    cols: u16,
    #[schemars(description = "New height in rows")]
    rows: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ScrollParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "Direction: \"up\" or \"down\"")]
    direction: String,
    #[schemars(description = "Number of scroll lines")]
    amount: u16,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WaitForParams {
    #[schemars(description = "The session ID")]
    session_id: String,
    #[schemars(description = "Regex pattern to search for in the terminal grid text")]
    pattern: String,
    #[schemars(description = "Timeout in milliseconds (default 10000)")]
    timeout_ms: Option<u64>,
}

// ---------------------------------------------------------------------------
// Grid response types (for structured output)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct GridResponse {
    cells: Vec<Vec<CellResponse>>,
    cursor: CursorResponse,
    size: SizeResponse,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct CellResponse {
    #[serde(rename = "char")]
    ch: String,
    fg: [u8; 3],
    bg: [u8; 3],
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct CursorResponse {
    row: u16,
    col: u16,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct SizeResponse {
    rows: u16,
    cols: u16,
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct CrescentServer {
    sessions: Arc<SessionManager>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl CrescentServer {
    fn new() -> Self {
        Self {
            sessions: Arc::new(SessionManager::new()),
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "terminal_launch",
        description = "Launch a new terminal session with the given command. Returns a session_id for use with other tools."
    )]
    async fn terminal_launch(
        &self,
        Parameters(params): Parameters<LaunchParams>,
    ) -> Result<CallToolResult, McpError> {
        let cols = params.cols.unwrap_or(80);
        let rows = params.rows.unwrap_or(24);
        let session_id = self
            .sessions
            .launch(&params.command, cols, rows)
            .await
            .map_err(|e| mcp_err(&format!("launch failed: {e}")))?;

        // Brief pause so the shell/app has time to render initial output
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Session launched: {session_id}"
        ))]))
    }

    #[tool(
        name = "terminal_screenshot",
        description = "Capture a PNG screenshot of the terminal. Returns the image as base64."
    )]
    async fn terminal_screenshot(
        &self,
        Parameters(params): Parameters<SessionId>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let config = RendererConfig::default();
        let png_bytes = session
            .screenshot(&config)
            .map_err(|e| mcp_err(&format!("screenshot failed: {e}")))?;

        let b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &png_bytes,
        );

        Ok(CallToolResult::success(vec![Content {
            raw: RawContent::Image(RawImageContent {
                data: b64,
                mime_type: "image/png".to_string(),
                meta: None,
            }),
            annotations: None,
        }]))
    }

    #[tool(
        name = "terminal_grid",
        description = "Get the structured grid state of the terminal: every cell's character, colors, and attributes, plus cursor position and terminal size. Cheaper than a screenshot for text analysis."
    )]
    async fn terminal_grid(
        &self,
        Parameters(params): Parameters<SessionId>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let grid = session.grid().map_err(|e| mcp_err(&e.to_string()))?;

        let resp = GridResponse {
            cells: grid
                .cells
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| CellResponse {
                            ch: c.ch.clone(),
                            fg: [c.fg.r, c.fg.g, c.fg.b],
                            bg: [c.bg.r, c.bg.g, c.bg.b],
                            bold: c.bold,
                            italic: c.italic,
                            underline: c.underline,
                        })
                        .collect()
                })
                .collect(),
            cursor: CursorResponse {
                row: grid.cursor.row,
                col: grid.cursor.col,
            },
            size: SizeResponse {
                rows: grid.size.rows,
                cols: grid.size.cols,
            },
        };

        let text = grid.text_content();
        let json =
            serde_json::to_string_pretty(&resp).map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![
            Content::text(format!("=== Terminal Text ===\n{text}")),
            Content::text(format!("=== Grid JSON ===\n{json}")),
        ]))
    }

    #[tool(
        name = "terminal_click",
        description = "Send a mouse click at the given row and column (0-indexed). Uses SGR mouse encoding."
    )]
    async fn terminal_click(
        &self,
        Parameters(params): Parameters<ClickParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let button = match params.button.as_deref().unwrap_or("left") {
            "left" => MouseButton::Left,
            "middle" => MouseButton::Middle,
            "right" => MouseButton::Right,
            other => return Err(mcp_err(&format!("unknown button: {other}"))),
        };

        session
            .click(params.row, params.col, button)
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(
        name = "terminal_type",
        description = "Type text into the terminal session. The text is sent as raw UTF-8 bytes."
    )]
    async fn terminal_type(
        &self,
        Parameters(params): Parameters<TypeParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        session
            .type_text(&params.text)
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(
        name = "terminal_key",
        description = "Send a key press to the terminal. Key names: Enter, Tab, Escape, Up, Down, Left, Right, Backspace, Home, End, PageUp, PageDown, Delete, Insert, F1-F12, or a single character. Modifiers: ctrl, alt, shift."
    )]
    async fn terminal_key(
        &self,
        Parameters(params): Parameters<KeyParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let key = input::parse_key(&params.key)
            .ok_or_else(|| mcp_err(&format!("unknown key: {}", params.key)))?;
        let mods = input::parse_modifiers(&params.modifiers.unwrap_or_default());

        session
            .send_key(&key, &mods)
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(
        name = "terminal_resize",
        description = "Resize the terminal to the given dimensions."
    )]
    async fn terminal_resize(
        &self,
        Parameters(params): Parameters<ResizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        session
            .resize(params.cols, params.rows)
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(
        name = "terminal_scroll",
        description = "Scroll the terminal up or down by the given number of lines."
    )]
    async fn terminal_scroll(
        &self,
        Parameters(params): Parameters<ScrollParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let dir = match params.direction.as_str() {
            "up" => ScrollDirection::Up,
            "down" => ScrollDirection::Down,
            other => return Err(mcp_err(&format!("unknown direction: {other}"))),
        };

        session
            .scroll(dir, params.amount)
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }

    #[tool(
        name = "terminal_wait_for",
        description = "Wait for a regex pattern to appear in the terminal grid text. Returns whether the pattern was found before the timeout."
    )]
    async fn terminal_wait_for(
        &self,
        Parameters(params): Parameters<WaitForParams>,
    ) -> Result<CallToolResult, McpError> {
        let session = self
            .sessions
            .get(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        let matched = session
            .wait_for(&params.pattern, params.timeout_ms)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "{{\"matched\": {matched}}}"
        ))]))
    }

    #[tool(
        name = "terminal_close",
        description = "Close a terminal session and clean up resources."
    )]
    async fn terminal_close(
        &self,
        Parameters(params): Parameters<SessionId>,
    ) -> Result<CallToolResult, McpError> {
        self.sessions
            .close(&params.session_id)
            .await
            .map_err(|e| mcp_err(&e.to_string()))?;

        Ok(CallToolResult::success(vec![Content::text("ok")]))
    }
}

#[tool_handler]
impl ServerHandler for CrescentServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Crescent: Playwright for terminals. Launch terminal sessions, \
                 inspect their visual state (grid + screenshots), and interact \
                 via keyboard, mouse, and scroll input."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "crescent".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: None,
                description: Some("Playwright for terminals".into()),
                icons: None,
                website_url: None,
            },
            ..Default::default()
        }
    }
}

fn mcp_err(msg: &str) -> McpError {
    McpError {
        code: rmcp::model::ErrorCode::INTERNAL_ERROR,
        message: msg.to_string().into(),
        data: None,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("crescent MCP server starting");

    let server = CrescentServer::new()
        .serve(rmcp::transport::stdio())
        .await?;

    server.waiting().await?;
    Ok(())
}
