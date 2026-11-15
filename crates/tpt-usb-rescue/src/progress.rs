//! Progress display for the command line: a live line on a terminal, an occasional line otherwise.
//!
//! Progress goes to stderr so stdout stays clean for reports and pipes. On a terminal the line is
//! redrawn in place; [`ClearingStdout`] wipes it before any report text is written, so the two
//! never share a row.

use std::io::{self, IsTerminal, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tpt_usb_rescue::ops::human_bytes;
use tpt_usb_rescue_imager::engine::ProgressHook;
use tpt_usb_rescue_imager::mapfile::Totals;

const LINE_WIDTH: usize = 79;
const TTY_REDRAW: Duration = Duration::from_millis(250);
const PIPE_INTERVAL: Duration = Duration::from_secs(30);

/// Progress state shared between the hook and the stdout wrapper.
pub struct Progress {
    tty: bool,
    /// A progress line is currently on screen and must be cleared before other output.
    active: Arc<AtomicBool>,
}

impl Progress {
    pub fn new() -> Self {
        Self {
            tty: io::stderr().is_terminal(),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The hook to hand to the engine.
    pub fn hook(&self) -> ProgressHook {
        let tty = self.tty;
        let active = Arc::clone(&self.active);
        let start = Instant::now();
        let mut last_draw: Option<Instant> = None;
        let mut base: Option<u64> = None;
        Box::new(move |t: &Totals| {
            let now = Instant::now();
            let every = if tty { TTY_REDRAW } else { PIPE_INTERVAL };
            if last_draw.is_some_and(|l| now.duration_since(l) < every) {
                return;
            }
            last_draw = Some(now);
            let first = *base.get_or_insert(t.recovered);
            let line = format_line(t, first, now.duration_since(start));
            let mut err = io::stderr().lock();
            if tty {
                let _ = write!(err, "\r{line:<LINE_WIDTH$}");
                active.store(true, Ordering::SeqCst);
            } else {
                let _ = writeln!(err, "{line}");
            }
            let _ = err.flush();
        })
    }

    /// Wipes the progress line, if one is showing.
    pub fn clear(&self) {
        clear_line(&self.active);
    }

    /// A writer for report text that clears the progress line before writing. It goes to stderr
    /// when `to_stderr` is set, which keeps stdout free for `--json`.
    pub fn writer(&self, to_stderr: bool) -> ClearingOut {
        ClearingOut {
            active: Arc::clone(&self.active),
            to_stderr,
        }
    }
}

fn clear_line(active: &AtomicBool) {
    if active.swap(false, Ordering::SeqCst) {
        let mut err = io::stderr().lock();
        let _ = write!(err, "\r{:<LINE_WIDTH$}\r", "");
        let _ = err.flush();
    }
}

pub struct ClearingOut {
    active: Arc<AtomicBool>,
    to_stderr: bool,
}

impl Write for ClearingOut {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        clear_line(&self.active);
        if self.to_stderr {
            io::stderr().write(buf)
        } else {
            io::stdout().write(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.to_stderr {
            io::stderr().flush()
        } else {
            io::stdout().flush()
        }
    }
}

/// One progress line: percent done, recovered and bad bytes, speed and time left.
fn format_line(t: &Totals, base_recovered: u64, elapsed: Duration) -> String {
    let total = t.recovered + t.bad + t.non_trimmed + t.untested;
    let done = t.recovered + t.bad;
    let pct = if total == 0 {
        100.0
    } else {
        done as f64 * 100.0 / total as f64
    };
    let secs = elapsed.as_secs_f64().max(0.001);
    let rate = t.recovered.saturating_sub(base_recovered) as f64 / secs;
    let mut line = format!(
        "{pct:5.1}%  recovered {}  bad {}",
        human_bytes(t.recovered),
        human_bytes(t.bad + t.non_trimmed)
    );
    if rate >= 1.0 {
        line.push_str(&format!("  {}/s", human_bytes(rate as u64)));
        if t.untested > 0 {
            line.push_str(&format!("  ETA {}", format_eta(t.untested as f64 / rate)));
        }
    }
    line
}

fn format_eta(secs: f64) -> String {
    let s = secs.round() as u64;
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m{:02}s", s / 60, s % 60),
        _ => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn totals(recovered: u64, bad: u64, non_trimmed: u64, untested: u64) -> Totals {
        Totals {
            recovered,
            bad,
            non_trimmed,
            untested,
        }
    }

    #[test]
    fn line_shows_percent_speed_and_eta() {
        let t = totals(50 << 20, 0, 0, 50 << 20);
        let line = format_line(&t, 0, Duration::from_secs(10));
        assert!(line.contains("50.0%"), "{line}");
        assert!(line.contains("/s"), "{line}");
        assert!(line.contains("ETA 10s"), "{line}");
    }

    #[test]
    fn finished_run_has_no_eta() {
        let line = format_line(&totals(100, 0, 0, 0), 0, Duration::from_secs(1));
        assert!(line.contains("100.0%") && !line.contains("ETA"), "{line}");
    }

    #[test]
    fn eta_formats() {
        assert_eq!(format_eta(5.0), "5s");
        assert_eq!(format_eta(125.0), "2m05s");
        assert_eq!(format_eta(7500.0), "2h05m");
    }
}
