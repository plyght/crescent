use crate::grid::Grid;
use regex::Regex;
use std::time::{Duration, Instant};
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
