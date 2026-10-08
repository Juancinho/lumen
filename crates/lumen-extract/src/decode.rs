use std::io::Read;
use std::path::Path;

use crate::kinds::{DocKind, kind_for_extension};

/// Files above this size are skipped (reported, not read): logs and dumps rarely help
/// retrieval and would dominate the embedding queue.
pub const DEFAULT_MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Bytes inspected for NUL characters to tell binary from text.
const SNIFF_BYTES: usize = 8 * 1024;

/// Why a file was not extracted. Every skip is visible (ADR-018 coverage guarantee).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// Not a text kind ([`kind_for_extension`]).
    Unsupported,
    /// Larger than the configured limit (size in bytes).
    TooLarge(u64),
    /// Binary content behind a text extension.
    Binary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    Skipped(Skip),
    /// Reading failed (permission, vanished, cloud placeholder that cannot be read…).
    Io(std::io::ErrorKind),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skipped(Skip::Unsupported) => f.write_str("not a text file"),
            Self::Skipped(Skip::TooLarge(n)) => write!(f, "too large ({n} bytes)"),
            Self::Skipped(Skip::Binary) => f.write_str("binary content"),
            Self::Io(kind) => write!(f, "read failed: {kind:?}"),
        }
    }
}

impl std::error::Error for ExtractError {}

/// Extracted text, normalized: BOM removed, `\r\n` and lone `\r` turned into `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub kind: DocKind,
    pub text: String,
    /// `utf-8`, `utf-16le`, `utf-16be` or `windows-1252`.
    pub encoding: &'static str,
    /// Undecodable bytes were replaced (U+FFFD).
    pub lossy: bool,
}

/// Reads and decodes a text file. `max_bytes` bounds the read ([`DEFAULT_MAX_BYTES`]).
///
/// # Errors
/// [`ExtractError::Skipped`] for unsupported, too large or binary files; `Io` otherwise.
pub fn extract_file(path: &Path, max_bytes: u64) -> Result<Extracted, ExtractError> {
    let kind = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(kind_for_extension)
        .ok_or(ExtractError::Skipped(Skip::Unsupported))?;
    let io = |e: std::io::Error| ExtractError::Io(e.kind());
    let file = std::fs::File::open(path).map_err(io)?;
    let len = file.metadata().map_err(io)?.len();
    if len > max_bytes {
        return Err(ExtractError::Skipped(Skip::TooLarge(len)));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    // `take`: the file may grow between metadata and read.
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > max_bytes {
        return Err(ExtractError::Skipped(Skip::TooLarge(bytes.len() as u64)));
    }
    let (text, encoding, lossy) = decode(&bytes).map_err(ExtractError::Skipped)?;
    Ok(Extracted {
        kind,
        text,
        encoding,
        lossy,
    })
}

/// Decodes file bytes: UTF-8/UTF-16 by BOM, else UTF-8 if valid, else Windows-1252 (the
/// usual legacy encoding of Spanish/Western Windows text files). NUL bytes in the first
/// 8 KiB of a file without a UTF-16 BOM mean binary.
///
/// # Errors
/// [`Skip::Binary`].
pub fn decode(bytes: &[u8]) -> Result<(String, &'static str, bool), Skip> {
    let (text, encoding, lossy) = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        let (t, lossy) = utf8(rest);
        (t, "utf-8", lossy)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        let (t, lossy) = encoding_rs::UTF_16LE.decode_without_bom_handling(rest);
        (t.into_owned(), "utf-16le", lossy)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        let (t, lossy) = encoding_rs::UTF_16BE.decode_without_bom_handling(rest);
        (t.into_owned(), "utf-16be", lossy)
    } else {
        if bytes[..bytes.len().min(SNIFF_BYTES)].contains(&0) {
            return Err(Skip::Binary);
        }
        match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_owned(), "utf-8", false),
            Err(_) => {
                let (t, lossy) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
                (t.into_owned(), "windows-1252", lossy)
            }
        }
    };
    Ok((normalize_newlines(&text), encoding, lossy))
}

fn utf8(bytes: &[u8]) -> (String, bool) {
    match String::from_utf8_lossy(bytes) {
        std::borrow::Cow::Borrowed(s) => (s.to_owned(), false),
        std::borrow::Cow::Owned(s) => (s, true),
    }
}

fn normalize_newlines(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_boms_utf8_and_legacy_text() {
        assert_eq!(
            decode(b"hola\r\nmundo\r").unwrap(),
            ("hola\nmundo\n".into(), "utf-8", false)
        );
        assert_eq!(decode(b"\xEF\xBB\xBFBOM").unwrap().0, "BOM");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("reunión".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            decode(&utf16).unwrap(),
            ("reunión".into(), "utf-16le", false)
        );
        // "reunión" in Windows-1252: ó = 0xF3, invalid as UTF-8.
        assert_eq!(
            decode(b"reuni\xF3n").unwrap(),
            ("reunión".into(), "windows-1252", false)
        );
        assert_eq!(decode(b"PK\x03\x04\x00\x00"), Err(Skip::Binary));
        assert_eq!(decode(b"").unwrap().0, "");
    }

    #[test]
    fn files_are_bounded_and_classified() {
        let dir = std::env::temp_dir().join(format!("lumen-extract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# Título\r\ntexto").unwrap();
        std::fs::write(dir.join("b.bin.txt"), [0u8, 1, 2]).unwrap();
        std::fs::write(dir.join("c.pdf"), b"%PDF").unwrap();
        std::fs::write(dir.join("d.log"), vec![b'x'; 100]).unwrap();
        let a = extract_file(&dir.join("a.md"), DEFAULT_MAX_BYTES).unwrap();
        assert_eq!(
            (a.kind, a.text.as_str()),
            (DocKind::Markdown, "# Título\ntexto")
        );
        assert_eq!(
            extract_file(&dir.join("b.bin.txt"), DEFAULT_MAX_BYTES),
            Err(ExtractError::Skipped(Skip::Binary))
        );
        assert_eq!(
            extract_file(&dir.join("c.pdf"), DEFAULT_MAX_BYTES),
            Err(ExtractError::Skipped(Skip::Unsupported))
        );
        assert_eq!(
            extract_file(&dir.join("d.log"), 50),
            Err(ExtractError::Skipped(Skip::TooLarge(100)))
        );
        assert!(matches!(
            extract_file(&dir.join("missing.txt"), DEFAULT_MAX_BYTES),
            Err(ExtractError::Io(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
