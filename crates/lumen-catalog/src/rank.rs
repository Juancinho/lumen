//! Name/path matching and ranking for the instant provider (T102,
//! docs/SEARCH_AND_INDEXING.md §3 "Filename/path").
//!
//! Signals, strongest first: exact name, exact stem (name without extension), name prefix,
//! every query token prefixing a name token (in order beats any order), initials
//! ("vsc" → "Visual Studio Code"), name + parent-folder tokens, and finally a bounded
//! edit-distance match for typos. Small bounded priors break ties: applications, recent
//! files, shallow paths; hidden/system entries sink.

use lumen_core::MatchKind;
use lumen_storage::{CatalogItem, Source, UsageSignal};

use crate::text::{fold, tokens};

/// Shortest token used as an FTS prefix term (see [`ParsedQuery::fts_matcher`]).
pub const MIN_FTS_TOKEN_CHARS: usize = 3;

/// A parsed root query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Whole query, folded.
    pub key: String,
    /// Folded tokens (same split as names).
    pub tokens: Vec<String>,
}

impl ParsedQuery {
    /// `None` when the query has no searchable characters.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let key = fold(text);
        let tokens = tokens(text);
        (!key.is_empty() && !tokens.is_empty()).then_some(Self { key, tokens })
    }

    /// FTS5 expression: tokens of at least [`MIN_FTS_TOKEN_CHARS`] characters as quoted
    /// prefix terms (tokens are alphanumeric, so quoting neutralizes FTS syntax). Shorter
    /// tokens would match most of the index and make bm25 rank all of it; the scorer still
    /// checks them. `None` when no token is long enough.
    #[must_use]
    pub fn fts_matcher(&self) -> Option<String> {
        let terms: Vec<String> = self
            .tokens
            .iter()
            .filter(|t| t.chars().count() >= MIN_FTS_TOKEN_CHARS)
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect();
        match terms.len() {
            0 => None,
            // One term: names only. Folder tokens matter for "folder + name" queries; for a
            // single word they would pull in every file below a matching folder.
            1 => Some(format!("name_parts : {}", terms[0])),
            _ => Some(terms.join(" ")),
        }
    }

    #[must_use]
    pub fn chars(&self) -> usize {
        self.key.chars().count()
    }
}

/// Match evidence for one item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scored {
    /// Match quality in `[0, 1]` (the provider's `Confidence`).
    pub base: f32,
    /// `base` plus bounded priors; sort key.
    pub rank: f32,
    pub kind: MatchKind,
}

fn stem(name_key: &str) -> &str {
    match name_key.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && ext.len() <= 5 && ext.chars().all(char::is_alphanumeric) =>
        {
            stem
        }
        _ => name_key,
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio(part: usize, whole: usize) -> f32 {
    part as f32 / whole.max(part).max(1) as f32
}

/// Assigns each query token to a distinct name token it prefixes (greedy, longest query
/// tokens first). Returns whether all matched and whether they matched in order from the
/// first name token.
fn token_match(query: &[String], name: &[&str]) -> Option<bool> {
    let mut used = vec![false; name.len()];
    let mut order = Vec::with_capacity(query.len());
    let mut by_len: Vec<&String> = query.iter().collect();
    by_len.sort_by_key(|t| std::cmp::Reverse(t.len()));
    for q in by_len {
        let i = (0..name.len()).find(|&i| !used[i] && name[i].starts_with(q.as_str()))?;
        used[i] = true;
        order.push((query.iter().position(|x| x == q).unwrap_or(0), i));
    }
    order.sort_unstable();
    let positions: Vec<usize> = order.into_iter().map(|(_, i)| i).collect();
    let in_order = positions.first() == Some(&0) && positions.windows(2).all(|w| w[0] < w[1]);
    Some(in_order)
}

/// Optimal string alignment distance (Damerau–Levenshtein with adjacent transpositions),
/// stopping early once it exceeds `max`.
#[must_use]
pub fn edit_distance(a: &str, b: &str, max: usize) -> Option<usize> {
    if a.chars().count().abs_diff(b.chars().count()) > max {
        return None;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let w = b.len() + 1;
    let mut d = vec![0usize; (a.len() + 1) * w];
    for (j, cell) in d.iter_mut().enumerate().take(w) {
        *cell = j;
    }
    for i in 1..=a.len() {
        d[i * w] = i;
        let mut row_min = usize::MAX;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[(i - 1) * w + j] + 1)
                .min(d[i * w + j - 1] + 1)
                .min(d[(i - 1) * w + j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[(i - 2) * w + j - 2] + 1);
            }
            d[i * w + j] = v;
            row_min = row_min.min(v);
        }
        if row_min > max {
            return None;
        }
    }
    let dist = d[a.len() * w + b.len()];
    (dist <= max).then_some(dist)
}

/// Typos allowed for a query of `chars` characters.
#[must_use]
pub fn typo_budget(chars: usize) -> usize {
    match chars {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

fn base_match(q: &ParsedQuery, item: &CatalogItem) -> Option<(f32, MatchKind)> {
    let name = item.name_key.as_str();
    let stem = stem(name);
    let qn = q.chars();
    if name == q.key {
        return Some((1.0, MatchKind::Exact));
    }
    if stem == q.key {
        return Some((0.97, MatchKind::Exact));
    }
    if name.starts_with(&q.key) {
        return Some((
            0.80 + 0.12 * ratio(qn, stem.chars().count()),
            MatchKind::Prefix,
        ));
    }
    let name_tokens: Vec<&str> = item
        .name_parts
        .split(' ')
        .filter(|t| !t.is_empty())
        .collect();
    let token_chars: usize = q.tokens.iter().map(|t| t.chars().count()).sum();
    if let Some(in_order) = token_match(&q.tokens, &name_tokens) {
        let cover = ratio(token_chars, stem.chars().count());
        let base = if in_order { 0.78 } else { 0.70 };
        return Some((base + 0.08 * cover, MatchKind::Prefix));
    }
    // Tokens split between the name and its folders ("lumen notas" → lumen\notas.md):
    // at least one token must hit the name.
    let path_tokens: Vec<&str> = item
        .path_parts
        .split(' ')
        .filter(|t| !t.is_empty())
        .collect();
    if q.tokens.len() > 1 {
        let in_name = q
            .tokens
            .iter()
            .filter(|t| name_tokens.iter().any(|n| n.starts_with(t.as_str())))
            .count();
        let all = q.tokens.iter().all(|t| {
            name_tokens
                .iter()
                .chain(&path_tokens)
                .any(|n| n.starts_with(t.as_str()))
        });
        if all && in_name > 0 {
            #[allow(clippy::cast_precision_loss)]
            let share = in_name as f32 / q.tokens.len() as f32;
            return Some((0.50 + 0.1 * share, MatchKind::Prefix));
        }
    }
    // Typos: whole stem, then each name token, against the whole query.
    let max = typo_budget(qn);
    if max > 0 {
        let candidates = std::iter::once(stem)
            .chain(name_tokens.iter().copied())
            .filter(|c| c.len() + max >= q.key.len());
        let best = candidates
            .filter_map(|c| {
                edit_distance(&q.key, c, max).or_else(|| {
                    // The same-length prefix too ("spotfy" vs "spotify tools").
                    let end = c.char_indices().nth(qn + 1).map_or(c.len(), |(i, _)| i);
                    (end < c.len())
                        .then(|| edit_distance(&q.key, &c[..end], max))
                        .flatten()
                })
            })
            .min();
        if let Some(d) = best {
            #[allow(clippy::cast_precision_loss)]
            return Some((0.45 - 0.08 * d as f32, MatchKind::Fuzzy));
        }
    }
    None
}

const DAY_MS: i64 = 86_400_000;

/// Scores `item` for `q`; `None` when it does not match. `now_ms` is Unix epoch milliseconds.
#[must_use]
pub fn score(q: &ParsedQuery, item: &CatalogItem, now_ms: i64) -> Option<Scored> {
    let (base, kind) = base_match(q, item)?;
    Some(with_priors(base, kind, item, now_ms))
}

/// Base of an item the user picked for this query before but whose name does not match it
/// (e.g. Calculator chosen for "s"); usage priors decide whether it surfaces.
pub const LEARNED_BASE: f32 = 0.6;

/// [`score`] for a previously chosen item without a name match.
#[must_use]
pub fn score_learned(item: &CatalogItem, now_ms: i64) -> Scored {
    with_priors(LEARNED_BASE, MatchKind::Suggestion, item, now_ms)
}

fn with_priors(base: f32, kind: MatchKind, item: &CatalogItem, now_ms: i64) -> Scored {
    let mut prior = 0.0_f32;
    if item.source == Source::Apps {
        prior += 0.06;
    }
    if let Some(m) = item.modified_at {
        let age = now_ms.saturating_sub(m);
        if age < 3 * DAY_MS {
            prior += 0.04;
        } else if age < 30 * DAY_MS {
            prior += 0.02;
        }
    }
    if item.attributes & 0b11 != 0 {
        prior -= 0.15; // hidden or system
    }
    let depth = item.path.matches(['\\', '/']).count();
    #[allow(clippy::cast_precision_loss)]
    {
        prior -= (depth.saturating_sub(4) as f32 * 0.004).min(0.04);
    }
    Scored {
        base: base.clamp(0.0, 1.0),
        rank: base + prior,
        kind,
    }
}

/// Usage priors (T106): pins, frecency and — strongest — having picked this item for this
/// very query before ("learned results"), which may lift a token match above another item's
/// exact name after a handful of picks. All bounded.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn usage_prior(usage: &UsageSignal) -> f32 {
    let mut prior = 0.0_f32;
    if usage.pinned {
        prior += 0.05;
    }
    if usage.frecency > 0.0 {
        prior += (0.025 * (1.0 + usage.frecency).ln() as f32).min(0.08);
    }
    if usage.query_uses > 0 {
        prior += (0.12 + 0.08 * (usage.query_uses as f32).ln()).min(0.35);
    }
    prior
}

#[cfg(test)]
mod tests {
    use lumen_storage::ItemKind;

    use super::*;
    use crate::text::name_parts;

    fn item(name: &str, path: &str) -> CatalogItem {
        CatalogItem {
            id: 1,
            kind: ItemKind::File,
            source: Source::Files,
            path: path.into(),
            raw_path: None,
            name: name.into(),
            name_key: fold(name),
            name_parts: name_parts(name),
            path_parts: crate::text::path_parts(path, 3),
            extension: None,
            launch_target: None,
            attributes: 0,
            modified_at: None,
        }
    }

    fn s(q: &str, name: &str) -> Option<Scored> {
        score(
            &ParsedQuery::parse(q).unwrap(),
            &item(name, &format!("/r/{name}")),
            0,
        )
    }

    #[test]
    fn match_kinds_in_strength_order() {
        let exact = s("notes.txt", "notes.txt").unwrap();
        let stem = s("notes", "notes.txt").unwrap();
        let prefix = s("not", "notes.txt").unwrap();
        let tokens = s("studio code", "Visual Studio Code").unwrap();
        let in_order = s("visual code", "Visual Studio Code").unwrap();
        let initials = s("vsc", "Visual Studio Code").unwrap();
        let typo = s("spotfy", "Spotify").unwrap();
        assert_eq!(exact.kind, MatchKind::Exact);
        assert!(exact.rank > stem.rank && stem.rank > prefix.rank);
        assert!(prefix.rank > in_order.rank, "name prefix beats token match");
        assert!(
            in_order.rank > tokens.rank,
            "in-order tokens beat out-of-order"
        );
        assert!(tokens.rank > typo.rank && initials.rank > typo.rank);
        assert_eq!(typo.kind, MatchKind::Fuzzy);
        assert!(s("xyz", "Visual Studio Code").is_none());
        assert!(s("spo", "Visual Studio Code").is_none());
    }

    #[test]
    fn code_names_and_numbers_match_by_token() {
        assert!(s("compo", "MyComponentName.tsx").is_some());
        assert!(s("budget 2025", "q3budget2025.xlsx").is_some());
        assert!(s("reunion", "Presupuesto Reunión.xlsx").is_some());
        assert!(s("http serv", "HTTPServer.rs").is_some());
    }

    #[test]
    fn folder_tokens_help_but_need_a_name_hit() {
        let q = ParsedQuery::parse("lumen notas").unwrap();
        let hit = score(&q, &item("notas.md", "/home/joao/lumen/notas.md"), 0).unwrap();
        assert!(hit.base < 0.7);
        let q = ParsedQuery::parse("lumen joao").unwrap();
        assert!(score(&q, &item("notas.md", "/home/joao/lumen/notas.md"), 0).is_none());
    }

    #[test]
    fn typos_are_bounded_by_length() {
        assert!(
            s("spt", "Spotify").is_none(),
            "short queries get no typo budget"
        );
        assert!(
            s("sptofiy", "Spotify").is_none(),
            "two transpositions > 1 allowed at 7 chars"
        );
        assert!(s("calculatr", "Calculator").is_some());
        assert!(s("clcultr", "Calculator").is_none());
        assert_eq!(
            edit_distance("spotify", "sptoify", 1),
            Some(1),
            "transposition"
        );
        assert_eq!(edit_distance("abc", "abcdef", 1), None);
    }

    #[test]
    fn priors_order_ties_but_never_beat_a_better_match() {
        let q = ParsedQuery::parse("spotify").unwrap();
        let mut app = item("Spotify", "shell:AppsFolder\\Spotify");
        app.source = Source::Apps;
        let file = item("Spotify", "/r/Spotify");
        assert!(score(&q, &app, 0).unwrap().rank > score(&q, &file, 0).unwrap().rank);
        let mut hidden = item("spotify-cache", "/r/spotify-cache");
        hidden.attributes = 1;
        assert!(score(&q, &file, 0).unwrap().rank > score(&q, &hidden, 0).unwrap().rank);
        // A recent prefix match still loses to an old exact match.
        let mut recent = item("spotify tools", "/r/spotify tools");
        recent.modified_at = Some(10);
        assert!(score(&q, &file, 20).unwrap().rank > score(&q, &recent, 20).unwrap().rank);
    }

    #[test]
    fn learned_choices_lift_after_a_few_picks_and_stay_bounded() {
        let q = ParsedQuery::parse("code").unwrap();
        let folder = score(&q, &item("code", "/r/code"), 0).unwrap();
        let app = score(&q, &item("Visual Studio Code", "/r/vsc"), 0).unwrap();
        let picked = |n| {
            usage_prior(&UsageSignal {
                query_uses: n,
                ..UsageSignal::default()
            })
        };
        assert!(app.rank + picked(1) < folder.rank, "one pick is not enough");
        assert!(app.rank + picked(8) > folder.rank, "a habit wins");
        assert!(picked(1_000_000) <= 0.35);
        let heavy = UsageSignal {
            frecency: 1e9,
            pinned: true,
            ..UsageSignal::default()
        };
        assert!(usage_prior(&heavy) <= 0.13 + f32::EPSILON);
        assert!(usage_prior(&UsageSignal::default()).abs() < f32::EPSILON);
    }

    #[test]
    fn query_parsing_and_matcher() {
        assert!(ParsedQuery::parse("  ").is_none());
        assert!(ParsedQuery::parse("!!").is_none());
        let q = ParsedQuery::parse("Visual \"Studio").unwrap();
        assert_eq!(q.tokens, ["visual", "studio"]);
        assert_eq!(q.fts_matcher().as_deref(), Some("\"visual\"* \"studio\"*"));
        let q = ParsedQuery::parse("notas").unwrap();
        assert_eq!(q.fts_matcher().as_deref(), Some("name_parts : \"notas\"*"));
        let q = ParsedQuery::parse("factura de luz").unwrap();
        assert_eq!(
            q.fts_matcher().as_deref(),
            Some("\"factura\"* \"luz\"*"),
            "drops `de`"
        );
        assert_eq!(ParsedQuery::parse("cv").unwrap().fts_matcher(), None);
    }
}
