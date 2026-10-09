#![cfg(windows)]
#![allow(clippy::unwrap_used)]
use lumen_windows::pdf::{MAX_SOURCE_BYTES, PreviewError, Renderer};
use std::{path::PathBuf, sync::Arc};

#[path = "../../../fixtures/pdf/mod.rs"]
mod fixture;
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn temp(label: &str) -> Temp {
    let dir = Temp(std::env::temp_dir().join(format!("lumen-t302-{label}-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    dir
}

#[test]
fn physical_pages_blank_rotation_cache_edit_and_bounds() {
    let dir = temp("pages");
    let file = dir.0.join("notes 東京.pdf");
    let mut doc = fixture::document(&[b"Page one", b"", b"Matched page three"]);
    let page_ids: Vec<_> = doc.get_pages().into_values().collect();
    doc.get_object_mut(page_ids[2])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Rotate", 90);
    doc.save(&file).unwrap();
    let mut renderer = Renderer::default();
    let first = renderer.render(&file, 1, &|| false).unwrap();
    assert_eq!(first.page_count, 3);
    assert_eq!((first.width, first.height), (678, 960));
    assert!(first.png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let blank = renderer.render(&file, 2, &|| false).unwrap();
    assert_ne!(first.png, blank.png);
    let third = renderer.render(&file, 3, &|| false).unwrap();
    assert_eq!((third.width, third.height), (960, 678));
    let cached = renderer.render(&file, 3, &|| false).unwrap();
    assert!(Arc::ptr_eq(&third.png, &cached.png));
    assert_eq!(
        renderer.render(&file, 4, &|| false),
        Err(PreviewError::InvalidPage)
    );
    assert_eq!(
        renderer.render(&file, 2, &|| true),
        Err(PreviewError::Cancelled)
    );
    fixture::document(&[b"Changed source, fewer pages"])
        .save(&file)
        .unwrap();
    let changed = renderer.render(&file, 1, &|| false).unwrap();
    assert_eq!(changed.page_count, 1);
    assert_ne!(changed.png, first.png);
    renderer.clear();
    let refreshed = renderer.render(&file, 1, &|| false).unwrap();
    assert!(!Arc::ptr_eq(&changed.png, &refreshed.png));
    let large = dir.0.join("large.pdf");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_SOURCE_BYTES + 1)
        .unwrap();
    assert_eq!(
        renderer.render(&large, 1, &|| false),
        Err(PreviewError::TooLarge)
    );
    std::fs::write(&large, b"malformed PDF").unwrap();
    assert!(renderer.render(&large, 1, &|| false).is_err());
}

#[test]
fn cancellation_during_os_load_leaves_no_stale_document() {
    use std::cell::Cell;
    let dir = temp("cancel");
    let file = dir.0.join("source.pdf");
    fixture::document(&[b"Page one", b"Page two"])
        .save(&file)
        .unwrap();
    let checks = Cell::new(0);
    let cancel = || {
        checks.set(checks.get() + 1);
        checks.get() > 1
    };
    let mut renderer = Renderer::default();
    assert_eq!(
        renderer.render(&file, 2, &cancel),
        Err(PreviewError::Cancelled)
    );
    assert_eq!(renderer.render(&file, 2, &|| false).unwrap().page_number, 2);
}

#[test]
fn image_only_pdf_and_protected_page_limits() {
    use lopdf::{
        Stream, dictionary,
        encryption::{EncryptionState, EncryptionVersion, Permissions},
    };
    let dir = temp("image");
    let file = dir.0.join("scan.pdf");
    let mut doc = fixture::document(&[b""]);
    let page = doc.get_pages()[&1];
    let image = doc.add_object(Stream::new(dictionary! { "Type"=>"XObject", "Subtype"=>"Image", "Width"=>2, "Height"=>2, "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8 }, vec![255,0,0,0,255,0,0,0,255,255,255,0]));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 200 0 0 200 50 500 cm /Im1 Do Q".to_vec(),
    ));
    let page_dict = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    page_dict.set(
        "Resources",
        dictionary! { "XObject"=>dictionary! { "Im1"=>image } },
    );
    page_dict.set("Contents", content);
    doc.save(&file).unwrap();
    let mut renderer = Renderer::default();
    let scan = renderer.render(&file, 1, &|| false).unwrap();
    assert!(scan.png.len() > 1000);
    let mut protected = fixture::document(&[b"secret"]);
    let encryption = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &protected,
        owner_password: "owner",
        user_password: "reader",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .unwrap();
    protected.encrypt(&encryption).unwrap();
    protected.save(&file).unwrap();
    assert_eq!(
        renderer.render(&file, 1, &|| false),
        Err(PreviewError::Encrypted)
    );
    fixture::document(&vec![b"".as_slice(); 513])
        .save(&file)
        .unwrap();
    assert_eq!(
        renderer.render(&file, 1, &|| false),
        Err(PreviewError::PageLimit)
    );
}
