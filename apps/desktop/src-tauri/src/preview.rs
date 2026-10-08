//! Quick Look data (T105): metadata and a short text excerpt of a result the user saw.
//! Narrow, bounded reads only (docs/ARCHITECTURE.md §4): at most [`READ_BYTES`] from files
//! up to [`MAX_TEXT_FILE`], text-like extensions only, nothing for binaries. Rich previews
//! (images, PDF pages) come with their extractors (T302/T303).

use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use lumen_core::{Payload, ResultItem, ResultKind};

use crate::dto::PreviewDto;

/// Bytes read for an excerpt.
pub(crate) const READ_BYTES: usize = 16 * 1024;
/// Characters shown at most.
pub(crate) const EXCERPT_CHARS: usize = 4000;
/// Larger files are not read at all (logs, dumps).
pub(crate) const MAX_TEXT_FILE: u64 = 8 * 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "rst",
    "log",
    "csv",
    "tsv",
    "json",
    "jsonc",
    "toml",
    "yaml",
    "yml",
    "ini",
    "cfg",
    "conf",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "js",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "jsx",
    "rs",
    "py",
    "rb",
    "go",
    "java",
    "kt",
    "c",
    "h",
    "cc",
    "cpp",
    "hpp",
    "cs",
    "fs",
    "swift",
    "php",
    "lua",
    "sql",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "psm1",
    "bat",
    "cmd",
    "gitignore",
    "env",
    "tex",
    "srt",
    "vtt",
];

pub(crate) fn is_text_extension(ext: &str) -> bool {
    TEXT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// The excerpt shown for `bytes` (start of a file): `None` for binary content. Cut at a line
/// boundary near [`EXCERPT_CHARS`]; the flag says whether anything was left out.
pub(crate) fn excerpt(bytes: &[u8], whole_file: bool) -> Option<(String, bool)> {
    if bytes.contains(&0) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}');
    // A lossy tail can be a cut multi-byte character: drop it.
    let text = if whole_file {
        text
    } else {
        text.trim_end_matches('\u{fffd}')
    };
    let replaced = text.chars().filter(|c| *c == '\u{fffd}').count();
    if replaced * 20 > text.chars().count().max(1) {
        return None; // mostly undecodable: not text in an encoding we can show
    }
    let mut cut = text.len();
    let mut truncated = !whole_file;
    if let Some((i, _)) = text.char_indices().nth(EXCERPT_CHARS) {
        cut = text[..i].rfind('\n').filter(|&n| n > i / 2).unwrap_or(i);
        truncated = true;
    }
    Some((text[..cut].trim_end().to_owned(), truncated))
}

fn kind_name(kind: ResultKind) -> &'static str {
    match kind {
        ResultKind::Application => "application",
        ResultKind::Folder => "folder",
        ResultKind::Command => "command",
        _ => "file",
    }
}

/// Preview of a result Lumen produced (looked up by id, never a UI-supplied path).
pub(crate) fn preview(item: &ResultItem) -> PreviewDto {
    let mut dto = PreviewDto {
        title: item.title.clone(),
        kind: kind_name(item.kind),
        location: item.detail.clone(),
        size_bytes: None,
        modified_ms: None,
        text: None,
        truncated: false,
    };
    let Payload::Path(path) = &item.payload else {
        return dto;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return dto;
    };
    dto.modified_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok());
    if meta.is_file() {
        dto.size_bytes = Some(meta.len());
        if meta.len() <= MAX_TEXT_FILE
            && has_text_extension(path)
            && let Some((text, truncated)) = read_excerpt(path, meta.len())
        {
            dto.text = Some(text);
            dto.truncated = truncated;
        }
    }
    dto
}

fn has_text_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(is_text_extension)
}

fn read_excerpt(path: &Path, len: u64) -> Option<(String, bool)> {
    let mut buf = Vec::with_capacity(READ_BYTES.min(usize::try_from(len).unwrap_or(READ_BYTES)));
    std::fs::File::open(path)
        .ok()?
        .take(READ_BYTES as u64)
        .read_to_end(&mut buf)
        .ok()?;
    let whole = u64::try_from(buf.len()).is_ok_and(|n| n >= len);
    excerpt(&buf, whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_text_and_rejects_binaries() {
        assert_eq!(
            excerpt(b"hello\nworld\n", true),
            Some(("hello\nworld".into(), false))
        );
        assert_eq!(
            excerpt(b"\xef\xbb\xbfBOM", true),
            Some(("BOM".into(), false))
        );
        assert_eq!(excerpt(b"PK\x03\x04\x00\x00", true), None);
        // A partial read is marked truncated and loses a cut UTF-8 tail.
        let (t, truncated) = excerpt("añ".as_bytes().split_at(2).0, false).unwrap();
        assert_eq!((t.as_str(), truncated), ("a", true));
        // Mostly undecodable bytes (e.g. UTF-16 without NULs is rare; Latin-1 heavy) -> none.
        assert_eq!(excerpt(&[0xe9; 64], true), None);
    }

    #[test]
    fn long_text_is_cut_at_a_line_boundary() {
        let line = "x".repeat(99);
        let text = format!("{line}\n").repeat(100); // 10,000 chars
        let (t, truncated) = excerpt(text.as_bytes(), true).unwrap();
        assert!(truncated);
        assert!(t.chars().count() <= EXCERPT_CHARS);
        assert!(t.ends_with('x'));
        assert_eq!(t.lines().count(), EXCERPT_CHARS / 100);
    }

    #[test]
    fn previews_real_files_by_payload() {
        use lumen_core::builtin::OPEN;
        use lumen_core::{
            CapabilitySet, Confidence, IconRef, MatchKind, ProviderId, ResultId, Score,
        };
        let dir = std::env::temp_dir().join(format!("lumen-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("notas.md");
        std::fs::write(&file, "# Notas\nreunión").unwrap();
        let item = ResultItem {
            id: ResultId::new("item:1").unwrap(),
            provider: ProviderId::new("lumen.catalog").unwrap(),
            kind: ResultKind::File,
            title: "notas.md".into(),
            subtitle: None,
            detail: Some(dir.display().to_string()),
            icon: IconRef::FileExtension("md".into()),
            score: Score::new(Confidence::CERTAIN, MatchKind::Exact),
            capabilities: CapabilitySet::default(),
            primary_action: OPEN,
            secondary_actions: Vec::new(),
            payload: Payload::Path(file.clone()),
        };
        let p = preview(&item);
        assert_eq!(p.text.as_deref(), Some("# Notas\nreunión"));
        assert_eq!(p.size_bytes, Some(16));
        assert!(p.modified_ms.is_some());
        assert!(!p.truncated);
        let mut binary = item.clone();
        binary.payload = Payload::Path(dir.join("missing.md"));
        assert!(preview(&binary).text.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
