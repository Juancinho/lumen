//! Lossless storage of OS paths (ADR-018: non-Unicode names are inventoried too).
//!
//! `text` is the lookup/display form: the path itself when it is valid Unicode, otherwise
//! valid parts kept and each invalid unit escaped (`\u{d800}` on Windows, `\x{ff}` on Unix).
//! `raw` then carries the exact path (Windows UTF-16LE, Unix bytes) so it can be opened.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathText {
    pub text: String,
    pub raw: Option<Vec<u8>>,
}

#[must_use]
pub fn encode(path: &Path) -> PathText {
    if let Some(text) = path.to_str() {
        return PathText {
            text: text.to_owned(),
            raw: None,
        };
    }
    platform::encode_lossless(path)
}

/// The exact path back from storage.
#[must_use]
pub fn decode(text: &str, raw: Option<&[u8]>) -> PathBuf {
    match raw {
        Some(bytes) => platform::decode_raw(bytes).unwrap_or_else(|| PathBuf::from(text)),
        None => PathBuf::from(text),
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    use super::{OsString, Path, PathBuf, PathText};

    pub(super) fn encode_lossless(path: &Path) -> PathText {
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut text = String::new();
        for r in char::decode_utf16(units.iter().copied()) {
            match r {
                Ok(c) => text.push(c),
                Err(e) => text.push_str(&format!("\\u{{{:x}}}", e.unpaired_surrogate())),
            }
        }
        let raw = units.iter().flat_map(|u| u.to_le_bytes()).collect();
        PathText {
            text,
            raw: Some(raw),
        }
    }

    pub(super) fn decode_raw(bytes: &[u8]) -> Option<PathBuf> {
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Some(PathBuf::from(OsString::from_wide(&units)))
    }
}

#[cfg(not(windows))]
mod platform {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    use super::{OsString, Path, PathBuf, PathText};

    pub(super) fn encode_lossless(path: &Path) -> PathText {
        let bytes = path.as_os_str().as_bytes();
        let mut text = String::new();
        for chunk in bytes.utf8_chunks() {
            text.push_str(chunk.valid());
            for b in chunk.invalid() {
                text.push_str(&format!("\\x{{{b:02x}}}"));
            }
        }
        PathText {
            text,
            raw: Some(bytes.to_vec()),
        }
    }

    #[allow(clippy::unnecessary_wraps)] // same signature as Windows
    pub(super) fn decode_raw(bytes: &[u8]) -> Option<PathBuf> {
        Some(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_paths_are_plain_text() {
        let p = Path::new("/home/joao/Reunión.md");
        let t = encode(p);
        assert_eq!(t.raw, None);
        assert_eq!(decode(&t.text, None), p);
    }

    #[cfg(unix)]
    #[test]
    fn non_unicode_paths_round_trip() {
        use std::os::unix::ffi::OsStrExt;
        let p = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/bad-\xff-name"));
        let t = encode(p);
        assert_eq!(t.text, "/tmp/bad-\\x{ff}-name");
        assert_eq!(decode(&t.text, t.raw.as_deref()), p);
    }

    #[cfg(windows)]
    #[test]
    fn unpaired_surrogates_round_trip() {
        use std::os::windows::ffi::OsStringExt;
        let mut units: Vec<u16> = "C:\\lone-".encode_utf16().collect();
        units.push(0xD800);
        units.extend(".txt".encode_utf16());
        let p = PathBuf::from(OsString::from_wide(&units));
        let t = encode(&p);
        assert_eq!(t.text, "C:\\lone-\\u{d800}.txt");
        assert_eq!(decode(&t.text, t.raw.as_deref()), p);
    }
}
