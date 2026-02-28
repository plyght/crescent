use anyhow::{bail, Context, Result};
use clap::Parser;
use crescent::input::{self, MouseButton, ScrollDirection};
use crescent::renderer::RendererConfig;
use crescent::session::SessionManager;
use std::io::{self, BufRead, Write};
use std::sync::Arc;

#[derive(Parser)]
#[command(
    name = "crescent",
    about = "Playwright for terminals — interactive CLI"
)]
struct Cli {
    /// Command to launch immediately (e.g. "zsh", "python3", "btop")
    #[arg(long)]
    launch: Option<String>,

    /// Terminal columns
    #[arg(long, default_value = "80")]
    cols: u16,

    /// Terminal rows
    #[arg(long, default_value = "24")]
    rows: u16,

    /// Output directory for screenshots and frames (default: ~/.crescent/out)
    #[arg(long, short)]
    output: Option<String>,
}

fn default_output_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".crescent")
        .join("out")
}

struct ReplState {
    manager: Arc<SessionManager>,
    last_session: Option<String>,
    output_dir: std::path::PathBuf,
}

impl ReplState {
    fn resolve_session(&self, arg: Option<&str>) -> Result<String> {
        if let Some(id) = arg {
            if id.len() >= 4 {
                return Ok(id.to_string());
            }
        }
        self.last_session
            .clone()
            .context("no active session — run `launch` first")
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let manager = Arc::new(SessionManager::new());

    let output_dir = cli
        .output
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(default_output_dir);
    std::fs::create_dir_all(&output_dir)?;

    let mut state = ReplState {
        manager: manager.clone(),
        last_session: None,
        output_dir,
    };

    if let Some(ref cmd) = cli.launch {
        let id = manager.launch(cmd, cli.cols, cli.rows).await?;
        println!("session: {id}");
        state.last_session = Some(id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        print!("> ");
        io::stdout().flush()?;
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (cmd, rest) = trimmed.split_once(' ').unwrap_or((trimmed, ""));
        let rest_parts: Vec<&str> = if rest.is_empty() { vec![] } else { vec![rest] };

        let result = match cmd {
            "launch" | "l" => cmd_launch(&mut state, &rest_parts).await,
            "type" | "t" => cmd_type(&state, &rest_parts).await,
            "key" | "k" => cmd_key(&state, &rest_parts).await,
            "click" => cmd_click(&state, &rest_parts).await,
            "scroll" => cmd_scroll(&state, &rest_parts).await,
            "resize" => cmd_resize(&state, &rest_parts).await,
            "text" | "g" => cmd_text(&state, &rest_parts).await,
            "grid" => cmd_grid(&state, &rest_parts).await,
            "screenshot" | "ss" => cmd_screenshot(&state, &rest_parts).await,
            "wait" | "w" => cmd_wait(&state, &rest_parts).await,
            "idle" | "i" => cmd_idle(&state, &rest_parts).await,
            "stable" | "s" => cmd_stable(&state, &rest_parts).await,
            "record" | "rec" => cmd_record(&state, &rest_parts).await,
            "frames" => cmd_frames(&state, &rest_parts).await,
            "clean" => cmd_clean(&state),
            "sleep" => cmd_sleep(&rest_parts).await,
            "list" | "ls" => cmd_list(&state).await,
            "close" | "c" => cmd_close(&mut state, &rest_parts).await,
            "help" | "h" | "?" => {
                print_help();
                Ok(())
            }
            "quit" | "q" | "exit" => break,
            _ => {
                eprintln!("unknown command: {cmd} — type `help` for usage");
                Ok(())
            }
        };

        if let Err(e) = result {
            eprintln!("error: {e}");
        }
    }

    Ok(())
}

async fn cmd_launch(state: &mut ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ").trim().to_string();

    if raw.is_empty() {
        let id = state.manager.launch("zsh", 80, 24).await?;
        println!("session: {id}");
        state.last_session = Some(id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        return Ok(());
    }

    // Check if the last two whitespace-separated tokens are numbers (cols rows).
    // Everything before them is the command.
    let words: Vec<&str> = raw.split_whitespace().collect();
    let (command, cols, rows) = if words.len() >= 3 {
        let maybe_rows: Result<u16, _> = words[words.len() - 1].parse();
        let maybe_cols: Result<u16, _> = words[words.len() - 2].parse();
        if let (Ok(c), Ok(r)) = (maybe_cols, maybe_rows) {
            let cmd_end = raw.rfind(words[words.len() - 2]).unwrap_or(raw.len());
            let cmd = raw[..cmd_end].trim();
            (cmd.to_string(), c, r)
        } else {
            (raw.clone(), 80, 24)
        }
    } else {
        (raw.clone(), 80, 24)
    };

    // Strip surrounding quotes if present
    let command = command
        .strip_prefix('"')
        .unwrap_or(&command)
        .strip_suffix('"')
        .unwrap_or(&command);

    let id = state.manager.launch(command, cols, rows).await?;
    println!("session: {id}");
    state.last_session = Some(id);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    Ok(())
}

async fn cmd_type(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, text) = split_sid_and_rest(state, &raw)?;
    let session = state.manager.get(&sid).await?;
    session.type_text(&text)?;
    println!("ok");
    Ok(())
}

async fn cmd_key(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let key_parts: Vec<&str> = rest.split_whitespace().collect();
    if key_parts.is_empty() {
        bail!("usage: key [session] <keyname> [modifiers...]");
    }

    let key = input::parse_key(key_parts[0])
        .ok_or_else(|| anyhow::anyhow!("unknown key: {}", key_parts[0]))?;
    let mod_strs: Vec<String> = key_parts[1..].iter().map(|s| s.to_string()).collect();
    let mods = input::parse_modifiers(&mod_strs);

    let session = state.manager.get(&sid).await?;
    session.send_key(&key, &mods)?;
    println!("ok");
    Ok(())
}

async fn cmd_click(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("usage: click [session] <row> <col> [button]");
    }

    let row: u16 = parts[0].parse().context("bad row")?;
    let col: u16 = parts[1].parse().context("bad col")?;
    let button = match parts.get(2).copied().unwrap_or("left") {
        "left" | "l" => MouseButton::Left,
        "middle" | "m" => MouseButton::Middle,
        "right" | "r" => MouseButton::Right,
        other => bail!("unknown button: {other}"),
    };

    let session = state.manager.get(&sid).await?;
    session.click(row, col, button)?;
    println!("ok");
    Ok(())
}

async fn cmd_scroll(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        bail!("usage: scroll [session] <up|down> [amount]");
    }

    let dir = match parts[0] {
        "up" | "u" => ScrollDirection::Up,
        "down" | "d" => ScrollDirection::Down,
        other => bail!("unknown direction: {other} (use up/down)"),
    };
    let amount: u16 = parts.get(1).unwrap_or(&"3").parse().context("bad amount")?;

    let session = state.manager.get(&sid).await?;
    session.scroll(dir, amount)?;
    println!("ok");
    Ok(())
}

async fn cmd_resize(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 2 {
        bail!("usage: resize [session] <cols> <rows>");
    }

    let cols: u16 = parts[0].parse().context("bad cols")?;
    let rows: u16 = parts[1].parse().context("bad rows")?;

    let session = state.manager.get(&sid).await?;
    session.resize(cols, rows)?;
    println!("ok ({cols}x{rows})");
    Ok(())
}

async fn cmd_text(state: &ReplState, args: &[&str]) -> Result<()> {
    let sid = state.resolve_session(args.first().copied())?;
    let session = state.manager.get(&sid).await?;
    let grid = session.grid()?;
    let text = grid.text_content();
    for line in text.lines() {
        println!("{line}");
    }
    Ok(())
}

async fn cmd_grid(state: &ReplState, args: &[&str]) -> Result<()> {
    let sid = state.resolve_session(args.first().copied())?;
    let session = state.manager.get(&sid).await?;
    let grid = session.grid()?;
    let json = serde_json::to_string_pretty(&grid)?;
    println!("{json}");
    Ok(())
}

async fn cmd_screenshot(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let name = rest.trim();
    let path = if name.is_empty() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        state.output_dir.join(format!("ss_{ts}.png"))
    } else {
        state.output_dir.join(name)
    };

    let session = state.manager.get(&sid).await?;
    let config = RendererConfig::default();
    let png = session.screenshot(&config)?;
    std::fs::write(&path, &png).with_context(|| format!("failed to write {}", path.display()))?;
    println!(
        "saved: {} ({:.1}KB)",
        path.display(),
        png.len() as f64 / 1024.0
    );
    Ok(())
}

async fn cmd_wait(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.splitn(2, ' ').collect();
    if parts.is_empty() || parts[0].is_empty() {
        bail!("usage: wait [session] <pattern> [timeout_ms]");
    }

    let pattern = parts[0];
    let timeout: Option<u64> = parts.get(1).and_then(|s| s.trim().parse().ok());

    let session = state.manager.get(&sid).await?;
    let matched = session.wait_for(pattern, timeout).await?;
    println!("matched: {matched}");
    Ok(())
}

async fn cmd_idle(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let quiet: u64 = parts
        .first()
        .unwrap_or(&"500")
        .parse()
        .context("bad quiet_ms")?;
    let timeout: Option<u64> = parts.get(1).and_then(|s| s.parse().ok());

    let session = state.manager.get(&sid).await?;
    let idle = session.wait_for_idle(quiet, timeout).await?;
    println!("idle: {idle}");
    Ok(())
}

async fn cmd_stable(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let stable: u64 = parts
        .first()
        .unwrap_or(&"500")
        .parse()
        .context("bad stable_ms")?;
    let timeout: Option<u64> = parts.get(1).and_then(|s| s.parse().ok());

    let session = state.manager.get(&sid).await?;
    let result = session.wait_for_stable(stable, timeout).await?;
    println!("stable: {result}");
    Ok(())
}

async fn cmd_record(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;
    let subcmd = rest.trim();

    let session = state.manager.get(&sid).await?;

    match subcmd {
        "start" | "" => {
            session.record_start();
            println!("recording started");
        }
        "stop" => {
            let frames = session.record_stop();
            println!("recording stopped: {} frames captured", frames.len());
        }
        "status" => {
            let recording = session.is_recording();
            let count = session.frame_count();
            println!(
                "recording: {}, frames: {}",
                if recording { "on" } else { "off" },
                count
            );
        }
        _ => bail!("usage: record [start|stop|status]"),
    }
    Ok(())
}

async fn cmd_frames(state: &ReplState, args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let (sid, rest) = split_sid_and_rest(state, &raw)?;

    let name = rest.trim();
    let dir = if name.is_empty() {
        state.output_dir.join("frames")
    } else {
        state.output_dir.join(name)
    };

    let session = state.manager.get(&sid).await?;
    let frames = session.record_stop();

    if frames.is_empty() {
        println!("no frames captured");
        return Ok(());
    }

    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;

    let first_ts = frames[0].timestamp_ms;
    for (i, frame) in frames.iter().enumerate() {
        let relative_ms = frame.timestamp_ms - first_ts;
        let text_path = dir.join(format!("{i:04}_{relative_ms}ms.txt"));
        std::fs::write(&text_path, &frame.text)?;
    }

    let duration_ms = frames.last().unwrap().timestamp_ms - first_ts;
    println!(
        "saved {} frames to {}/ ({:.1}s)",
        frames.len(),
        dir.display(),
        duration_ms as f64 / 1000.0
    );
    Ok(())
}

fn cmd_clean(state: &ReplState) -> Result<()> {
    let dir = &state.output_dir;
    if !dir.exists() {
        println!("nothing to clean ({})", dir.display());
        return Ok(());
    }

    fn count_entries(path: &std::path::Path) -> usize {
        std::fs::read_dir(path)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| {
                        if e.path().is_dir() {
                            1 + count_entries(&e.path())
                        } else {
                            1
                        }
                    })
                    .sum()
            })
            .unwrap_or(0)
    }
    let count = count_entries(dir);

    if count == 0 {
        println!("nothing to clean ({})", dir.display());
        return Ok(());
    }

    std::fs::remove_dir_all(dir)?;
    std::fs::create_dir_all(dir)?;
    println!("cleaned {} files from {}", count, dir.display());
    Ok(())
}

async fn cmd_sleep(args: &[&str]) -> Result<()> {
    let raw = args.join(" ");
    let ms: u64 = raw.trim().parse().context("usage: sleep <ms>")?;
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
    Ok(())
}

async fn cmd_list(state: &ReplState) -> Result<()> {
    let ids = state.manager.list().await;
    if ids.is_empty() {
        println!("no sessions");
    } else {
        for id in &ids {
            let session = state.manager.get(id).await?;
            let status = if session.is_alive() { "alive" } else { "dead" };
            let marker = if state.last_session.as_deref() == Some(id) {
                " *"
            } else {
                ""
            };
            println!("{id} ({status}){marker}");
        }
    }
    Ok(())
}

async fn cmd_close(state: &mut ReplState, args: &[&str]) -> Result<()> {
    let sid = state.resolve_session(args.first().copied())?;
    state.manager.close(&sid).await?;
    if state.last_session.as_deref() == Some(&sid) {
        state.last_session = None;
    }
    println!("closed: {sid}");
    Ok(())
}

/// Tries to split "maybe-session-id rest..." from input.
/// If the first word looks like a UUID, use it as session ID.
/// Otherwise, use the last active session and treat entire input as the rest.
fn split_sid_and_rest(state: &ReplState, input: &str) -> Result<(String, String)> {
    let input = input.trim();
    if let Some((first, rest)) = input.split_once(' ') {
        if looks_like_session_id(first) {
            return Ok((first.to_string(), rest.to_string()));
        }
    } else if looks_like_session_id(input) {
        let sid = input.to_string();
        return Ok((sid, String::new()));
    }
    let sid = state.resolve_session(None)?;
    Ok((sid, input.to_string()))
}

fn looks_like_session_id(s: &str) -> bool {
    s.len() >= 36 && s.contains('-')
}

fn print_help() {
    println!(
        "\
commands (session ID optional if only one session):

  launch [cmd] [cols rows]  launch a terminal (default: zsh 80 24)
  type [sid] <text>         type text into the terminal
  key [sid] <name> [mods]   send a key (Enter, Tab, Up, etc.) with optional ctrl/alt/shift
  click [sid] <row> <col>   mouse click at row,col
  scroll [sid] <up|down> [n] scroll by n lines (default 3)
  resize [sid] <cols> <rows> resize terminal
  text [sid]                print terminal text content
  grid [sid]                print full grid JSON
  screenshot [sid] [path]   save PNG screenshot (default: screenshot.png)
  wait [sid] <pattern> [ms] wait for regex pattern (default timeout: 10s)
  idle [sid] [quiet] [ms]   wait until PTY output stops (for shells/builds)
  stable [sid] [ms] [timeout] wait until visible text stops changing (for TUIs)
  record [sid] start|stop   start/stop frame recording
  frames [sid] [dir]        stop recording and save frames to dir (default: ./frames)
  clean                     wipe all screenshots and frames from output dir
  sleep <ms>                pause for N milliseconds
  list                      list all sessions
  close [sid]               close a session
  help                      show this help
  quit                      exit

shortcuts: l=launch t=type k=key g=text ss=screenshot w=wait i=idle s=stable ls=list c=close q=quit"
    );
}
