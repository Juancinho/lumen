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

/// Vocabulary of [`synthetic_document`].
pub(crate) const VOCAB: &[&str] = &[
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

/// Deterministic pseudo-document of about `words` words (xorshift64 over a vocabulary).
pub(crate) fn synthetic_document(seed: u64, words: usize) -> String {
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

/// Multi-word queries made of [`VOCAB`] terms, so they match synthetic documents (the
/// realistic [`QUERIES`] mostly do not, which would time empty result sets).
pub(crate) fn vocabulary_queries(n: usize) -> Vec<String> {
    let v = VOCAB.len();
    (0..n)
        .map(|i| {
            let words = 1 + i % 3;
            (0..words)
                .map(|w| VOCAB[(i * 7 + w * 13 + 3) % v])
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// Distinct lowercase words of [`QUERIES`] (first-appearance order).
pub(crate) fn query_terms() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for q in QUERIES {
        for w in q
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
        {
            let w = w.to_lowercase();
            if !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out
}

/// Storage-benchmark corpus (T016): the [`VOCAB`] words plus every [`QUERIES`] term, drawn
/// with a Zipf distribution (rank `r` has weight `1 / (r + 1)^exponent`), so realistic
/// queries hit realistic numbers of chunks — frequent words many, rare terms few — instead
/// of none. The first ten VOCAB words (function words) take the top ranks; after them query
/// terms and the remaining VOCAB words alternate, spreading query terms over the head,
/// body and tail of the distribution.
pub(crate) struct ZipfCorpus {
    words: Vec<String>,
    cumulative: Vec<f64>,
}

impl ZipfCorpus {
    pub(crate) fn new(exponent: f64) -> Self {
        let terms = query_terms();
        let mut words: Vec<String> = VOCAB[..10].iter().map(|w| (*w).to_owned()).collect();
        let mut rest = VOCAB[10..].iter().map(|w| (*w).to_owned());
        let mut terms = terms.into_iter();
        loop {
            let (t, v) = (terms.next(), rest.next());
            if t.is_none() && v.is_none() {
                break;
            }
            for w in [t, v].into_iter().flatten() {
                if !words.contains(&w) {
                    words.push(w);
                }
            }
        }
        let mut total = 0.0;
        let cumulative = (0..words.len())
            .map(|r| {
                #[allow(clippy::cast_precision_loss)]
                let weight = 1.0 / ((r + 1) as f64).powf(exponent);
                total += weight;
                total
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|c| c / total)
            .collect();
        Self { words, cumulative }
    }

    pub(crate) fn vocabulary_size(&self) -> usize {
        self.words.len()
    }

    /// Deterministic document of `words` words.
    pub(crate) fn document(&self, seed: u64, words: usize) -> String {
        let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let mut out = String::with_capacity(words * 8);
        for i in 0..words {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            #[allow(clippy::cast_precision_loss)]
            let u = (state >> 11) as f64 / (1_u64 << 53) as f64;
            let r = self
                .cumulative
                .partition_point(|&c| c < u)
                .min(self.words.len() - 1);
            if i > 0 {
                out.push_str(if i % 17 == 0 { ". " } else { " " });
            }
            out.push_str(&self.words[r]);
        }
        out
    }
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
    fn vocabulary_queries_use_vocabulary_words() {
        let q = vocabulary_queries(9);
        assert_eq!(q.len(), 9);
        assert!(q.iter().all(|q| q.split(' ').all(|w| VOCAB.contains(&w))));
        assert_eq!(q[2].split(' ').count(), 3);
    }

    #[test]
    fn queries_are_nonempty() {
        assert!(QUERIES.len() >= 30);
        assert!(QUERIES.iter().all(|q| !q.trim().is_empty()));
    }

    #[test]
    fn zipf_corpus_contains_every_query_term_with_a_skew() {
        let c = ZipfCorpus::new(1.0);
        assert!(c.vocabulary_size() > VOCAB.len());
        let doc = (0..400)
            .map(|i| c.document(i, 120))
            .collect::<Vec<_>>()
            .join(" ");
        let count = |w: &str| {
            doc.split(|ch: char| !ch.is_alphanumeric())
                .filter(|t| *t == w)
                .count()
        };
        // Head words are far more frequent than tail terms, and query terms occur.
        assert!(count("the") > 20 * count("tortilla").max(1));
        assert!(count("spotify") > 0);
        assert_eq!(c.document(3, 50), c.document(3, 50));
        assert!(query_terms().contains(&"econnrefused".to_owned()));
    }
}
