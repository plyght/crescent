use crate::grid::{self, Grid};
use crate::input::{self, Key, Modifiers, MouseButton, ScrollDirection};
use crate::renderer::{self, RendererConfig};
use crate::wait;
use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use uuid::Uuid;

#[derive(Clone)]
pub struct Frame {
    pub text: String,
    pub timestamp_ms: u64,
}

pub struct Recorder {
    enabled: bool,
    frames: VecDeque<Frame>,
    max_frames: usize,
    last_text: String,
}

impl Recorder {
    fn new(max_frames: usize) -> Self {
        Self {
            enabled: false,
            frames: VecDeque::new(),
            max_frames,
            last_text: String::new(),
        }
    }

    fn capture(&mut self, text: String, timestamp_ms: u64) {
        if !self.enabled || text == self.last_text {
            return;
        }
        self.last_text = text.clone();
        if self.frames.len() >= self.max_frames {
            self.frames.pop_front();
        }
        self.frames.push_back(Frame { text, timestamp_ms });
    }
}

pub struct Session {
    pub id: String,
    parser: Arc<RwLock<vt100::Parser>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    alive: Arc<AtomicBool>,
    last_output_epoch_ms: Arc<AtomicU64>,
    recorder: Arc<Mutex<Recorder>>,
}

impl Session {
    pub fn grid(&self) -> Result<Grid> {
        let parser = self
            .parser
            .read()
            .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        Ok(grid::extract_grid(parser.screen()))
    }

    pub fn write_bytes(&self, data: &[u8]) -> Result<()> {
        let mut w = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("lock poisoned: {e}"))?;
        w.write_all(data).context("failed to write to PTY")?;
        w.flush().context("failed to flush PTY")
    }

    pub fn type_text(&self, text: &str) -> Result<()> {
        self.write_bytes(text.as_bytes())
    }

    pub fn send_key(&self, key: &Key, modifiers: &Modifiers) -> Result<()> {
        let seq = input::key_to_escape(key, modifiers);
        self.write_bytes(&seq)
    }

    pub fn click(&self, row: u16, col: u16, button: MouseButton) -> Result<()> {
        let press = input::sgr_mouse_press(row, col, button);
        let release = input::sgr_mouse_release(row, col, button);
        self.write_bytes(&press)?;
        self.write_bytes(&release)
    }

    pub fn scroll(&self, direction: ScrollDirection, amount: u16) -> Result<()> {
        let (rows, cols) = {
            let parser = self.parser.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            parser.screen().size()
        };
        let center_row = rows / 2;
        let center_col = cols / 2;
        for _ in 0..amount {
            let ev = input::sgr_scroll(center_row, center_col, direction);
            self.write_bytes(&ev)?;
        }
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let master = self.master.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize PTY")?;
        let mut parser = self.parser.write().map_err(|e| anyhow::anyhow!("{e}"))?;
        parser.screen_mut().set_size(rows, cols);
        Ok(())
    }

    pub fn screenshot(&self, config: &RendererConfig) -> Result<Vec<u8>> {
        let g = self.grid()?;
        renderer::render_grid_to_png(&g, config)
    }

    pub async fn wait_for(&self, pattern: &str, timeout_ms: Option<u64>) -> Result<bool> {
        let parser = Arc::clone(&self.parser);
        wait::wait_for_pattern(pattern, timeout_ms, move || {
            let p = parser.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grid::extract_grid(p.screen()))
        })
        .await
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn last_output_ms(&self) -> u64 {
        self.last_output_epoch_ms.load(Ordering::Relaxed)
    }

    pub fn record_start(&self) {
        if let Ok(mut rec) = self.recorder.lock() {
            rec.frames.clear();
            rec.last_text.clear();
            rec.enabled = true;
        }
    }

    pub fn record_stop(&self) -> Vec<Frame> {
        if let Ok(mut rec) = self.recorder.lock() {
            rec.enabled = false;
            rec.frames.drain(..).collect()
        } else {
            Vec::new()
        }
    }

    pub fn is_recording(&self) -> bool {
        self.recorder.lock().map(|r| r.enabled).unwrap_or(false)
    }

    pub fn frame_count(&self) -> usize {
        self.recorder.lock().map(|r| r.frames.len()).unwrap_or(0)
    }

    /// Wait until no PTY output has been received for `quiet_ms` milliseconds.
    /// Best for non-TUI commands (shell, builds, scripts).
    pub async fn wait_for_idle(&self, quiet_ms: u64, timeout_ms: Option<u64>) -> Result<bool> {
        wait::wait_for_idle(quiet_ms, timeout_ms, Arc::clone(&self.last_output_epoch_ms)).await
    }

    /// Wait until the visible text content hasn't changed for `stable_ms`.
    /// Works with TUIs that constantly repaint — only tracks actual text changes.
    pub async fn wait_for_stable(&self, stable_ms: u64, timeout_ms: Option<u64>) -> Result<bool> {
        let parser = Arc::clone(&self.parser);
        wait::wait_for_stable(stable_ms, timeout_ms, move || {
            let p = parser.read().map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(grid::extract_grid(p.screen()))
        })
        .await
    }
}

pub struct SessionManager {
    sessions: tokio::sync::RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    pub async fn launch(&self, command: &str, cols: u16, rows: u16) -> Result<String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY")?;

        let args = shell_words::split(command).context("failed to parse command")?;
        let mut cmd = if args.is_empty() {
            CommandBuilder::new_default_prog()
        } else {
            let mut cb = CommandBuilder::new(&args[0]);
            for arg in &args[1..] {
                cb.arg(arg);
            }
            cb
        };
        cmd.env("TERM", "xterm-256color");

        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn command")?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let id = Uuid::new_v4().to_string();
        let parser = Arc::new(RwLock::new(vt100::Parser::new(rows, cols, 1000)));
        let alive = Arc::new(AtomicBool::new(true));
        let last_output = Arc::new(AtomicU64::new(0));

        let recorder = Arc::new(Mutex::new(Recorder::new(1000)));

        let session = Arc::new(Session {
            id: id.clone(),
            parser: Arc::clone(&parser),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            alive: Arc::clone(&alive),
            last_output_epoch_ms: Arc::clone(&last_output),
            recorder: Arc::clone(&recorder),
        });

        {
            let parser = Arc::clone(&parser);
            let alive = Arc::clone(&alive);
            let last_output = Arc::clone(&last_output);
            let recorder = Arc::clone(&recorder);
            let mut child = child;
            std::thread::spawn(move || {
                Self::reader_loop(reader, parser, alive, last_output, recorder);
                let _ = child.wait();
            });
        }

        self.sessions.write().await.insert(id.clone(), session);
        Ok(id)
    }

    fn reader_loop(
        mut reader: Box<dyn Read + Send>,
        parser: Arc<RwLock<vt100::Parser>>,
        alive: Arc<AtomicBool>,
        last_output: Arc<AtomicU64>,
        recorder: Arc<Mutex<Recorder>>,
    ) {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut p) = parser.write() {
                        p.process(&buf[..n]);
                    }
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    last_output.store(now, Ordering::Relaxed);

                    if let Ok(mut rec) = recorder.lock() {
                        if rec.enabled {
                            if let Ok(p) = parser.read() {
                                let g = grid::extract_grid(p.screen());
                                rec.capture(g.text_content(), now);
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
        alive.store(false, Ordering::Relaxed);
    }

    pub async fn get(&self, session_id: &str) -> Result<Arc<Session>> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))
    }

    pub async fn close(&self, session_id: &str) -> Result<()> {
        self.sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| anyhow::anyhow!("session not found: {session_id}"))?;
        Ok(())
    }

    pub async fn list(&self) -> Vec<String> {
        self.sessions.read().await.keys().cloned().collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
