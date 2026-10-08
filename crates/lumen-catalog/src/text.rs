//! Search-key normalization and tokenization for names and paths
//! (docs/SEARCH_AND_INDEXING.md §3: case-insensitive like Windows, accents ignored so
//! "reunion" finds "Reunión", code names split so "compo" finds "MyComponent.tsx").

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Lower,
    Upper,
    Digit,
    Other,
}

fn class(c: char) -> Class {
    if c.is_numeric() {
        Class::Digit
    } else if c.is_uppercase() {
        Class::Upper
    } else if c.is_alphabetic() {
        Class::Lower
    } else {
        Class::Other
    }
}

/// Word-like tokens of `s`, folded: split on non-alphanumerics, lower→upper case changes
/// (`myComponent` → `my`, `component`), acronym ends (`HTTPServer` → `http`, `server`) and
/// letter/digit changes (`q3budget2025` → `q`, `3`, `budget`, `2025`).
#[must_use]
pub fn tokens(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.nfc().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            let folded = fold(cur);
            if !folded.is_empty() {
                out.push(folded);
            }
            cur.clear();
        }
    };
    for (i, &c) in chars.iter().enumerate() {
        let k = class(c);
        if k == Class::Other {
            flush(&mut cur, &mut out);
            continue;
        }
        if let Some(&prev) = i.checked_sub(1).and_then(|j| chars.get(j)) {
            let p = class(prev);
            let next_lower = chars.get(i + 1).is_some_and(|&n| class(n) == Class::Lower);
            let boundary = match (p, k) {
                (Class::Lower, Class::Upper) => true,
                (Class::Upper, Class::Upper) => next_lower, // "HTTPServer": split before "Se"
                (Class::Digit, Class::Lower | Class::Upper)
                | (Class::Lower | Class::Upper, Class::Digit) => true,
                _ => false,
            };
            if boundary {
                flush(&mut cur, &mut out);
            }
        }
        cur.push(c);
    }
    flush(&mut cur, &mut out);
    out
}

/// Indexed token text of a name (`items.name_parts`): its tokens, the folded words they were
/// split from (so `mycomp` still prefixes `MyComponent`), and the initials of multi-word
/// names (`vsc` for "Visual Studio Code").
#[must_use]
pub fn name_parts(name: &str) -> String {
    let parts = tokens(name);
    let mut out: Vec<String> = parts.clone();
    for word in fold(name).split(|c: char| !c.is_alphanumeric()) {
        if !word.is_empty() && !out.iter().any(|p| p == word) {
            out.push(word.to_owned());
        }
    }
    // Initials come from the stem: "MyComponent.tsx" -> "mc", not "mct".
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && ext.len() <= 5 && ext.chars().all(char::is_alphanumeric) =>
        {
            stem
        }
        _ => name,
    };
    let stem_parts = tokens(stem);
    let alphabetic: Vec<&String> = stem_parts
        .iter()
        .filter(|p| p.chars().next().is_some_and(char::is_alphabetic))
        .collect();
    if alphabetic.len() >= 2 {
        let initials: String = alphabetic.iter().filter_map(|p| p.chars().next()).collect();
        if !out.contains(&initials) {
            out.push(initials);
        }
    }
    out.join(" ")
}

/// Indexed text of the folders holding an item (`items.path_parts`): tokens of the last
/// `segments` parent directory names, nearest last.
#[must_use]
pub fn path_parts(path: &str, segments: usize) -> String {
    let parents: Vec<&str> = path
        .split(['\\', '/'])
        .filter(|s| !s.is_empty() && !s.ends_with(':'))
        .collect();
    let dirs = parents.len().saturating_sub(1);
    let start = dirs.saturating_sub(segments);
    parents[start..dirs]
        .iter()
        .flat_map(|d| tokens(d))
        .collect::<Vec<_>>()
        .join(" ")
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

    #[test]
    fn tokens_split_code_and_numbers() {
        assert_eq!(
            tokens("MyComponentName.tsx"),
            ["my", "component", "name", "tsx"]
        );
        assert_eq!(tokens("HTTPServer_config"), ["http", "server", "config"]);
        assert_eq!(
            tokens("q3budget2025.xlsx"),
            ["q", "3", "budget", "2025", "xlsx"]
        );
        assert_eq!(tokens("Presupuesto Reunión"), ["presupuesto", "reunion"]);
        assert_eq!(tokens("  --- "), Vec::<String>::new());
        assert_eq!(tokens("日本語のファイル"), ["日本語のファイル"]);
    }

    #[test]
    fn name_parts_add_words_and_initials() {
        assert_eq!(name_parts("Visual Studio Code"), "visual studio code vsc");
        assert_eq!(
            name_parts("MyComponent.tsx"),
            "my component tsx mycomponent mc"
        );
        assert_eq!(name_parts("notes.txt"), "notes txt");
        assert_eq!(name_parts("2025 report"), "2025 report");
    }

    #[test]
    fn path_parts_take_the_nearest_folders() {
        assert_eq!(
            path_parts(r"C:\Users\Joao\Proyectos\lumen\notas.md", 3),
            "joao proyectos lumen"
        );
        assert_eq!(path_parts("/home/joao/doc.txt", 3), "home joao");
        assert_eq!(path_parts("file.txt", 3), "");
    }
}
