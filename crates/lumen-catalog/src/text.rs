//! Search-key normalization for names (docs/SEARCH_AND_INDEXING.md §3: case-insensitive like
//! Windows, accents ignored so "reunion" finds "Reunión").

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// Case-folded, diacritic-free, whitespace-collapsed form of `s`. Index keys and query
/// keys must both go through this function.
#[must_use]
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.nfkd().filter(|c| !is_combining_mark(*c)) {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.extend(c.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_case_accents_and_space() {
        assert_eq!(fold("Reunión  Cliente.DOCX"), "reunion cliente.docx");
        assert_eq!(fold("  Ñandú "), "nandu");
        assert_eq!(fold("ＦＵＬＬ"), "full", "compatibility forms");
        assert_eq!(
            fold("Straße"),
            "straße",
            "lowercase only, no special casing"
        );
        assert_eq!(fold("日本語"), "日本語");
        assert_eq!(fold(""), "");
    }
}
