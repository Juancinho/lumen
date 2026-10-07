//! Task prompts. The exact strings are part of the embedding space: changing them
//! requires a new `PromptFormat` version and therefore a new index generation.

use std::fmt;

/// What the text is for. Retrieval models embed queries and documents differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingTask {
    /// A user search query (root search).
    SearchQuery,
    /// An indexed chunk (file text, code, PDF page text).
    SearchDocument,
}

/// One text to embed. `title` is optional document context (e.g. file name).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TextInput<'a> {
    pub text: &'a str,
    pub title: Option<&'a str>,
}

impl<'a> TextInput<'a> {
    #[must_use]
    pub const fn new(text: &'a str) -> Self {
        Self { text, title: None }
    }

    #[must_use]
    pub const fn with_title(text: &'a str, title: &'a str) -> Self {
        Self {
            text,
            title: Some(title),
        }
    }
}

impl fmt::Debug for TextInput<'_> {
    /// Never prints content (privacy: inputs may be file text or queries).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextInput")
            .field("text_len", &self.text.len())
            .field("has_title", &self.title.is_some())
            .finish()
    }
}

/// Versioned prompt scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PromptFormat {
    pub id: &'static str,
    pub version: u32,
    /// Prepended to queries.
    query_prefix: &'static str,
    /// Document form: `{doc_title_prefix}{title or doc_no_title}{doc_text_sep}{text}`.
    /// All empty = raw text.
    doc_title_prefix: &'static str,
    doc_no_title: &'static str,
    doc_text_sep: &'static str,
}

impl PromptFormat {
    /// Retrieval prompts published for EmbeddingGemma (v1 model card):
    /// query `task: search result | query: {q}`,
    /// document `title: {title | "none"} | text: {t}`.
    ///
    /// T006 MUST verify these against the EmbeddingGemma 2 model card before any
    /// persistent index is built; if they differ, add a new version, do not edit this one.
    pub const EMBEDDINGGEMMA_RETRIEVAL_V1: Self = Self {
        id: "embeddinggemma-retrieval",
        version: 1,
        query_prefix: "task: search result | query: ",
        doc_title_prefix: "title: ",
        doc_no_title: "none",
        doc_text_sep: " | text: ",
    };

    /// No prompts: raw text in, for mocks and models without task prompts.
    pub const RAW: Self = Self {
        id: "raw",
        version: 1,
        query_prefix: "",
        doc_title_prefix: "",
        doc_no_title: "",
        doc_text_sep: "",
    };

    /// Formats one input for `task`.
    #[must_use]
    pub fn format(&self, task: EmbeddingTask, input: TextInput<'_>) -> String {
        match task {
            EmbeddingTask::SearchQuery => {
                let mut s = String::with_capacity(self.query_prefix.len() + input.text.len());
                s.push_str(self.query_prefix);
                s.push_str(input.text);
                s
            }
            EmbeddingTask::SearchDocument if self.doc_title_prefix.is_empty() => {
                input.text.to_owned()
            }
            EmbeddingTask::SearchDocument => {
                let title = input
                    .title
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .unwrap_or(self.doc_no_title);
                [self.doc_title_prefix, title, self.doc_text_sep, input.text].concat()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEMMA: PromptFormat = PromptFormat::EMBEDDINGGEMMA_RETRIEVAL_V1;

    #[test]
    fn embeddinggemma_query_prompt() {
        assert_eq!(
            GEMMA.format(
                EmbeddingTask::SearchQuery,
                TextInput::new("docker connection refused")
            ),
            "task: search result | query: docker connection refused"
        );
    }

    #[test]
    fn embeddinggemma_document_prompt() {
        assert_eq!(
            GEMMA.format(
                EmbeddingTask::SearchDocument,
                TextInput::with_title("Retry with backoff.", "retry.py")
            ),
            "title: retry.py | text: Retry with backoff."
        );
        assert_eq!(
            GEMMA.format(
                EmbeddingTask::SearchDocument,
                TextInput::with_title("x", "  ")
            ),
            "title: none | text: x"
        );
        assert_eq!(
            GEMMA.format(EmbeddingTask::SearchDocument, TextInput::new("x")),
            "title: none | text: x"
        );
    }

    #[test]
    fn raw_prompt_is_identity() {
        for task in [EmbeddingTask::SearchQuery, EmbeddingTask::SearchDocument] {
            assert_eq!(
                PromptFormat::RAW.format(task, TextInput::with_title("hola", "t")),
                "hola"
            );
        }
    }

    #[test]
    fn debug_does_not_leak_content() {
        let shown = format!(
            "{:?}",
            TextInput::with_title("secret api key", "passwords.txt")
        );
        assert!(
            !shown.contains("secret") && !shown.contains("passwords"),
            "{shown}"
        );
    }
}
