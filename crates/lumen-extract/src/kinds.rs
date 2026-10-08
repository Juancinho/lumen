/// Programming language of a code file (chunking heuristics and metadata).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    CSharp,
    Java,
    Kotlin,
    Go,
    C,
    Cpp,
    Swift,
    Php,
    Ruby,
    Lua,
    Sql,
    Shell,
    PowerShell,
    Batch,
    Html,
    Css,
}

impl Language {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::CSharp => "csharp",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Go => "go",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Swift => "swift",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Lua => "lua",
            Self::Sql => "sql",
            Self::Shell => "shell",
            Self::PowerShell => "powershell",
            Self::Batch => "batch",
            Self::Html => "html",
            Self::Css => "css",
        }
    }
}

/// How a text file is chunked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DocKind {
    /// Paragraphs and sentences.
    Prose,
    /// Heading sections; fences kept intact.
    Markdown,
    Code(Language),
    /// Structured data (JSON, CSV, YAML, …): line windows.
    Data,
}

/// Every extension [`kind_for_extension`] accepts (lowercase): what the content pass asks
/// the catalog for.
pub const TEXT_EXTENSIONS: &[&str] = &[
    "txt",
    "text",
    "log",
    "rst",
    "srt",
    "vtt",
    "tex",
    "org",
    "adoc",
    "md",
    "markdown",
    "mdx",
    "json",
    "jsonc",
    "csv",
    "tsv",
    "yaml",
    "yml",
    "toml",
    "ini",
    "cfg",
    "conf",
    "xml",
    "env",
    "properties",
    "rs",
    "py",
    "pyw",
    "pyi",
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "mts",
    "cts",
    "tsx",
    "cs",
    "java",
    "kt",
    "kts",
    "go",
    "c",
    "h",
    "cc",
    "cpp",
    "cxx",
    "hpp",
    "hh",
    "hxx",
    "swift",
    "php",
    "rb",
    "lua",
    "sql",
    "sh",
    "bash",
    "zsh",
    "ps1",
    "psm1",
    "psd1",
    "bat",
    "cmd",
    "html",
    "htm",
    "vue",
    "svelte",
    "css",
    "scss",
    "less",
];

/// The document kind for a lowercase-insensitive extension (without the dot); `None` for
/// files that are not plain text (they are catalogued by name only).
#[must_use]
pub fn kind_for_extension(ext: &str) -> Option<DocKind> {
    use Language as L;
    let kind = match ext.to_ascii_lowercase().as_str() {
        "txt" | "text" | "log" | "rst" | "srt" | "vtt" | "tex" | "org" | "adoc" => DocKind::Prose,
        "md" | "markdown" | "mdx" => DocKind::Markdown,
        "json" | "jsonc" | "csv" | "tsv" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf"
        | "xml" | "env" | "properties" => DocKind::Data,
        "rs" => DocKind::Code(L::Rust),
        "py" | "pyw" | "pyi" => DocKind::Code(L::Python),
        "js" | "mjs" | "cjs" | "jsx" => DocKind::Code(L::JavaScript),
        "ts" | "mts" | "cts" | "tsx" => DocKind::Code(L::TypeScript),
        "cs" => DocKind::Code(L::CSharp),
        "java" => DocKind::Code(L::Java),
        "kt" | "kts" => DocKind::Code(L::Kotlin),
        "go" => DocKind::Code(L::Go),
        "c" | "h" => DocKind::Code(L::C),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => DocKind::Code(L::Cpp),
        "swift" => DocKind::Code(L::Swift),
        "php" => DocKind::Code(L::Php),
        "rb" => DocKind::Code(L::Ruby),
        "lua" => DocKind::Code(L::Lua),
        "sql" => DocKind::Code(L::Sql),
        "sh" | "bash" | "zsh" => DocKind::Code(L::Shell),
        "ps1" | "psm1" | "psd1" => DocKind::Code(L::PowerShell),
        "bat" | "cmd" => DocKind::Code(L::Batch),
        "html" | "htm" | "vue" | "svelte" => DocKind::Code(L::Html),
        "css" | "scss" | "less" => DocKind::Code(L::Css),
        _ => return None,
    };
    Some(kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_extensions_lists_exactly_the_accepted_ones() {
        assert!(
            TEXT_EXTENSIONS
                .iter()
                .all(|e| kind_for_extension(e).is_some())
        );
        let mut unique = TEXT_EXTENSIONS.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), TEXT_EXTENSIONS.len());
    }

    #[test]
    fn extensions_map_to_kinds() {
        assert_eq!(kind_for_extension("MD"), Some(DocKind::Markdown));
        assert_eq!(
            kind_for_extension("rs"),
            Some(DocKind::Code(Language::Rust))
        );
        assert_eq!(
            kind_for_extension("tsx"),
            Some(DocKind::Code(Language::TypeScript))
        );
        assert_eq!(kind_for_extension("csv"), Some(DocKind::Data));
        assert_eq!(kind_for_extension("txt"), Some(DocKind::Prose));
        assert_eq!(kind_for_extension("pdf"), None);
        assert_eq!(kind_for_extension("docx"), None);
        assert_eq!(Language::CSharp.as_str(), "csharp");
    }
}
