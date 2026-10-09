//! Root query syntax (T208). Parsing is local, deterministic and shell independent.

use std::borrow::Cow;

/// Hard metadata constraints, combined with AND, including repeated operators.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryFilters(pub Vec<QueryFilter>);

impl QueryFilters {
    /// Only file entities have indexed passages. Avoid FTS/model work for app/folder
    /// requests while leaving their instant catalog results available.
    #[must_use]
    pub fn allows_file_content(&self) -> bool {
        !self.0.iter().any(|filter| {
            matches!(
                filter,
                QueryFilter::Type(QueryType::Folder | QueryType::Application)
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryFilter {
    Type(QueryType),
    Extension(String),
    /// Slash-normalized path or contiguous directory components, ASCII case folded.
    In(String),
    /// Exclusive UTC start of a calendar day, in Unix milliseconds.
    Before(i64),
    /// Inclusive UTC start of the following calendar day, in Unix milliseconds.
    After(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    File,
    Folder,
    Application,
    Document,
    Image,
    Audio,
    Video,
    Code,
}

impl QueryType {
    /// File categories describe inventory metadata, not extraction/model coverage.
    #[must_use]
    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Document => &[
                "txt", "md", "markdown", "pdf", "doc", "docx", "odt", "rtf", "csv", "xls", "xlsx",
                "ppt", "pptx",
            ],
            Self::Image => &[
                "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "heic", "avif", "svg",
            ],
            Self::Audio => &["mp3", "wav", "flac", "aac", "ogg", "m4a", "wma", "opus"],
            Self::Video => &[
                "mp4", "mkv", "avi", "mov", "wmv", "webm", "m4v", "mpeg", "mpg",
            ],
            Self::Code => &[
                "rs", "py", "js", "jsx", "ts", "tsx", "c", "h", "cpp", "hpp", "cs", "java", "go",
                "rb", "php", "swift", "kt", "kts", "sh", "ps1", "sql", "html", "css", "scss",
                "vue", "svelte",
            ],
            Self::File | Self::Folder | Self::Application => &[],
        }
    }
}

/// Recognized invalid/incomplete operators fail closed until corrected. Unknown `key:`
/// text remains literal (URLs, drive letters and code are ordinary search terms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery<'a> {
    pub text: Cow<'a, str>,
    pub filters: QueryFilters,
    pub phrases: Vec<String>,
    pub valid: bool,
}

impl<'a> SearchQuery<'a> {
    #[must_use]
    pub fn parse(input: &'a str) -> Self {
        let mut query = Self {
            text: Cow::Borrowed(input),
            filters: QueryFilters::default(),
            phrases: Vec::new(),
            valid: true,
        };
        if !input.contains(':') && !input.contains('"') {
            return query;
        }
        let mut terms = Vec::new();
        let mut rest = input;
        let mut removed = false;
        while !rest.is_empty() {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            let mut quoted = false;
            let end = rest
                .char_indices()
                .find_map(|(i, c)| {
                    if c == '"' {
                        quoted = !quoted;
                    }
                    (c.is_whitespace() && !quoted).then_some(i)
                })
                .unwrap_or(rest.len());
            let token = &rest[..end];
            rest = &rest[end..];
            let operator = token.split_once(':').filter(|(key, _)| {
                ["type", "ext", "in", "before", "after"]
                    .iter()
                    .any(|k| key.eq_ignore_ascii_case(k))
            });
            if let Some((key, raw)) = operator {
                removed = true;
                let value = if raw.starts_with('"') {
                    raw.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
                } else {
                    Some(raw)
                };
                let filter = value
                    .filter(|v| !v.is_empty())
                    .and_then(|value| parse_filter(key, value));
                if let Some(filter) = filter.filter(|_| query.filters.0.len() < 16) {
                    query.filters.0.push(filter);
                } else {
                    query.valid = false;
                }
            } else {
                // An open quote retains existing FTS behavior while typing.
                let mut quoted_rest = token;
                while let Some(start) = quoted_rest.find('"') {
                    let after = &quoted_rest[start + 1..];
                    let Some(end) = after.find('"') else {
                        break;
                    };
                    if !after[..end].trim().is_empty() {
                        query.phrases.push(after[..end].to_owned());
                    }
                    quoted_rest = &after[end + 1..];
                }
                terms.push(token);
            }
        }
        if removed {
            let mut text = terms.join(" ");
            if input.ends_with(char::is_whitespace) {
                text.push(' ');
            }
            query.text = Cow::Owned(text);
        }
        query
    }
}

fn parse_filter(key: &str, value: &str) -> Option<QueryFilter> {
    match key.to_ascii_lowercase().as_str() {
        "type" => Some(QueryFilter::Type(
            match value.to_ascii_lowercase().as_str() {
                "file" => QueryType::File,
                "folder" => QueryType::Folder,
                "app" | "application" => QueryType::Application,
                "document" => QueryType::Document,
                "image" => QueryType::Image,
                "audio" => QueryType::Audio,
                "video" => QueryType::Video,
                "code" => QueryType::Code,
                _ => return None,
            },
        )),
        "ext" => {
            let ext = value.strip_prefix('.').unwrap_or(value);
            (!ext.is_empty() && ext.chars().all(char::is_alphanumeric))
                .then(|| QueryFilter::Extension(ext.to_ascii_lowercase()))
        }
        "in" => {
            let path = value.replace('\\', "/").to_ascii_lowercase();
            let path = if path == "/" {
                path
            } else {
                path.trim_end_matches('/').to_owned()
            };
            (!path.is_empty()
                && !path.split('/').any(|p| p == "." || p == "..")
                && !path.contains('"'))
            .then_some(QueryFilter::In(path))
        }
        "before" => day_ms(value).map(QueryFilter::Before),
        "after" => day_ms(value).map(|day| QueryFilter::After(day + 86_400_000)),
        _ => None,
    }
}

/// Gregorian dates, with no timezone dependency or date parsing heuristics.
fn day_ms(value: &str) -> Option<i64> {
    if value.len() != 10
        || !value.is_ascii()
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || value
            .bytes()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return None;
    }
    let year: i64 = value[..4].parse().ok()?;
    let month: i64 = value[5..7].parse().ok()?;
    let day: i64 = value[8..].parse().ok()?;
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=days).contains(&day) {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    Some((era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468) * 86_400_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_text_and_unknown_operators_are_preserved() {
        for input in [
            "what is a beach",
            "https://host/path C:\\docs",
            "language:rust",
        ] {
            let q = SearchQuery::parse(input);
            assert_eq!(q.text, input);
            assert!(q.valid && q.filters.0.is_empty());
        }
    }

    #[test]
    fn extracts_filters_without_touching_quotes_or_windows_paths() {
        let q = SearchQuery::parse(
            "contrato EXT:.PDF in:\"D:\\Mis documentos\" before:2026-10-01 \"type:image\" ",
        );
        assert!(q.valid);
        assert_eq!(q.text, "contrato \"type:image\" ");
        assert_eq!(q.phrases, ["type:image"]);
        assert_eq!(
            q.filters.0,
            [
                QueryFilter::Extension("pdf".into()),
                QueryFilter::In("d:/mis documentos".into()),
                QueryFilter::Before(1_790_812_800_000)
            ]
        );
    }

    #[test]
    fn invalid_known_filters_fail_closed_and_dates_are_exclusive() {
        for text in [
            "type:",
            "type:unknown",
            "ext:p*",
            "in:\"open",
            "in:../docs",
            "before:2025-02-29",
            "after:2026-13-01",
            "before:2026-1-01",
            "before:2026-+1-01",
        ] {
            assert!(!SearchQuery::parse(text).valid, "{text}");
        }
        assert_eq!(day_ms("1970-01-01"), Some(0));
        assert_eq!(day_ms("2000-02-29"), Some(951_782_400_000));
        assert!(day_ms("1900-02-29").is_none());
        assert_eq!(
            SearchQuery::parse("after:1970-01-01").filters.0,
            [QueryFilter::After(86_400_000)]
        );
        assert!(!SearchQuery::parse(&"ext:pdf ".repeat(17)).valid);
    }
}
