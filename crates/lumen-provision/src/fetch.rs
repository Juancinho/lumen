//! Where component files come from.
//!
//! - [`CurlFetch`]: HTTPS through the system `curl` (shipped with Windows 10 1803+ as
//!   `%SystemRoot%\System32\curl.exe`; Schannel TLS, the Windows certificate store). Lumen
//!   links no TLS stack and opens no connection itself (ADR-034). Resumes partial files.
//! - [`DirFetch`]: a local folder holding the files under their URL file names — offline
//!   installs from a USB stick or a shared drive, and tests.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use lumen_core::CancellationToken;

#[derive(Debug)]
pub enum FetchError {
    Cancelled,
    /// Message for logs and the UI (no personal data: URLs are the pinned public ones).
    Failed(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for FetchError {}

/// Downloads `url` into `dest`, appending to what `dest` already holds (resume).
/// `progress(bytes_in_dest)` is called while it runs.
pub trait Fetch: Send + Sync {
    /// # Errors
    /// [`FetchError::Cancelled`] when `cancel` fired; `Failed` otherwise.
    fn fetch(
        &self,
        url: &str,
        dest: &Path,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), FetchError>;
}

/// The last path segment of a URL.
#[must_use]
pub fn url_file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// HTTPS via the system `curl`.
#[derive(Debug, Clone)]
pub struct CurlFetch {
    program: PathBuf,
}

impl Default for CurlFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl CurlFetch {
    /// On Windows the absolute System32 path (never a `curl` found on `PATH`).
    #[must_use]
    pub fn new() -> Self {
        let program = if cfg!(windows) {
            std::env::var_os("SystemRoot").map_or_else(
                || PathBuf::from(r"C:\Windows\System32\curl.exe"),
                |r| PathBuf::from(r).join("System32").join("curl.exe"),
            )
        } else {
            PathBuf::from("curl")
        };
        Self { program }
    }

    /// Whether the program can run at all.
    #[must_use]
    pub fn available(&self) -> bool {
        Command::new(&self.program)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

impl Fetch for CurlFetch {
    fn fetch(
        &self,
        url: &str,
        dest: &Path,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), FetchError> {
        if !url.starts_with("https://") {
            return Err(FetchError::Failed("only https URLs are fetched".into()));
        }
        let mut child = Command::new(&self.program)
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--proto",
                "=https",
                "--retry",
                "3",
                "--connect-timeout",
                "20",
                "--continue-at",
                "-",
                "--output",
            ])
            .arg(dest)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| FetchError::Failed(format!("cannot run curl: {e}")))?;
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(FetchError::Cancelled);
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    progress(std::fs::metadata(dest).map_or(0, |m| m.len()));
                    if status.success() {
                        return Ok(());
                    }
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = std::io::Read::read_to_string(&mut e, &mut err);
                    }
                    return Err(FetchError::Failed(format!(
                        "download failed ({}): {}",
                        status
                            .code()
                            .map_or_else(|| "signal".into(), |c| c.to_string()),
                        err.trim()
                    )));
                }
                Ok(None) => {}
                Err(e) => return Err(FetchError::Failed(e.to_string())),
            }
            progress(std::fs::metadata(dest).map_or(0, |m| m.len()));
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

/// Files from a local folder, by URL file name.
#[derive(Debug, Clone)]
pub struct DirFetch {
    pub dir: PathBuf,
}

impl Fetch for DirFetch {
    fn fetch(
        &self,
        url: &str,
        dest: &Path,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), FetchError> {
        use std::io::{Read, Seek, SeekFrom};
        let fail = |e: std::io::Error| FetchError::Failed(e.to_string());
        let src = self.dir.join(url_file_name(url));
        let mut from = std::fs::File::open(&src)
            .map_err(|e| FetchError::Failed(format!("{}: {e}", url_file_name(url))))?;
        let mut to = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dest)
            .map_err(fail)?;
        let mut have = to.metadata().map_err(fail)?.len();
        from.seek(SeekFrom::Start(have)).map_err(fail)?;
        let mut buf = vec![0; 1 << 20];
        loop {
            if cancel.is_cancelled() {
                return Err(FetchError::Cancelled);
            }
            let n = from.read(&mut buf).map_err(fail)?;
            if n == 0 {
                return Ok(());
            }
            to.write_all(&buf[..n]).map_err(fail)?;
            have += n as u64;
            progress(have);
        }
    }
}
