//! Synthetic benchmark inputs. Latency depends on token counts, not meaning, so a
//! deterministic corpus is enough here; relevance evaluation is T205.

/// Representative root-search queries (docs/PRODUCT.md §4 and variants), incl.
/// Spanish and code intents. Short queries dominate real usage.
pub(crate) const QUERIES: &[&str] = &[
    "spotify",
    "bluetooth settings",
    "transformers paper",
    "screenshot docker connection refused",
    "python retry failed http requests",
    "clipboard api key template",
    "dev gestureos",
    "what was I editing Tuesday afternoon",
    "pdf where I explained gradient descent convergence",
    "the screenshot where VS Code had a Docker error",
    "invoice from march",
    "factura de la luz de marzo",
    "presentación del proyecto lumen",
    "notas de la reunión con el cliente",
    "rust async cancellation token",
    "function that parses command line arguments",
    "react hook to debounce input",
    "sql migration add index",
    "photos from the beach trip",
    "contract termination clause",
    "tax return 2025",
    "kubernetes ingress tls certificate",
    "meeting recording where we discussed pricing",
    "budget spreadsheet q3",
    "readme installation steps",
    "error ECONNREFUSED 127.0.0.1:5432",
    "how to reset network adapter",
    "receta de tortilla de patatas",
    "thesis chapter on time series forecasting",
    "logo svg dark version",
    "email draft to landlord",
    "unit tests for the ranking fusion",
];

/// Deterministic pseudo-document of about `words` words (xorshift64 over a vocabulary).
pub(crate) fn synthetic_document(seed: u64, words: usize) -> String {
    const VOCAB: &[&str] = &[
        "the",
        "index",
        "search",
        "result",
        "file",
        "model",
        "vector",
        "query",
        "latency",
        "windows",
        "overlay",
        "semantic",
        "document",
        "project",
        "meeting",
        "budget",
        "error",
        "connection",
        "retry",
        "request",
        "function",
        "returns",
        "value",
        "user",
        "local",
        "privacy",
        "embedding",
        "chunk",
        "page",
        "image",
        "screenshot",
        "code",
        "test",
        "and",
        "of",
        "to",
        "in",
        "with",
        "for",
        "is",
        "datos",
        "proyecto",
        "reunión",
        "factura",
        "de",
        "la",
        "el",
        "configuración",
        "rendimiento",
        "memoria",
    ];
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut out = String::with_capacity(words * 8);
    for i in 0..words {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        #[allow(clippy::cast_possible_truncation)]
        let word = VOCAB[(state % VOCAB.len() as u64) as usize];
        if i > 0 {
            out.push(if i % 17 == 0 { '.' } else { ' ' });
            if i % 17 == 0 {
                out.push(' ');
            }
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_are_deterministic_and_sized() {
        let a = synthetic_document(1, 200);
        assert_eq!(a, synthetic_document(1, 200));
        assert_ne!(a, synthetic_document(2, 200));
        let words = a.split_whitespace().count();
        assert_eq!(words, 200);
    }

    #[test]
    fn queries_are_nonempty() {
        assert!(QUERIES.len() >= 30);
        assert!(QUERIES.iter().all(|q| !q.trim().is_empty()));
    }
}
