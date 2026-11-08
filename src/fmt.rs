//! Terminal colours and date formatting.

use std::io::IsTerminal;
use std::sync::LazyLock;

use chrono::{Local, TimeZone};

/// Colours are only emitted when stdout is a terminal.
pub static COLOR: LazyLock<bool> = LazyLock::new(|| std::io::stdout().is_terminal());

pub const RED: &str = "31";
pub const GREEN: &str = "32";
pub const YELLOW: &str = "33";
pub const CYAN: &str = "36";
pub const DIM: &str = "2";
pub const BOLD: &str = "1";

pub fn col(s: &str, code: &str) -> String {
    if *COLOR {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_string()
    }
}

/// `Mon 03.11 14:00 (in 2d)`, relative to `now`.
pub fn when(ts: i64, now: i64) -> String {
    let date = Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%a %d.%m %H:%M").to_string())
        .unwrap_or_else(|| ts.to_string());
    format!("{date} ({})", relative(ts, now))
}

fn relative(ts: i64, now: i64) -> String {
    let delta = (ts - now) as f64 / 86400.0;
    if delta >= 1.0 {
        format!("in {delta:.0}d")
    } else if delta >= 0.0 {
        format!("in {:.0}h", delta * 24.0)
    } else {
        format!("{:.0}d ago", -delta)
    }
}

/// Short local date, e.g. `03.11`.
pub fn day_month(ts: i64) -> String {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%d.%m").to_string())
        .unwrap_or_default()
}

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// Replace the home directory prefix with `~`.
pub fn tilde(path: &str) -> String {
    match dirs::home_dir().and_then(|h| h.to_str().map(str::to_string)) {
        Some(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_dates() {
        let now = 1_000_000;
        assert_eq!(relative(now + 3 * 86400, now), "in 3d");
        assert_eq!(relative(now + 5 * 3600, now), "in 5h");
        assert_eq!(relative(now - 2 * 86400, now), "2d ago");
    }

    #[test]
    fn sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1536), "1.5 KB");
        assert_eq!(size(1952876), "1.9 MB");
    }
}
