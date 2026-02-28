use crate::grid::Grid;
use regex::Regex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn wait_for_pattern<F>(
    pattern: &str,
    timeout_ms: Option<u64>,
    mut get_grid: F,
) -> anyhow::Result<bool>
where
    F: FnMut() -> anyhow::Result<Grid>,
{
    let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex pattern: {e}"))?;

    let timeout = timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT);
    let deadline = Instant::now() + timeout;

    loop {
        let grid = get_grid()?;
        let text = grid.text_content();
        if re.is_match(&text) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(POLL_INTERVAL.min(deadline - Instant::now())).await;
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Wait until no PTY output for `quiet_ms` milliseconds.
/// Best for non-TUI use: shell commands, build output, etc.
/// Returns true if idle was reached, false if timeout hit first.
pub async fn wait_for_idle(
    quiet_ms: u64,
    timeout_ms: Option<u64>,
    last_output: Arc<AtomicU64>,
) -> anyhow::Result<bool> {
    let timeout = timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT);
    let deadline = Instant::now() + timeout;

    loop {
        let last = last_output.load(Ordering::Relaxed);
        let now = now_ms();
        if last > 0 && now.saturating_sub(last) >= quiet_ms {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        sleep(POLL_INTERVAL.min(deadline - Instant::now())).await;
    }
}

/// Wait until the visible terminal text hasn't changed for `stable_ms`.
/// Works with TUIs that constantly repaint (cursor blink, status bar) -
/// only cares about whether the actual text content changed.
pub async fn wait_for_stable<F>(
    stable_ms: u64,
    timeout_ms: Option<u64>,
    mut get_grid: F,
) -> anyhow::Result<bool>
where
    F: FnMut() -> anyhow::Result<Grid>,
{
    let timeout = timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT);
    let deadline = Instant::now() + timeout;

    let mut last_text = get_grid()?.text_content();
    let mut stable_since = Instant::now();

    loop {
        sleep(POLL_INTERVAL.min(deadline - Instant::now())).await;

        let text = get_grid()?.text_content();
        if text != last_text {
            last_text = text;
            stable_since = Instant::now();
        } else if stable_since.elapsed() >= Duration::from_millis(stable_ms) {
            return Ok(true);
        }

        if Instant::now() >= deadline {
            return Ok(false);
        }
    }
}
