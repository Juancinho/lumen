//! Opt-in timing diagnostics (T012): set `LUMEN_DIAG_LOG=<file>` and the shell appends one
//! line per event, `<event> <value_ms> <unix_ms>`, for `scripts/t012/run-windows-webview.ps1`.
//! Disabled (the default), every call is a cheap check of a `OnceLock`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Environment variable naming the diagnostics log file.
pub(crate) const ENV_LOG: &str = "LUMEN_DIAG_LOG";

static START: OnceLock<Instant> = OnceLock::new();
static LOG: OnceLock<Option<Mutex<File>>> = OnceLock::new();
static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);
/// The show currently waiting for the UI's paint acknowledgement.
static PENDING_SHOW: Mutex<Option<(u64, Instant)>> = Mutex::new(None);

/// Call first thing in `main` so `since_start` covers Tauri/WebView start-up.
pub(crate) fn init() {
    START.get_or_init(Instant::now);
    LOG.get_or_init(|| {
        let path = std::env::var_os(ENV_LOG)?;
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(file) => Some(Mutex::new(file)),
            Err(err) => {
                eprintln!("lumen: cannot open {ENV_LOG}: {err}");
                None
            }
        }
    });
}

pub(crate) fn enabled() -> bool {
    LOG.get().is_some_and(Option::is_some)
}

/// Milliseconds since `init`.
pub(crate) fn since_start_ms() -> f64 {
    START
        .get()
        .map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0)
}

pub(crate) fn record(event: &str, value_ms: f64) {
    let Some(Some(file)) = LOG.get() else { return };
    let unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    if let Ok(mut f) = file.lock() {
        let _ = writeln!(f, "{event} {value_ms:.3} {unix_ms}");
    }
}

/// Starts timing a show; returns the sequence number the UI echoes after painting.
pub(crate) fn begin_show() -> Option<u64> {
    if !enabled() {
        return None;
    }
    let seq = NEXT_SEQ.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut pending) = PENDING_SHOW.lock() {
        *pending = Some((seq, Instant::now()));
    }
    Some(seq)
}

/// The UI painted after show `seq`: records show → paint latency (and, for the first show,
/// process start → first paint).
pub(crate) fn painted(seq: u64) {
    let started = PENDING_SHOW
        .lock()
        .ok()
        .and_then(|mut p| p.take_if(|(s, _)| *s == seq));
    if let Some((_, at)) = started {
        record("show_to_paint_ms", at.elapsed().as_secs_f64() * 1000.0);
        if seq == 1 {
            record("first_paint_ms", since_start_ms());
        }
    }
}
