//! T208 through real name/content providers; synthetic metadata, no live app-data.
#![allow(clippy::unwrap_used)]

use lumen_catalog::{CatalogProvider, ContentProvider, record_action, text};
use lumen_core::{
    CancellationToken, Provider, ProviderQuery, QueryId, ResultItem, builtin, validate_result,
};
use lumen_storage::{CatalogEntry, ItemKind, NewChunk, Source, Store};
use std::path::PathBuf;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    writer: Store,
    dir: TempDir,
}
impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = TempDir(
            std::env::temp_dir().join(format!("lumen-query-syntax-{tag}-{}", std::process::id())),
        );
        std::fs::create_dir_all(&dir.0).unwrap();
        Self {
            writer: Store::open_writer(&dir.0.join("test.db")).unwrap(),
            dir,
        }
    }
    fn add(
        &mut self,
        path: &str,
        name: &str,
        kind: ItemKind,
        mtime: Option<i64>,
        body: &str,
    ) -> i64 {
        let scan = self.writer.begin_scan(Source::Files).unwrap();
        self.writer
            .upsert_entries(
                scan,
                &[CatalogEntry {
                    kind,
                    source: if kind == ItemKind::Application {
                        Source::Apps
                    } else {
                        Source::Files
                    },
                    path,
                    raw_path: None,
                    name,
                    name_key: &text::fold(name),
                    name_parts: &text::name_parts(name),
                    path_parts: &text::path_parts(path, 3),
                    extension: if kind == ItemKind::File {
                        name.rsplit_once('.').map(|(_, ext)| ext)
                    } else {
                        None
                    },
                    volume_id: None,
                    file_id: None,
                    launch_target: None,
                    attributes: 0,
                    size_bytes: None,
                    modified_at: mtime,
                    created_at: None,
                    error: None,
                }],
            )
            .unwrap();
        let id = self.writer.item_id_by_path(path).unwrap().unwrap();
        if kind == ItemKind::File {
            self.writer
                .insert_chunks(&[NewChunk {
                    item_id: id,
                    ordinal: 0,
                    chunk_kind: "text",
                    text: body,
                    symbol_name: None,
                    page_number: None,
                    start_offset: None,
                    end_offset: None,
                }])
                .unwrap();
        }
        id
    }
    fn names(&self) -> CatalogProvider {
        CatalogProvider::new(Store::open_reader(&self.dir.0.join("test.db")).unwrap())
    }
    fn content(&self) -> ContentProvider {
        ContentProvider::new(Store::open_reader(&self.dir.0.join("test.db")).unwrap())
    }
}
fn ask(provider: &dyn Provider, text: &str, typing: bool, limit: usize) -> Vec<ResultItem> {
    provider
        .search(
            &ProviderQuery {
                id: QueryId::new(1).unwrap(),
                text,
                typing,
                limit,
            },
            &CancellationToken::new(),
        )
        .unwrap()
}

#[test]
fn hard_filters_combine_across_names_and_content() {
    let mut f = Fixture::new("constraints");
    let old = 1_704_067_200_000; // 2024-01-01 UTC
    let now = 1_790_812_800_000; // 2026-10-01 UTC
    let body = "invoice for summer holiday beach hotel";
    f.add(
        r"D:\Docs\invoice.PDF",
        "invoice.PDF",
        ItemKind::File,
        Some(now),
        body,
    );
    f.add(
        r"D:\Docs-old\invoice.md",
        "invoice.md",
        ItemKind::File,
        Some(old),
        body,
    );
    f.add(
        r"D:\Mis documentos\invoice.py",
        "invoice.py",
        ItemKind::File,
        Some(now),
        body,
    );
    f.add(r"D:\Docs\beach.jpg", "beach.jpg", ItemKind::File, None, "");
    f.add(r"D:\Docs", "Docs", ItemKind::Folder, Some(now), "");
    f.add(
        "shell:AppsFolder\\Invoice",
        "Invoice",
        ItemKind::Application,
        None,
        "",
    );
    let names = f.names();
    let content = f.content();
    for (q, count) in [
        ("invoice ext:.PDF", 1),
        ("invoice type:file", 3),
        ("invoice type:app", 1),
        ("invoice type:folder", 0),
        (r"invoice in:D:\Docs", 1),
        ("invoice in:docs", 1),
        (r#"invoice in:"D:\Mis documentos" type:code"#, 1),
        ("invoice before:2026-10-01", 1),
        ("invoice after:2026-09-30", 2),
        ("invoice after:2026-10-01", 0),
        ("invoice ext:pdf ext:md", 0),
        ("invoice type:unknown", 0),
        ("invoice in:invoice.PDF", 0),
        ("invoice before:2025-02-29", 0),
        ("invoice ext:", 0),
    ] {
        let results = ask(&names, q, true, 10);
        assert_eq!(results.len(), count, "{q}: {results:?}");
        for result in results {
            assert!(validate_result(&result, &builtin::DESCRIPTORS).is_empty());
        }
        assert_eq!(
            ask(&content, q, false, 10).len(),
            if q.contains("type:app") { 0 } else { count },
            "content {q}"
        );
    }
    for (q, expected) in [
        ("type:image", "beach.jpg"),
        ("type:folder", "Docs"),
        ("type:app", "Invoice"),
        ("ext:pdf in:docs", "invoice.PDF"),
    ] {
        let results = ask(&names, q, true, 10);
        assert_eq!(results.len(), 1, "{q}");
        assert_eq!(results[0].title, expected);
        assert!(ask(&content, q, false, 10).is_empty());
    }
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(
        names
            .search(
                &ProviderQuery {
                    id: QueryId::new(2).unwrap(),
                    text: "ext:pdf",
                    typing: true,
                    limit: 10
                },
                &cancel
            )
            .is_err()
    );
}

#[test]
fn phrases_are_required_and_learning_cannot_bypass_filters() {
    let mut f = Fixture::new("phrases");
    f.add(
        "/docs/holiday beach.md",
        "holiday beach.md",
        ItemKind::File,
        None,
        "holiday beach hotel trip",
    );
    f.add(
        "/docs/beach holiday.md",
        "beach holiday.md",
        ItemKind::File,
        None,
        "beach holiday hotel trip",
    );
    f.add(
        "/docs/holiday beaches.md",
        "holiday beaches.md",
        ItemKind::File,
        None,
        "holiday beaches hotel trip",
    );
    let unrelated = f.add(
        "/docs/other.py",
        "other.py",
        ItemKind::File,
        None,
        "holiday ocean trip hotel",
    );
    for provider in [&f.names() as &dyn Provider, &f.content() as &dyn Provider] {
        let results = ask(provider, "\"holiday beach\" ext:md", false, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "holiday beach.md");
        assert!(ask(provider, "\"holiday beach\" ocean trip", false, 10).is_empty());
        assert!(ask(provider, "\"holiday bea\"", false, 10).is_empty());
    }
    let result = lumen_core::ResultId::new(format!("item:{unrelated}")).unwrap();
    for _ in 0..5 {
        record_action(
            &mut f.writer,
            &result,
            &builtin::OPEN,
            "holiday ext:py",
            1_790_812_800_000,
        )
        .unwrap();
    }
    assert!(
        f.writer
            .learned_choices("holiday", 5)
            .unwrap()
            .contains(&unrelated)
    );
    let results = ask(&f.names(), "holiday ext:md", true, 10);
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.id != result));
}

#[test]
fn filters_apply_before_limits_and_quoted_operators_stay_literal() {
    let mut f = Fixture::new("limits");
    for i in 0..80 {
        f.add(
            &format!("/excluded/invoice{i:03}.txt"),
            &format!("invoice{i:03}.txt"),
            ItemKind::File,
            None,
            "invoice invoice invoice",
        );
    }
    f.add(
        "/allowed/invoice999.pdf",
        "invoice999.pdf",
        ItemKind::File,
        None,
        "invoice",
    );
    f.add(
        "/docs/type-image.md",
        "type-image.md",
        ItemKind::File,
        None,
        "type image",
    );
    for provider in [&f.names() as &dyn Provider, &f.content() as &dyn Provider] {
        let results = ask(provider, "invoice ext:pdf in:allowed", false, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "invoice999.pdf");
        let results = ask(provider, "\"type:image\"", false, 10);
        assert_eq!(results.len(), 1, "operators inside quotes are literal");
        assert_eq!(results[0].title, "type-image.md");
    }
}
