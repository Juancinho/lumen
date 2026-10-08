use crate::decode::Extracted;
use crate::kinds::{DocKind, Language};

/// Token counting for chunk sizes. T202 implements it with the embedding model's tokenizer;
/// [`EstimateTokens`] approximates it without one.
pub trait TokenCount {
    fn count(&self, text: &str) -> usize;
}

/// Dependency-free estimate close to subword tokenizers: each run of letters/digits costs
/// one token per started 5 characters, every other visible character one token,
/// whitespace nothing. Calibrated with `lumen-bench chunk --tokenizer` against the
/// EmbeddingGemma 2 tokenizer (T201): estimate/real p50 1.12–1.15, p95 1.35–1.45 on this
/// repository's code and Markdown, i.e. slightly conservative.
#[derive(Debug, Clone, Copy, Default)]
pub struct EstimateTokens;

impl TokenCount for EstimateTokens {
    fn count(&self, text: &str) -> usize {
        let mut tokens = 0;
        let mut run = 0usize;
        for c in text.chars() {
            if c.is_alphanumeric() {
                run += 1;
                continue;
            }
            tokens += run.div_ceil(5);
            run = 0;
            if !c.is_whitespace() {
                tokens += 1;
            }
        }
        tokens + run.div_ceil(5)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkConfig {
    /// Chunks are packed up to this size (ADR-015: ~128 tokens doubles throughput vs ~260).
    pub target_tokens: usize,
    /// A single unit above this is split further (sentences, lines, words).
    pub max_tokens: usize,
    /// Lines repeated between consecutive line windows of one oversized code block.
    pub overlap_lines: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            target_tokens: 128,
            max_tokens: 192,
            overlap_lines: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkKind {
    Text,
    Code,
}

impl ChunkKind {
    /// Value of `chunks.chunk_kind`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Code => "code",
        }
    }
}

/// One retrieval chunk: a byte range of the extracted text plus metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub ordinal: u32,
    pub kind: ChunkKind,
    /// Byte offsets into [`Extracted::text`] (`start..end`, on char boundaries).
    pub start: usize,
    pub end: usize,
    pub tokens: usize,
    /// Code: the definition the chunk holds (`parse_args`, `Store`).
    pub symbol: Option<String>,
    /// Markdown heading path (`Install › Windows`) or the enclosing code symbol.
    pub context: Option<String>,
}

impl Chunk {
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.start..self.end]
    }
}

/// A unit of text that the packer may combine with its neighbours.
#[derive(Debug, Clone)]
struct Unit {
    start: usize,
    end: usize,
    symbol: Option<String>,
    /// Units with different groups never share a chunk (Markdown sections).
    group: usize,
    context: Option<String>,
}

/// Splits `doc` into retrieval chunks. Empty or whitespace-only text gives none.
#[must_use]
pub fn chunk(doc: &Extracted, cfg: &ChunkConfig, counter: &dyn TokenCount) -> Vec<Chunk> {
    let text = doc.text.as_str();
    let c = Ctx { text, cfg, counter };
    let (units, kind) = match doc.kind {
        DocKind::Prose => (c.prose_units(0, text.len(), 0, None), ChunkKind::Text),
        DocKind::Markdown => (c.markdown_units(), ChunkKind::Text),
        DocKind::Code(lang) => (c.code_units(0, text.len(), lang, None), ChunkKind::Code),
        DocKind::Data => (c.line_units(0, text.len(), 0, None), ChunkKind::Code),
    };
    c.pack(&units, kind)
}

struct Ctx<'a> {
    text: &'a str,
    cfg: &'a ChunkConfig,
    counter: &'a dyn TokenCount,
}

/// Byte ranges of the lines of `text[start..end]` (without the `\n`).
fn lines(text: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut s = start;
    for (i, b) in text.as_bytes()[start..end].iter().enumerate() {
        if *b == b'\n' {
            out.push((s, start + i));
            s = start + i + 1;
        }
    }
    if s < end {
        out.push((s, end));
    }
    out
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn indent(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// Trims surrounding whitespace off a byte range (keeps char boundaries).
fn trim(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let s = &text[start..end];
    let lead = s.len() - s.trim_start().len();
    let trail = s.len() - s.trim_end().len();
    (lead + trail < s.len()).then(|| (start + lead, end - trail))
}

impl Ctx<'_> {
    fn tokens(&self, start: usize, end: usize) -> usize {
        self.counter.count(&self.text[start..end])
    }

    fn unit(&self, start: usize, end: usize, group: usize, context: Option<&str>) -> Option<Unit> {
        trim(self.text, start, end).map(|(start, end)| Unit {
            start,
            end,
            symbol: None,
            group,
            context: context.map(str::to_owned),
        })
    }

    // --- prose ---------------------------------------------------------------------------

    /// Paragraphs (blank-line separated); oversized ones split into sentences, then words.
    fn prose_units(&self, start: usize, end: usize, group: usize, ctx: Option<&str>) -> Vec<Unit> {
        let mut out = Vec::new();
        let mut para: Option<(usize, usize)> = None;
        let flush = |para: &mut Option<(usize, usize)>, out: &mut Vec<Unit>| {
            if let Some((s, e)) = para.take() {
                out.extend(self.sized(s, e, group, ctx, Split::Sentences));
            }
        };
        for (s, e) in lines(self.text, start, end) {
            if is_blank(&self.text[s..e]) {
                flush(&mut para, &mut out);
            } else {
                para = Some(para.map_or((s, e), |(ps, _)| (ps, e)));
            }
        }
        flush(&mut para, &mut out);
        out
    }

    /// `start..end` as one unit if it fits `max_tokens`, else split by `how`.
    fn sized(
        &self,
        start: usize,
        end: usize,
        group: usize,
        ctx: Option<&str>,
        how: Split,
    ) -> Vec<Unit> {
        let Some((start, end)) = trim(self.text, start, end) else {
            return Vec::new();
        };
        if self.tokens(start, end) <= self.cfg.max_tokens {
            return self.unit(start, end, group, ctx).into_iter().collect();
        }
        match how {
            Split::Sentences => {
                let pieces = self.sentence_ranges(start, end);
                if pieces.len() > 1 {
                    return pieces
                        .into_iter()
                        .flat_map(|(s, e)| self.sized(s, e, group, ctx, Split::Words))
                        .collect();
                }
                self.word_units(start, end, group, ctx)
            }
            Split::Lines => {
                let ls = lines(self.text, start, end);
                if ls.len() > 1 {
                    return ls
                        .into_iter()
                        .flat_map(|(s, e)| self.sized(s, e, group, ctx, Split::Words))
                        .collect();
                }
                self.word_units(start, end, group, ctx)
            }
            Split::Words => self.word_units(start, end, group, ctx),
        }
    }

    /// Sentence ranges: after `.`, `!`, `?` (or `…`) followed by whitespace, and at line ends.
    fn sentence_ranges(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let s = &self.text[start..end];
        let mut out = Vec::new();
        let mut from = 0;
        let mut it = s.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            let next_ws = it.peek().is_none_or(|(_, n)| n.is_whitespace());
            if c == '\n' || (matches!(c, '.' | '!' | '?' | '…') && next_ws) {
                let to = i + c.len_utf8();
                if trim(s, from, to).is_some() {
                    out.push((start + from, start + to));
                }
                from = to;
            }
        }
        if trim(s, from, s.len()).is_some() {
            out.push((start + from, end));
        }
        out
    }

    /// Word groups of at most `max_tokens`; a single huge "word" is cut by characters.
    fn word_units(&self, start: usize, end: usize, group: usize, ctx: Option<&str>) -> Vec<Unit> {
        let s = &self.text[start..end];
        let mut out = Vec::new();
        let mut piece_start: Option<usize> = None;
        let mut piece_end = 0;
        let mut words = s
            .split_inclusive(char::is_whitespace)
            .scan(0usize, |pos, w| {
                let at = *pos;
                *pos += w.len();
                Some((at, at + w.len()))
            });
        for (ws, we) in words.by_ref() {
            let candidate = piece_start.unwrap_or(ws);
            if piece_start.is_some()
                && self.tokens(start + candidate, start + we) > self.cfg.max_tokens
            {
                out.extend(self.unit(start + candidate, start + piece_end, group, ctx));
                piece_start = None;
            }
            if self.tokens(start + ws, start + we) > self.cfg.max_tokens {
                // One enormous token run (base64, minified line): cut by characters.
                out.extend(self.char_units(start + ws, start + we, group, ctx));
                piece_start = None;
                continue;
            }
            piece_start = Some(piece_start.unwrap_or(ws));
            piece_end = we;
        }
        if let Some(ps) = piece_start {
            out.extend(self.unit(start + ps, start + piece_end, group, ctx));
        }
        out
    }

    fn char_units(&self, start: usize, end: usize, group: usize, ctx: Option<&str>) -> Vec<Unit> {
        let s = &self.text[start..end];
        let mut out = Vec::new();
        let mut from = 0;
        let mut last = 0;
        for (i, c) in s.char_indices() {
            let to = i + c.len_utf8();
            if self.tokens(start + from, start + to) > self.cfg.max_tokens && last > from {
                out.extend(self.unit(start + from, start + last, group, ctx));
                from = last;
            }
            last = to;
        }
        out.extend(self.unit(start + from, end, group, ctx));
        out
    }

    // --- markdown ------------------------------------------------------------------------

    /// Sections from each heading to the next; fenced blocks are atomic units; a section
    /// that holds only headings so far is not cut (no heading-only chunks).
    fn markdown_units(&self) -> Vec<Unit> {
        let text = self.text;
        let mut out = Vec::new();
        let mut path: Vec<(usize, String)> = Vec::new();
        let mut group = 0;
        let mut block_start: Option<usize> = None; // current paragraph start
        let mut block_end = 0;
        let mut fence: Option<(String, usize)> = None; // marker, start
        let mut body_seen = false;
        let context = |path: &[(usize, String)]| {
            (!path.is_empty()).then(|| {
                path.iter()
                    .map(|(_, t)| t.as_str())
                    .collect::<Vec<_>>()
                    .join(" › ")
            })
        };
        for (s, e) in lines(text, 0, text.len()) {
            let line = &text[s..e];
            let t = line.trim_start();
            if let Some((marker, fs)) = &fence {
                if t.starts_with(marker.as_str()) {
                    let ctx = context(&path);
                    out.extend(self.sized(*fs, e, group, ctx.as_deref(), Split::Lines));
                    fence = None;
                }
                continue;
            }
            if t.starts_with("```") || t.starts_with("~~~") {
                if let Some(bs) = block_start.take() {
                    let ctx = context(&path);
                    out.extend(self.sized(bs, block_end, group, ctx.as_deref(), Split::Sentences));
                }
                fence = Some((t[..3].to_owned(), s));
                body_seen = true;
                continue;
            }
            let level = t.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&level) && t[level..].starts_with([' ', '\t'])
                || (1..=6).contains(&level) && t.len() == level
            {
                if let Some(bs) = block_start.take() {
                    let ctx = context(&path);
                    out.extend(self.sized(bs, block_end, group, ctx.as_deref(), Split::Sentences));
                }
                if body_seen {
                    group += 1;
                    body_seen = false;
                }
                let title = t[level..].trim().trim_end_matches('#').trim().to_owned();
                path.retain(|(l, _)| *l < level);
                path.push((level, title));
                let ctx = context(&path);
                out.extend(self.unit(s, e, group, ctx.as_deref()));
                continue;
            }
            if is_blank(line) {
                if let Some(bs) = block_start.take() {
                    let ctx = context(&path);
                    out.extend(self.sized(bs, block_end, group, ctx.as_deref(), Split::Sentences));
                }
            } else {
                body_seen = true;
                block_start.get_or_insert(s);
                block_end = e;
            }
        }
        let ctx = context(&path);
        if let Some((_, fs)) = fence {
            // Unclosed fence: keep it as code lines.
            out.extend(self.sized(fs, text.len(), group, ctx.as_deref(), Split::Lines));
        }
        if let Some(bs) = block_start {
            out.extend(self.sized(bs, block_end, group, ctx.as_deref(), Split::Sentences));
        }
        out
    }

    // --- code ----------------------------------------------------------------------------

    /// Top-level regions: a non-blank line at the block's base indentation after a blank
    /// line starts a new region, unless it closes the previous one (`}`, `)`, `end`…).
    /// Oversized regions are split one indentation level deeper (methods of a class), then
    /// into overlapping line windows.
    fn code_units(
        &self,
        start: usize,
        end: usize,
        lang: Language,
        parent: Option<&str>,
    ) -> Vec<Unit> {
        let regions = self.regions(start, end);

        let mut out = Vec::new();
        for (s, e) in regions {
            let symbol = symbol_of(&self.text[s..e], lang);
            if self.tokens(s, e) <= self.cfg.max_tokens {
                out.extend(self.unit(s, e, 0, parent).map(|mut u| {
                    u.symbol.clone_from(&symbol);
                    u
                }));
                continue;
            }
            // Split inside: the header line(s) go with the first inner region.
            let inner_start = lines(self.text, s, e).get(1).map_or(e, |&(ls2, _)| ls2);
            // Recurse only when the body has several regions of its own (methods of a
            // class); one long body becomes line windows.
            let inner = if inner_start < e && self.regions(inner_start, e).len() > 1 {
                self.code_units(inner_start, e, lang, symbol.as_deref().or(parent))
            } else {
                Vec::new()
            };
            if inner.len() > 1 {
                let mut inner = inner;
                inner[0].start = s;
                if inner[0].symbol.is_none() {
                    inner[0].symbol.clone_from(&symbol);
                }
                // The merged first unit may now exceed max: re-split it as lines.
                if self.tokens(inner[0].start, inner[0].end) > self.cfg.max_tokens {
                    let first = inner.remove(0);
                    let mut windows =
                        self.line_windows(first.start, first.end, first.context.as_deref());
                    for w in &mut windows {
                        w.symbol.clone_from(&first.symbol);
                    }
                    out.extend(windows);
                }
                out.extend(inner);
            } else {
                let mut windows = self.line_windows(s, e, parent);
                for w in &mut windows {
                    w.symbol.clone_from(&symbol);
                }
                out.extend(windows);
            }
        }
        out
    }

    /// Region boundaries of `start..end` (see [`Self::code_units`]).
    fn regions(&self, start: usize, end: usize) -> Vec<(usize, usize)> {
        let ls = lines(self.text, start, end);
        let base = ls
            .iter()
            .map(|&(s, e)| &self.text[s..e])
            .filter(|l| !is_blank(l))
            .map(indent)
            .min()
            .unwrap_or(0);
        let mut regions: Vec<(usize, usize)> = Vec::new();
        let mut cur: Option<(usize, usize)> = None;
        let mut prev_blank = true;
        for &(s, e) in &ls {
            let line = &self.text[s..e];
            if is_blank(line) {
                prev_blank = true;
                continue;
            }
            let starts = prev_blank && indent(line) == base && !closes(line.trim_start());
            match (&mut cur, starts) {
                (Some(r), false) => r.1 = e,
                (Some(r), true) => {
                    regions.push(*r);
                    cur = Some((s, e));
                }
                (None, _) => cur = Some((s, e)),
            }
            prev_blank = false;
        }
        regions.extend(cur);
        regions
    }

    /// Line windows of about `target_tokens` with `overlap_lines` repeated.
    fn line_windows(&self, start: usize, end: usize, ctx: Option<&str>) -> Vec<Unit> {
        let ls = lines(self.text, start, end);
        let mut out = Vec::new();
        let mut i = 0;
        while i < ls.len() {
            let from = ls[i].0;
            let mut j = i;
            while j + 1 < ls.len() && self.tokens(from, ls[j + 1].1) <= self.cfg.target_tokens {
                j += 1;
            }
            let to = ls[j].1;
            if self.tokens(from, to) > self.cfg.max_tokens {
                out.extend(self.sized(from, to, 0, ctx, Split::Words));
            } else {
                out.extend(self.unit(from, to, 0, ctx));
            }
            if j + 1 >= ls.len() {
                break;
            }
            let next = (j + 1).saturating_sub(self.cfg.overlap_lines);
            i = if next > i { next } else { j + 1 };
        }
        out
    }

    /// Data files: lines packed later; long lines split by words.
    fn line_units(&self, start: usize, end: usize, group: usize, ctx: Option<&str>) -> Vec<Unit> {
        lines(self.text, start, end)
            .into_iter()
            .flat_map(|(s, e)| self.sized(s, e, group, ctx, Split::Words))
            .collect()
    }

    // --- packing -------------------------------------------------------------------------

    /// Packs consecutive units of one group into chunks of at most `target_tokens` (a unit
    /// larger than that stands alone; units are already ≤ `max_tokens`).
    fn pack(&self, units: &[Unit], kind: ChunkKind) -> Vec<Chunk> {
        let mut out: Vec<Chunk> = Vec::new();
        let mut cur: Option<Unit> = None;
        let push = |out: &mut Vec<Chunk>, u: Unit| {
            out.push(Chunk {
                ordinal: u32::try_from(out.len()).unwrap_or(u32::MAX),
                kind,
                start: u.start,
                end: u.end,
                tokens: self.tokens(u.start, u.end),
                symbol: u.symbol,
                context: u.context,
            });
        };
        for u in units {
            if let Some(c) = &mut cur {
                // Overlapping windows (code) never merge back into the previous chunk.
                let fits = c.group == u.group
                    && u.start >= c.end
                    && self.tokens(c.start, u.end) <= self.cfg.target_tokens
                    // Two different definitions stay apart unless the first is tiny.
                    && !(c.symbol.is_some()
                        && u.symbol.is_some()
                        && c.symbol != u.symbol
                        && self.tokens(c.start, c.end) > self.cfg.target_tokens / 4);
                if fits {
                    c.end = u.end;
                    if c.symbol.is_none() {
                        c.symbol.clone_from(&u.symbol);
                    }
                    continue;
                }
                if let Some(done) = cur.take() {
                    push(&mut out, done);
                }
            }
            cur = Some(u.clone());
        }
        if let Some(done) = cur {
            push(&mut out, done);
        }
        out
    }
}

#[derive(Clone, Copy)]
enum Split {
    Sentences,
    Lines,
    Words,
}

/// Lines that close a block rather than start one.
fn closes(line: &str) -> bool {
    [
        "}", ")", "]", "end", "</", "fi", "done", "esac", "elif", "else", "except", "finally",
        "catch",
    ]
    .iter()
    .any(|k| line.starts_with(k))
}

/// The defined name in the first lines of a code region (`fn parse_args` → `parse_args`).
fn symbol_of(region: &str, lang: Language) -> Option<String> {
    const MODIFIERS: &[&str] = &[
        "pub",
        "pub(crate)",
        "pub(super)",
        "async",
        "export",
        "default",
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "abstract",
        "final",
        "override",
        "unsafe",
        "const",
        "extern",
        "virtual",
        "sealed",
        "partial",
        "open",
        "data",
        "inline",
        "local",
        "readonly",
    ];
    const KEYWORDS: &[&str] = &[
        "fn",
        "def",
        "class",
        "struct",
        "enum",
        "trait",
        "impl",
        "interface",
        "function",
        "func",
        "type",
        "mod",
        "module",
        "namespace",
        "record",
        "object",
        "union",
        "macro_rules!",
        "CREATE",
    ];
    for line in region.lines().take(6) {
        let t = line.trim();
        // `const name = (…) =>` / `let name = function` (JS/TS function values).
        if matches!(lang, Language::JavaScript | Language::TypeScript) {
            let decl = t
                .trim_start_matches("export ")
                .trim_start_matches("default ");
            if let Some(rest) = ["const ", "let ", "var "]
                .iter()
                .find_map(|k| decl.strip_prefix(k))
                && (t.contains("=>") || t.contains("function"))
            {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                    .collect();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
        if t.is_empty()
            || t.starts_with("//")
            || t.starts_with('#') && lang != Language::Python && !t.starts_with("#[")
            || t.starts_with("/*")
            || t.starts_with('*')
            || t.starts_with("#[")
            || t.starts_with('@')
            || t.starts_with("--")
        {
            continue;
        }
        // Go methods: `func (s *Server) Start()` names `Start`.
        let go_method;
        let t = match t.strip_prefix("func (").and_then(|r| r.split_once(')')) {
            Some((_, rest)) => {
                go_method = format!("func {}", rest.trim_start());
                go_method.as_str()
            }
            None => t,
        };
        let mut words = t
            .split(|c: char| {
                c.is_whitespace() || c == '(' || c == '<' || c == ':' || c == '{' || c == '='
            })
            .filter(|w| !w.is_empty())
            .skip_while(|w| MODIFIERS.contains(w));
        let Some(first) = words.next() else { continue };
        if KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(first)) {
            let mut name = words.next()?;
            if first.eq_ignore_ascii_case("CREATE") {
                // CREATE TABLE/INDEX/VIEW name
                name = words.find(|w| {
                    ![
                        "TABLE", "INDEX", "VIEW", "UNIQUE", "VIRTUAL", "IF", "NOT", "EXISTS",
                    ]
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(w))
                })?;
            }
            let name: String = name
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.' || *c == '$')
                .collect();
            return (!name.is_empty()).then_some(name);
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(kind: DocKind, text: &str) -> Extracted {
        Extracted {
            kind,
            text: text.to_owned(),
            encoding: "utf-8",
            lossy: false,
        }
    }

    fn small() -> ChunkConfig {
        ChunkConfig {
            target_tokens: 24,
            max_tokens: 36,
            overlap_lines: 1,
        }
    }

    fn texts(d: &Extracted, cfg: &ChunkConfig) -> Vec<String> {
        chunk(d, cfg, &EstimateTokens)
            .iter()
            .map(|c| c.text(&d.text).to_owned())
            .collect()
    }

    #[test]
    fn estimate_counts_words_and_symbols() {
        let e = EstimateTokens;
        assert_eq!(e.count(""), 0);
        assert_eq!(e.count("hello world"), 2); // 5 letters -> 1 token each
        assert_eq!(e.count("a, b."), 4);
        assert_eq!(e.count("reunión"), 2);
        assert_eq!(e.count("documentation"), 3);
    }

    #[test]
    fn prose_packs_paragraphs_and_splits_long_ones() {
        let text = "First short paragraph.\n\nSecond one is here.\n\n\
                    This third paragraph is much longer than the others. It has several \
                    sentences in it. Each sentence adds words until the budget is passed. \
                    And one more sentence to be sure.";
        let d = doc(DocKind::Prose, text);
        let cfg = small();
        let chunks = chunk(&d, &cfg, &EstimateTokens);
        assert!(chunks.len() >= 3, "{:?}", texts(&d, &cfg));
        assert!(
            chunks[0]
                .text(text)
                .starts_with("First short paragraph.\n\nSecond one is here."),
            "short paragraphs pack together"
        );
        for c in &chunks {
            assert!(c.tokens <= cfg.max_tokens, "{c:?}");
            assert!(text.is_char_boundary(c.start) && text.is_char_boundary(c.end));
        }
        // Ordered, non-overlapping, and every word is in some chunk.
        assert!(chunks.windows(2).all(|w| w[0].end <= w[1].start));
        let joined = texts(&d, &cfg).join(" ");
        for w in text.split_whitespace() {
            assert!(joined.contains(w), "{w} lost");
        }
        assert_eq!(
            chunks.iter().map(|c| c.ordinal).collect::<Vec<_>>(),
            (0..chunks.len() as u32).collect::<Vec<_>>()
        );
    }

    #[test]
    fn huge_tokens_are_cut() {
        let blob = "A".repeat(1000);
        let d = doc(DocKind::Prose, &blob);
        let chunks = chunk(&d, &small(), &EstimateTokens);
        assert!(chunks.len() > 5);
        assert!(chunks.iter().all(|c| c.tokens <= small().max_tokens));
        assert_eq!(chunks.last().map(|c| c.end), Some(1000));
    }

    #[test]
    fn markdown_keeps_heading_paths_and_fences() {
        let text = "# Lumen\n\nIntro text.\n\n## Install\n\n### Windows\n\nRun the installer.\n\n```ps1\n\nnpm run build\ncargo build\n```\n\n## Usage\n\nPress Alt+Space.\n";
        let d = doc(DocKind::Markdown, text);
        let cfg = ChunkConfig::default();
        let chunks = chunk(&d, &cfg, &EstimateTokens);
        let by_ctx: Vec<(Option<&str>, &str)> = chunks
            .iter()
            .map(|c| (c.context.as_deref(), c.text(text)))
            .collect();
        assert_eq!(by_ctx[0], (Some("Lumen"), "# Lumen\n\nIntro text."));
        // "## Install" has no body of its own: it travels with "### Windows".
        assert!(
            by_ctx[1]
                .1
                .starts_with("## Install\n\n### Windows\n\nRun the installer.")
        );
        assert!(
            by_ctx[1].1.contains("cargo build\n```"),
            "fence intact: {by_ctx:?}"
        );
        assert_eq!(
            by_ctx[2],
            (Some("Lumen › Usage"), "## Usage\n\nPress Alt+Space.")
        );
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn code_splits_at_definitions_with_symbols() {
        let text = r#"use std::io;

/// Parses arguments.
pub fn parse_args(args: &[String]) -> Options {
    let mut o = Options::default();
    for a in args {
        o.push(a);
    }
    o
}

pub struct Options {
    items: Vec<String>,
}

impl Options {
    fn push(&mut self, a: &str) {
        self.items.push(a.to_owned());
    }
}
"#;
        let d = doc(DocKind::Code(Language::Rust), text);
        let cfg = ChunkConfig {
            target_tokens: 40,
            max_tokens: 60,
            overlap_lines: 1,
        };
        let chunks = chunk(&d, &cfg, &EstimateTokens);
        let symbols: Vec<Option<&str>> = chunks.iter().map(|c| c.symbol.as_deref()).collect();
        assert!(symbols.contains(&Some("parse_args")), "{symbols:?}");
        assert!(symbols.contains(&Some("Options")), "{symbols:?}");
        let parse = chunks
            .iter()
            .find(|c| c.symbol.as_deref() == Some("parse_args"))
            .unwrap();
        assert!(
            parse.text(text).starts_with("/// Parses arguments."),
            "doc comment stays with fn"
        );
        assert!(parse.text(text).trim_end().ends_with('}'));
        assert!(chunks.iter().all(|c| c.kind == ChunkKind::Code));
    }

    #[test]
    fn python_classes_split_into_methods_when_large() {
        let mut text = String::from("class Store:\n    \"\"\"Item store.\"\"\"\n\n");
        for i in 0..6 {
            text.push_str(&format!(
                "    def method_{i}(self, value):\n        result = self.compute(value) + {i}\n        return result * 2\n\n"
            ));
        }
        let d = doc(DocKind::Code(Language::Python), &text);
        let cfg = ChunkConfig {
            target_tokens: 30,
            max_tokens: 45,
            overlap_lines: 1,
        };
        let chunks = chunk(&d, &cfg, &EstimateTokens);
        assert!(chunks.len() >= 3, "{chunks:?}");
        assert_eq!(chunks[0].symbol.as_deref(), Some("Store"));
        assert!(
            chunks
                .iter()
                .any(|c| c.symbol.as_deref() == Some("method_3"))
        );
        assert!(
            chunks
                .iter()
                .skip(1)
                .all(|c| c.context.as_deref() == Some("Store"))
        );
        assert!(chunks.iter().all(|c| c.tokens <= cfg.max_tokens));
    }

    #[test]
    fn long_functions_become_overlapping_line_windows() {
        let mut text = String::from("fn long() {\n");
        for i in 0..80 {
            text.push_str(&format!("    let v{i} = compute({i});\n"));
        }
        text.push_str("}\n");
        let d = doc(DocKind::Code(Language::Rust), &text);
        let cfg = ChunkConfig::default();
        let chunks = chunk(&d, &cfg, &EstimateTokens);
        assert!(chunks.len() > 3);
        assert!(chunks.iter().all(|c| c.symbol.as_deref() == Some("long")));
        assert!(chunks.iter().all(|c| c.tokens <= cfg.max_tokens));
        // Overlap: consecutive windows share lines.
        assert!(chunks.windows(2).any(|w| w[1].start < w[0].end));
        assert!(chunks.last().unwrap().text(&text).ends_with('}'));
    }

    #[test]
    fn data_files_pack_lines() {
        let text: String = (0..50).map(|i| format!("{i},name{i},value\n")).collect();
        let d = doc(DocKind::Data, &text);
        let chunks = chunk(&d, &ChunkConfig::default(), &EstimateTokens);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.tokens <= 128));
        assert!(chunks.windows(2).all(|w| w[0].end <= w[1].start));
    }

    #[test]
    fn empty_documents_have_no_chunks() {
        for kind in [
            DocKind::Prose,
            DocKind::Markdown,
            DocKind::Data,
            DocKind::Code(Language::Go),
        ] {
            assert!(
                chunk(
                    &doc(kind, "  \n\n \n"),
                    &ChunkConfig::default(),
                    &EstimateTokens
                )
                .is_empty()
            );
        }
    }

    #[test]
    fn symbols_across_languages() {
        assert_eq!(
            symbol_of("def run(x):\n  pass", Language::Python).as_deref(),
            Some("run")
        );
        assert_eq!(
            symbol_of("export async function load(url) {", Language::TypeScript).as_deref(),
            Some("load")
        );
        assert_eq!(
            symbol_of("const debounce = (fn, ms) => {", Language::JavaScript).as_deref(),
            Some("debounce")
        );
        assert_eq!(
            symbol_of("public sealed class Store : IStore", Language::CSharp).as_deref(),
            Some("Store")
        );
        assert_eq!(
            symbol_of("func (s *Server) Start() error {", Language::Go).as_deref(),
            Some("Start")
        );
        assert_eq!(
            symbol_of("func Start() error {", Language::Go).as_deref(),
            Some("Start")
        );
        assert_eq!(
            symbol_of("#[derive(Debug)]\npub struct Item {", Language::Rust).as_deref(),
            Some("Item")
        );
        assert_eq!(
            symbol_of("CREATE TABLE items (", Language::Sql).as_deref(),
            Some("items")
        );
        assert_eq!(symbol_of("let x = 1;", Language::Rust), None);
    }
}
