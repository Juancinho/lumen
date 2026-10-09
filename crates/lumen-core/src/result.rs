//! The universal result model (docs/COMMAND_MODEL.md §2).
//!
//! Every provider — files, apps, calculator, settings, later clipboard/workflows —
//! returns [`ResultItem`]s. The coordinator fuses and ranks them; shells project
//! them into wire DTOs (ADR-013). [`Payload`] stays in Rust and never crosses the
//! UI boundary: the UI refers to results only by [`ResultId`].

use std::cmp::Ordering;
use std::fmt;
use std::path::PathBuf;

use crate::capability::CapabilitySet;
use crate::ids::{ActionId, ProviderId, ResultId};

/// Presentation-relevant category. Use for icons/row layout only; behaviour comes
/// from capabilities and actions. Variants are added by the tasks that need them
/// (PDF page T301, image T303, snippet T405, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResultKind {
    File,
    /// A matching code passage, retaining the file's entity identity.
    Code,
    /// A text passage on a physical PDF page, retaining the file's entity identity.
    PdfPage,
    Folder,
    Application,
    /// A built-in or system command (Windows setting, system action).
    Command,
}

/// How the shell should render the result's icon. Never carries image bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IconRef {
    /// Generic glyph for the result's [`ResultKind`].
    KindDefault,
    /// Generic icon for a file type, by lowercase extension without the dot (`pdf`).
    FileExtension(Box<str>),
    /// The OS-native icon of the target (app/exe/shortcut), resolved lazily by the
    /// shell from the result's id; falls back to [`IconRef::KindDefault`].
    Native,
}

/// Provider-normalized confidence in `[0.0, 1.0]`.
///
/// Raw provider scores (BM25, cosine, fuzzy distance) are not comparable across
/// providers; each provider maps its raw score into this range before global
/// fusion (docs/SEARCH_AND_INDEXING.md §20). Never shown in normal UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confidence(f32);

impl Confidence {
    pub const ZERO: Self = Self(0.0);
    pub const CERTAIN: Self = Self(1.0);

    /// Returns `None` for NaN or values outside `[0, 1]`.
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        // `+ 0.0` normalizes -0.0 to 0.0 so `Eq`/`Ord` agree.
        (0.0..=1.0).contains(&value).then_some(Self(value + 0.0))
    }

    /// Clamps into `[0, 1]`; NaN becomes `0`.
    #[must_use]
    pub fn saturating(value: f32) -> Self {
        if value.is_nan() {
            Self::ZERO
        } else {
            Self(value.clamp(0.0, 1.0) + 0.0)
        }
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Eq for Confidence {}

impl PartialOrd for Confidence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Confidence {
    fn cmp(&self, other: &Self) -> Ordering {
        // Never NaN and never -0.0 by construction, so total_cmp is numeric order.
        self.0.total_cmp(&other.0)
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.3}", self.0)
    }
}

/// Which kind of evidence produced the match. Lets the coordinator apply intent
/// rules (exact name beats semantic neighbour) without comparing raw scores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MatchKind {
    /// Query equals the title/name (case-insensitive).
    Exact,
    Prefix,
    Fuzzy,
    /// Lexical full-text (FTS5).
    FullText,
    /// Vector similarity.
    Semantic,
    /// Deterministic intent parse (calculator expression, setting keyword).
    Intent,
    /// Suggested without a query match (recent/pinned on empty query).
    Suggestion,
}

/// Provider-side relevance evidence for one result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub confidence: Confidence,
    pub match_kind: MatchKind,
}

impl Score {
    #[must_use]
    pub const fn new(confidence: Confidence, match_kind: MatchKind) -> Self {
        Self {
            confidence,
            match_kind,
        }
    }
}

/// What an executor needs to act on the target. Rust-only: never serialized to
/// the UI, which cannot therefore ask Lumen to act on arbitrary paths.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Payload {
    /// Local file or folder.
    Path(PathBuf),
    /// File plus the matching code passage. Boxed to keep the hot result compact.
    Code(Box<CodeTarget>),
    Pdf(Box<PdfTarget>),
    /// A textual value (calculation result, URL).
    Text(Box<str>),
    /// Opaque key the owning provider resolves itself (app user-model id, setting URI).
    ProviderKey(Box<str>),
}

/// Trusted provider context for code actions. No editor command or UI-supplied path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeTarget {
    pub path: PathBuf,
    pub symbol: Option<String>,
    pub language: String,
    /// Nearest Git repository (including worktrees), discovered during indexing.
    pub repository: Option<PathBuf>,
    /// Offsets into normalized extracted text, not raw file offsets.
    pub start_offset: Option<u64>,
    pub end_offset: Option<u64>,
    /// Bounded indexed passage for Quick Look, including matches beyond the file start.
    pub passage: String,
}

/// Trusted page context from the indexed PDF text layer. Opening still uses the file
/// handler; rendering and viewer-specific page navigation belong to T302.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfTarget {
    pub path: PathBuf,
    pub page_number: std::num::NonZeroU32,
    pub passage: String,
}

impl Payload {
    #[must_use]
    pub fn local_path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Path(path) => Some(path),
            Self::Code(code) => Some(&code.path),
            Self::Pdf(pdf) => Some(&pdf.path),
            _ => None,
        }
    }
}

/// One row of the universal result list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultItem {
    /// Entity identity; equal across providers for the same entity.
    pub id: ResultId,
    /// Provider that produced this instance (after merging: the highest-ranked one).
    pub provider: ProviderId,
    pub kind: ResultKind,
    /// Primary line, e.g. file or app name.
    pub title: String,
    /// Secondary line, e.g. snippet or semantic hit context.
    pub subtitle: Option<String>,
    /// Tertiary line, e.g. location/path (UI middle-truncates).
    pub detail: Option<String>,
    pub icon: IconRef,
    pub score: Score,
    pub capabilities: CapabilitySet,
    /// What Enter does. Must be listed nowhere else and must be safe (see `contract`).
    pub primary_action: ActionId,
    /// Additional result-specific actions in provider-suggested order. The Action
    /// Panel (T108) may add capability-derived actions on top.
    pub secondary_actions: Vec<ActionId>,
    pub payload: Payload,
}

impl ResultItem {
    /// Whether `action` is offered by this result (primary or secondary).
    #[must_use]
    pub fn offers(&self, action: &ActionId) -> bool {
        &self.primary_action == action || self.secondary_actions.contains(action)
    }

    /// All offered actions, primary first.
    pub fn actions(&self) -> impl Iterator<Item = &ActionId> {
        std::iter::once(&self.primary_action).chain(&self.secondary_actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_bounds() {
        assert_eq!(Confidence::new(0.5).map(Confidence::get), Some(0.5));
        assert!(Confidence::new(1.0001).is_none());
        assert!(Confidence::new(-0.1).is_none());
        assert!(Confidence::new(f32::NAN).is_none());
        assert_eq!(Confidence::saturating(7.0), Confidence::CERTAIN);
        assert_eq!(Confidence::saturating(-3.0).get(), 0.0);
        assert_eq!(Confidence::saturating(f32::NAN), Confidence::ZERO);
        let neg_zero = Confidence::new(-0.0).unwrap();
        assert_eq!(neg_zero.cmp(&Confidence::ZERO), Ordering::Equal);
        assert!(neg_zero.get().is_sign_positive());
    }

    #[test]
    fn confidence_sorts_totally() {
        let mut v: Vec<Confidence> = [0.2, 0.9, 0.0, 1.0, 0.5]
            .into_iter()
            .filter_map(Confidence::new)
            .collect();
        v.sort_unstable_by(|a, b| b.cmp(a));
        let got: Vec<f32> = v.into_iter().map(Confidence::get).collect();
        assert_eq!(got, [1.0, 0.9, 0.5, 0.2, 0.0]);
    }

    /// Guards against accidental bloat of the hot per-result struct (copied per
    /// keystroke batch). Raise deliberately, with a reason, if a field is added.
    #[test]
    fn result_item_stays_compact() {
        let size = size_of::<ResultItem>();
        assert!(size <= 256, "ResultItem grew to {size} bytes"); // 224 on x86_64 at T011
    }
}
