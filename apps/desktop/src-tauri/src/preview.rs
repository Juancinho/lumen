//! Quick Look data (T105): metadata and a short text excerpt of a result the user saw.
//! Narrow, bounded reads only (docs/ARCHITECTURE.md §4): at most [`READ_BYTES`] from files
//! up to [`MAX_TEXT_FILE`], text-like extensions only. T302 renders PDF pages separately
//! on its bounded worker; this first response supplies metadata and indexed text.

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
        ResultKind::Code => "code",
        ResultKind::PdfPage => "pdf-page",
        ResultKind::Image => "image",
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
        page_number: None,
        image: match &item.payload {
            Payload::Image(image) => Some(crate::dto::ImageContextDto::from(image.as_ref())),
            _ => None,
        },
        image_ocr: None,
    };
    if let Payload::Code(code) = &item.payload {
        dto.text = Some(code.passage.clone());
        // It is the indexed matching passage, rather than the complete file.
        dto.truncated = true;
    }
    if let Payload::Pdf(pdf) = &item.payload {
        dto.text = Some(pdf.passage.clone());
        dto.page_number = Some(pdf.page_number.get());
        dto.truncated = true;
    }
    let Some(path) = item.payload.local_path() else {
        return dto;
    };
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
    {
        dto.page_number.get_or_insert(1);
    }
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
        if dto.text.is_none()
            && meta.len() <= MAX_TEXT_FILE
            && has_text_extension(path)
            && let Some((text, truncated)) = read_excerpt(path, meta.len())
        {
            dto.text = Some(text);
            dto.truncated = truncated;
        }
    }
    dto
}

pub(crate) fn image_ocr(
    dto: &mut PreviewDto,
    enabled: bool,
    unavailable: bool,
    record: Option<&lumen_storage::ocr::Preview>,
) {
    dto.text = None;
    let mut state = if !enabled {
        "off"
    } else if unavailable {
        "unavailable"
    } else {
        "pending"
    };
    let mut language = None;
    let mut reason = None;
    if enabled && let Some(record) = record {
        state = match record.state.as_str() {
            "indexed" => "indexed",
            "empty" => "empty",
            "skipped" => "skipped",
            "failed" => "failed",
            _ => "pending",
        };
        language = record
            .language
            .clone()
            .filter(|l| l.len() <= 80 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
        reason = record
            .reason
            .clone()
            .filter(|r| lumen_storage::ocr::valid_reason(r));
        if state == "indexed" && record.text.len() <= lumen_storage::ocr::MAX_TEXT_BYTES {
            dto.text = Some(record.text.clone());
            dto.truncated = false;
        }
    }
    dto.image_ocr = Some(crate::dto::ImageOcrDto {
        state,
        language,
        reason,
    });
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
        let mut code = item;
        code.kind = ResultKind::Code;
        code.payload = Payload::Code(Box::new(lumen_core::CodeTarget {
            path: file,
            symbol: Some("retry".into()),
            language: "python".into(),
            repository: None,
            start_offset: Some(50000),
            end_offset: Some(50100),
            passage: "def retry(): pass".into(),
        }));
        let p = preview(&code);
        assert_eq!(p.kind, "code");
        assert_eq!(
            p.text.as_deref(),
            Some("def retry(): pass"),
            "Quick Look shows the matching passage even beyond its file-read budget"
        );
        assert!(p.truncated);
        let mut pdf = code;
        pdf.kind = ResultKind::PdfPage;
        pdf.payload = Payload::Pdf(Box::new(lumen_core::PdfTarget {
            path: dir.join("missing.pdf"),
            page_number: std::num::NonZeroU32::new(7).unwrap(),
            passage: "matching page seven".into(),
        }));
        let p = preview(&pdf);
        assert_eq!(p.kind, "pdf-page");
        assert_eq!(p.page_number, Some(7));
        assert_eq!(p.text.as_deref(), Some("matching page seven"));
        assert!(
            p.truncated,
            "preview uses the indexed passage without parsing the PDF again"
        );
        let mut image = pdf;
        image.kind = ResultKind::Image;
        image.payload = Payload::Path(dir.join("capture.png"));
        let mut dto = preview(&image);
        let record = lumen_storage::ocr::Preview {
            path: lumen_catalog::path::encode(&dir.join("capture.png")).text,
            state: "indexed".into(),
            language: Some("es-ES".into()),
            reason: Some("private backend detail".into()),
            text: "ERROR 42\ncontraseña".into(),
        };
        image_ocr(&mut dto, true, false, Some(&record));
        assert_eq!(dto.text.as_deref(), Some("ERROR 42\ncontraseña"));
        assert_eq!(dto.image_ocr.as_ref().unwrap().reason, None);
        image_ocr(&mut dto, false, false, Some(&record));
        assert_eq!(dto.text, None);
        assert_eq!(dto.image_ocr.as_ref().unwrap().state, "off");
        image_ocr(&mut dto, true, true, None);
        assert_eq!(dto.image_ocr.as_ref().unwrap().state, "unavailable");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
