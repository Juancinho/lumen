# Synthetic PDF fixtures (T301)

`mod.rs` builds text-only PDFs in memory using lopdf and a standard Helvetica font.
Tests and the `pdf_text` release benchmark save them only under their own temporary
directories. No user documents, font files or downloaded PDFs are used.

The extractor tests add a synthetic ToUnicode map, encryption and compressed-stream
limits; the content tests exercise blank physical pages, scope, durable queues, moves,
edits and cancellation. These fixtures test text extraction, not rendered layout.
