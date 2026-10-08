//! The provider contract (docs/COMMAND_MODEL.md §3, docs/ARCHITECTURE.md §20.C).
//!
//! A provider turns a root query into [`ResultItem`]s. It is synchronous like the
//! embedding backend (ADR-014): the coordinator (T107) owns threads, deadlines and
//! cancellation, and calls each provider according to its [`LatencyClass`].

use std::fmt;

use crate::execution::CancellationToken;
use crate::ids::{ProviderId, QueryId};
use crate::result::ResultItem;

/// Rough cost class a provider declares; the coordinator routes by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LatencyClass {
    /// In-memory or single indexed lookup (apps, exact file names): every keystroke.
    Instant,
    /// SQLite/FTS/settings: every keystroke, under a budget.
    Fast,
    /// Query embedding + ANN: after typing settles.
    Semantic,
    /// Heavy or opt-in (context, media, network).
    Deferred,
}

/// One provider request.
#[derive(Debug, Clone, Copy)]
pub struct ProviderQuery<'a> {
    pub id: QueryId,
    /// Raw root query text as typed (providers normalize it themselves).
    pub text: &'a str,
    /// The user may still be typing the last word (prefix semantics).
    pub typing: bool,
    /// Maximum results wanted from this provider.
    pub limit: usize,
}

/// Why a provider produced no answer. Never contains query text (privacy, logs).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// Stopped by the cancellation token or the coordinator's budget.
    Cancelled,
    /// The provider's backing store/runtime failed; message for logs only.
    Unavailable(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("provider cancelled"),
            Self::Unavailable(why) => write!(f, "provider unavailable: {why}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A result source. Implementations must be cheap to call repeatedly and must check
/// `cancel` between expensive steps.
pub trait Provider: Send + Sync {
    fn id(&self) -> &ProviderId;

    fn latency_class(&self) -> LatencyClass;

    /// Results for `query`, best first, at most `query.limit`. Every result must pass
    /// [`crate::validate_result`] against the registered actions.
    ///
    /// # Errors
    /// [`ProviderError::Cancelled`] when `cancel` fired; `Unavailable` on backend failure.
    fn search(
        &self,
        query: &ProviderQuery<'_>,
        cancel: &CancellationToken,
    ) -> Result<Vec<ResultItem>, ProviderError>;
}
