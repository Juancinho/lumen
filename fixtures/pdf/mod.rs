//! Synthetic PDF fixture builder; shared by T301 tests/bench, never production.
use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};

pub(super) fn document(pages: &[&[u8]]) -> Document {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
    let mut kids = Vec::new();
    for text in pages {
        let content = Content {
            operations: vec![
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![50.into(), 750.into()]),
                Operation::new(
                    "Tj",
                    vec![Object::String(text.to_vec(), lopdf::StringFormat::Literal)],
                ),
                Operation::new("ET", vec![]),
            ],
        };
        let mut stream = Stream::new(dictionary! {}, content.encode().expect("synthetic content"));
        stream.compress().expect("synthetic compression");
        let contents = doc.add_object(stream);
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => contents,
            "Resources" => resources, "MediaBox" => vec![0.into(),0.into(),595.into(),842.into()],
        });
        kids.push(page.into());
    }
    doc.objects.insert(
        pages_id,
        dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages.len() as i64,
        }
        .into(),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.trailer.set(
        "ID",
        vec![
            Object::string_literal("lumen-synthetic-pdf-id"),
            Object::string_literal("lumen-synthetic-pdf-id"),
        ],
    );
    doc
}
