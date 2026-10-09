#![allow(clippy::unwrap_used)]
use super::*;
use crate::EstimateTokens;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "../../../fixtures/pdf/mod.rs"]
mod fixture;

struct Temp(std::path::PathBuf);
impl Temp {
    fn pdf(mut doc: Document) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lumen-pdf-{}-{}.pdf",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        doc.save(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn extract(t: &Temp, limits: &PdfLimits) -> Result<IndexedDocument, PdfError> {
    extract_pdf(
        &t.0,
        limits,
        &ChunkConfig::default(),
        &EstimateTokens,
        &|| false,
    )
}

#[test]
fn physical_pages_normalized_offsets_and_page_bounded_chunks() {
    let first = "solar panels clean electricity ".repeat(150);
    let t = Temp::pdf(fixture::document(&[
        first.as_bytes(),
        b"",
        b"ocean tides fish conservation\r\nreef",
    ]));
    let indexed = extract(&t, &PdfLimits::default()).unwrap();
    assert!(indexed.chunks.len() > 2);
    for (i, c) in indexed.chunks.iter().enumerate() {
        assert_eq!(c.chunk.ordinal as usize, i);
        assert!(c.chunk.tokens <= ChunkConfig::default().max_tokens);
        let text = c.chunk.text(&indexed.doc.text);
        assert!(!text.contains('\r'));
        match c.page_number {
            Some(1) => assert!(text.contains("solar") && !text.contains("ocean")),
            Some(3) => assert!(text.contains("ocean") && !text.contains("solar")),
            p => panic!("unexpected physical page {p:?}"),
        }
    }
    assert_eq!(indexed.chunks.last().unwrap().page_number, Some(3));
}

#[test]
fn unicode_to_unicode_font_map_and_legacy_encoding() {
    let mut doc = fixture::document(&[b"ABC"]);
    let cmap = doc.add_object(lopdf::Stream::new(lopdf::dictionary!{}, b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def /CMapName /Test def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange 3 beginbfchar <41> <00E1> <42> <4F60> <43> <597D> endbfchar endcmap CMapName currentdict /CMap defineresource pop end end".to_vec()));
    for object in doc.objects.values_mut() {
        if let Ok(font) = object.as_dict_mut()
            && font.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"Font")
        {
            font.set("ToUnicode", cmap);
            font.remove(b"Encoding");
        }
    }
    let t = Temp::pdf(doc);
    assert!(
        extract(&t, &PdfLimits::default())
            .unwrap()
            .doc
            .text
            .contains("á你好")
    );
    let legacy = Temp::pdf(fixture::document(&[b"caf\xe9 reuni\xf3n"]));
    assert!(
        extract(&legacy, &PdfLimits::default())
            .unwrap()
            .doc
            .text
            .contains("café reunión")
    );
}

#[test]
fn limits_no_text_and_cancellation_are_explicit() {
    let t = Temp::pdf(fixture::document(&[b"first page", b"second page"]));
    let limits = PdfLimits {
        max_file_bytes: 10,
        ..PdfLimits::default()
    };
    assert_eq!(extract(&t, &limits).unwrap_err(), PdfError::TooLarge);
    let limits = PdfLimits {
        max_pages: 1,
        ..PdfLimits::default()
    };
    assert_eq!(extract(&t, &limits).unwrap_err(), PdfError::TooManyPages);
    let limits = PdfLimits {
        max_text_bytes: 15,
        ..PdfLimits::default()
    };
    assert_eq!(extract(&t, &limits).unwrap_err(), PdfError::TextLimit);
    let limits = PdfLimits {
        max_elapsed: Duration::ZERO,
        ..PdfLimits::default()
    };
    assert_eq!(extract(&t, &limits).unwrap_err(), PdfError::TimeBudget);
    assert_eq!(
        extract_pdf(
            &t.0,
            &PdfLimits::default(),
            &ChunkConfig::default(),
            &EstimateTokens,
            &|| true
        )
        .unwrap_err(),
        PdfError::Cancelled
    );
    let blank = Temp::pdf(fixture::document(&[b""]));
    assert_eq!(
        extract(&blank, &PdfLimits::default()).unwrap_err(),
        PdfError::NoText
    );
    // Cancel after work has started: no successful partial document escapes.
    let calls = std::cell::Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() >= 7
    };
    assert_eq!(
        extract_pdf(
            &t.0,
            &PdfLimits::default(),
            &ChunkConfig::default(),
            &EstimateTokens,
            &cancelled
        )
        .unwrap_err(),
        PdfError::Cancelled
    );
}

#[test]
fn compressed_page_bomb_is_rejected_without_unbounded_decoding() {
    let text = "highly compressible PDF text ".repeat(100_000);
    let t = Temp::pdf(fixture::document(&[text.as_bytes()]));
    assert!(std::fs::metadata(&t.0).unwrap().len() < 50_000);
    let limits = PdfLimits {
        max_stream_bytes: 64 * 1024,
        ..PdfLimits::default()
    };
    assert_eq!(
        extract(&t, &limits).unwrap_err(),
        PdfError::DecompressionLimit
    );
}

#[test]
fn encrypted_and_malformed_documents_are_not_indexed() {
    use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
    let mut doc = fixture::document(&[b"secret page"]);
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner",
        user_password: "reader",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    doc.encrypt(&state).unwrap();
    let t = Temp::pdf(doc);
    assert_eq!(
        extract(&t, &PdfLimits::default()).unwrap_err(),
        PdfError::Encrypted
    );
    let mut unlocked = fixture::document(&[b"encrypted with empty reader password"]);
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &unlocked,
        owner_password: "owner",
        user_password: "",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    unlocked.encrypt(&state).unwrap();
    let unlocked = Temp::pdf(unlocked);
    assert_eq!(
        extract(&unlocked, &PdfLimits::default()).unwrap_err(),
        PdfError::Encrypted
    );
    std::fs::write(&t.0, b"%PDF-1.7\ninvalid private content").unwrap();
    assert_eq!(
        extract(&t, &PdfLimits::default()).unwrap_err(),
        PdfError::Malformed
    );
    assert_eq!(PdfError::Malformed.to_string(), "pdf:malformed");
}

#[test]
fn compressed_font_and_load_time_xref_bombs_are_also_bounded() {
    let mut doc = fixture::document(&[b"ABC"]);
    let mut bomb = lopdf::Stream::new(lopdf::dictionary! {}, vec![b' '; 256 * 1024]);
    bomb.compress().unwrap();
    let id = doc.add_object(bomb.clone());
    for object in doc.objects.values_mut() {
        if let Ok(font) = object.as_dict_mut()
            && font.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"Font")
        {
            font.remove(b"Encoding");
            font.set("ToUnicode", id);
        }
    }
    let t = Temp::pdf(doc);
    let limits = PdfLimits {
        max_stream_bytes: 32 * 1024,
        ..PdfLimits::default()
    };
    assert_eq!(
        extract(&t, &limits).unwrap_err(),
        PdfError::DecompressionLimit
    );
    // Independently construct a malformed but correctly framed xref stream. Loading
    // must hit the decompression limit before interpreting its huge reference table.
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let offset = pdf.len();
    pdf.extend_from_slice(format!("1 0 obj\n<< /Type /XRef /Size 1 /W [1 1 1] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n", bomb.content.len()).as_bytes());
    pdf.extend_from_slice(&bomb.content);
    pdf.extend_from_slice(format!("\nendstream\nendobj\nstartxref\n{offset}\n%%EOF").as_bytes());
    std::fs::write(&t.0, pdf).unwrap();
    assert_eq!(
        extract(&t, &limits).unwrap_err(),
        PdfError::DecompressionLimit
    );
}
