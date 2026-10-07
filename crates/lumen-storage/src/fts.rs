//! Safe FTS5 queries from raw user input.
//!
//! User text is never passed to `MATCH` verbatim: FTS5 syntax (`AND`, `NEAR`, `-`, `:`, `^`,
//! unbalanced quotes) would either error or change meaning. Every term is quoted; `"quoted
//! phrases"` stay phrases (docs/COMMAND_MODEL.md §7: quoted = stronger lexical/exact); the
//! last bare term becomes a prefix query while the user is still typing.

/// Shortest last term that is searched as a prefix while typing. One- and two-letter
/// prefixes match a large share of the vocabulary and force bm25 over most rows (T007
/// benchmark: >100 ms p95); filename search (T102) serves those keystrokes instead.
pub const MIN_PREFIX_CHARS: usize = 3;

/// A sanitized FTS5 `MATCH` expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtsQuery(String);

impl FtsQuery {
    /// Builds a query; `None` when the input has no searchable terms.
    ///
    /// `typing` = the user may still be typing the last word, so it is matched as a prefix
    /// (`retr` → `retr*`). Phrases are never prefixed.
    #[must_use]
    pub fn from_user(input: &str, typing: bool) -> Option<Self> {
        let mut parts: Vec<String> = Vec::new();
        let mut last_bare: Option<usize> = None;
        let mut rest = input;
        while let Some(start) = rest.find('"') {
            push_terms(&rest[..start], &mut parts, &mut last_bare);
            let after = &rest[start + 1..];
            if let Some(end) = after.find('"') {
                let phrase = clean(&after[..end]);
                if !phrase.is_empty() {
                    parts.push(format!("\"{phrase}\""));
                    last_bare = None;
                }
                rest = &after[end + 1..];
            } else {
                // Unbalanced quote: treat the remainder as plain terms.
                rest = after;
                break;
            }
        }
        push_terms(rest, &mut parts, &mut last_bare);
        if parts.is_empty() {
            return None;
        }
        let ends_with_space = input.ends_with(char::is_whitespace);
        if typing
            && !ends_with_space
            && let Some(i) = last_bare
            && i == parts.len() - 1
        {
            // `"abc"` → 3 chars inside the quotes.
            let chars = parts[i].chars().count().saturating_sub(2);
            if chars >= MIN_PREFIX_CHARS {
                parts[i].push('*');
            } else if parts.len() > 1 {
                // Too short to be useful yet: wait for more keystrokes.
                parts.pop();
            } else {
                return None;
            }
        }
        Some(Self(parts.join(" ")))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Keeps letters, digits and inner apostrophes/hyphens as spaces; FTS5 tokenizes the rest.
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_terms(s: &str, parts: &mut Vec<String>, last_bare: &mut Option<usize>) {
    for word in clean(s).split_whitespace() {
        parts.push(format!("\"{word}\""));
        *last_bare = Some(parts.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(input: &str, typing: bool) -> Option<String> {
        FtsQuery::from_user(input, typing).map(|q| q.0)
    }

    #[test]
    fn quotes_terms_and_prefixes_last_while_typing() {
        assert_eq!(
            q("retry http", true).as_deref(),
            Some("\"retry\" \"http\"*")
        );
        assert_eq!(
            q("retry http ", true).as_deref(),
            Some("\"retry\" \"http\"")
        );
        assert_eq!(
            q("retry http", false).as_deref(),
            Some("\"retry\" \"http\"")
        );
    }

    #[test]
    fn short_trailing_prefixes_wait_for_more_input() {
        assert_eq!(q("s", true), None);
        assert_eq!(q("sp", true), None);
        assert_eq!(q("spo", true).as_deref(), Some("\"spo\"*"));
        assert_eq!(q("docker c", true).as_deref(), Some("\"docker\""));
        // Not typing (or a finished word): short terms are kept as exact tokens.
        assert_eq!(q("docker c", false).as_deref(), Some("\"docker\" \"c\""));
        assert_eq!(q("go ", true).as_deref(), Some("\"go\""));
    }

    #[test]
    fn neutralizes_fts_syntax() {
        assert_eq!(
            q("NOT a AND b OR c:d -e ^f NEAR(g", false).as_deref(),
            Some("\"NOT\" \"a\" \"AND\" \"b\" \"OR\" \"c\" \"d\" \"e\" \"f\" \"NEAR\" \"g\"")
        );
        assert_eq!(q("   ", true), None);
        assert_eq!(q("!!! ???", true), None);
    }

    #[test]
    fn keeps_phrases_and_handles_unbalanced_quotes() {
        assert_eq!(
            q("docker \"connection refused\" port", true).as_deref(),
            Some("\"docker\" \"connection refused\" \"port\"*")
        );
        assert_eq!(
            q("\"exact phrase\"", true).as_deref(),
            Some("\"exact phrase\"")
        );
        assert_eq!(
            q("open \"quote", true).as_deref(),
            Some("\"open\" \"quote\"*")
        );
    }

    #[test]
    fn unicode_terms_survive() {
        assert_eq!(
            q("reunión cliente", false).as_deref(),
            Some("\"reunión\" \"cliente\"")
        );
        assert_eq!(q("日本語", false).as_deref(), Some("\"日本語\""));
    }
}
