//! Typed execution context for actions, plus cooperative cancellation.
//!
//! Flow (executors arrive with T108/T109):
//!
//! ```text
//! UI: (query, result id, action id, invocation, confirmed)  ── ActionRequest
//!        │
//! core:  look up the ResultItem the coordinator produced for that id
//!        look up the ActionDescriptor
//!        ExecutionContext::authorize(..)  ── policy checks, no side effects
//!        │
//! executor: run with ExecutionContext (+ Payload from the ResultItem)
//! ```
//!
//! The UI never sends paths or payloads, only ids, so it can only trigger actions
//! that a provider actually offered on a result it actually produced.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::action::{ActionDescriptor, ActionSafety};
use crate::capability::CapabilitySet;
use crate::ids::{ActionId, QueryId, ResultId};
use crate::result::ResultItem;

/// How the user triggered the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Invocation {
    /// Enter / double-click: must resolve to the result's primary action.
    Primary,
    /// Chosen in the Action Panel (`Ctrl+K`).
    ActionPanel,
    /// Direct per-action shortcut (e.g. `Ctrl+Enter` → reveal).
    Shortcut,
}

/// What the UI asks for. Contains ids only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    /// Query whose result list the user acted on. Actions are NOT rejected for being
    /// on an older query: the user acts on what they saw, even if typing continued.
    pub query: QueryId,
    pub result: ResultId,
    pub action: ActionId,
    pub invocation: Invocation,
    /// Set only after the UI showed an explicit confirmation step to the user.
    pub confirmed: bool,
}

/// Why a request was refused. All checks are side-effect free.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthorizationError {
    /// The request names a different result than the one supplied.
    ResultMismatch,
    /// The descriptor supplied is not the requested action.
    DescriptorMismatch,
    /// The result does not offer this action.
    NotOffered,
    /// `Invocation::Primary` must run exactly the result's primary action.
    NotPrimary,
    /// The result lacks capabilities the action needs.
    MissingCapabilities(CapabilitySet),
    /// Destructive/privileged action without explicit confirmation.
    ConfirmationRequired(ActionSafety),
}

impl fmt::Display for AuthorizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResultMismatch => f.write_str("request refers to a different result"),
            Self::DescriptorMismatch => f.write_str("descriptor does not match requested action"),
            Self::NotOffered => f.write_str("result does not offer this action"),
            Self::NotPrimary => f.write_str("primary invocation must run the primary action"),
            Self::MissingCapabilities(m) => write!(f, "result lacks capabilities {m:?}"),
            Self::ConfirmationRequired(s) => write!(f, "{s:?} action requires confirmation"),
        }
    }
}

impl std::error::Error for AuthorizationError {}

/// Cooperative cancellation flag shared between a requester and workers.
///
/// Workers poll [`CancellationToken::is_cancelled`] at natural boundaries (between
/// batches, before expensive steps). Cloning shares the same flag.
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Idempotent.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CancellationToken")
            .field(&self.is_cancelled())
            .finish()
    }
}

/// An authorized action invocation. Only obtainable through [`ExecutionContext::authorize`],
/// so an executor holding one knows the policy checks passed.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    request: ActionRequest,
    safety: ActionSafety,
    cancellation: CancellationToken,
}

impl ExecutionContext {
    /// Applies the action policy to a request.
    ///
    /// # Errors
    /// The first failed check, in this order: identity (result, descriptor), offer,
    /// primary invocation, capabilities, confirmation.
    pub fn authorize(
        request: ActionRequest,
        item: &ResultItem,
        descriptor: &ActionDescriptor,
        cancellation: CancellationToken,
    ) -> Result<Self, AuthorizationError> {
        if request.result != item.id {
            return Err(AuthorizationError::ResultMismatch);
        }
        if descriptor.id != request.action {
            return Err(AuthorizationError::DescriptorMismatch);
        }
        if !item.offers(&request.action) {
            return Err(AuthorizationError::NotOffered);
        }
        if request.invocation == Invocation::Primary && request.action != item.primary_action {
            return Err(AuthorizationError::NotPrimary);
        }
        let missing = item.capabilities.missing(descriptor.requires);
        if !missing.is_empty() {
            return Err(AuthorizationError::MissingCapabilities(missing));
        }
        if descriptor.safety.requires_confirmation() && !request.confirmed {
            return Err(AuthorizationError::ConfirmationRequired(descriptor.safety));
        }
        Ok(Self {
            request,
            safety: descriptor.safety,
            cancellation,
        })
    }

    #[must_use]
    pub fn request(&self) -> &ActionRequest {
        &self.request
    }

    #[must_use]
    pub fn safety(&self) -> ActionSafety {
        self.safety
    }

    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionLookup;
    use crate::capability::Capability;
    use crate::contract::fixtures::*;

    fn request(action: ActionId, invocation: Invocation) -> ActionRequest {
        ActionRequest {
            query: QueryId::new(7).unwrap(),
            result: file_result().id,
            action,
            invocation,
            confirmed: false,
        }
    }

    fn authorize(
        req: ActionRequest,
        item: &ResultItem,
    ) -> Result<ExecutionContext, AuthorizationError> {
        let table = actions();
        let descriptor = table.action(&req.action).expect("fixture action").clone();
        ExecutionContext::authorize(req, item, &descriptor, CancellationToken::new())
    }

    #[test]
    fn primary_invocation_runs_primary_action() {
        let item = file_result();
        let ctx = authorize(request(OPEN, Invocation::Primary), &item).unwrap();
        assert_eq!(ctx.request().action, OPEN);
        assert_eq!(ctx.safety(), ActionSafety::SafeRead);
        assert!(!ctx.cancellation().is_cancelled());
    }

    #[test]
    fn primary_invocation_cannot_run_secondary() {
        let item = file_result();
        assert_eq!(
            authorize(request(REVEAL, Invocation::Primary), &item).unwrap_err(),
            AuthorizationError::NotPrimary
        );
        assert!(authorize(request(REVEAL, Invocation::Shortcut), &item).is_ok());
    }

    #[test]
    fn actions_not_offered_are_refused() {
        let mut item = file_result();
        item.capabilities = item.capabilities.with(Capability::TextValue);
        assert_eq!(
            authorize(request(COPY, Invocation::ActionPanel), &item).unwrap_err(),
            AuthorizationError::NotOffered
        );
    }

    #[test]
    fn destructive_requires_confirmation() {
        let item = file_result();
        let mut req = request(DELETE, Invocation::ActionPanel);
        assert_eq!(
            authorize(req.clone(), &item).unwrap_err(),
            AuthorizationError::ConfirmationRequired(ActionSafety::Destructive)
        );
        req.confirmed = true;
        assert!(authorize(req, &item).is_ok());
    }

    #[test]
    fn missing_capabilities_are_refused() {
        let mut item = file_result();
        item.secondary_actions.push(COPY);
        assert_eq!(
            authorize(request(COPY, Invocation::ActionPanel), &item).unwrap_err(),
            AuthorizationError::MissingCapabilities(CapabilitySet::of(&[Capability::TextValue]))
        );
    }

    #[test]
    fn identity_mismatches_are_refused() {
        let item = file_result();
        let mut req = request(OPEN, Invocation::Primary);
        req.result = ResultId::new("file:other").unwrap();
        assert_eq!(
            authorize(req, &item).unwrap_err(),
            AuthorizationError::ResultMismatch
        );

        let table = actions();
        let wrong_descriptor = table.action(&REVEAL).unwrap();
        assert_eq!(
            ExecutionContext::authorize(
                request(OPEN, Invocation::Primary),
                &item,
                wrong_descriptor,
                CancellationToken::new()
            )
            .unwrap_err(),
            AuthorizationError::DescriptorMismatch
        );
    }

    #[test]
    fn cancellation_is_shared_and_idempotent() {
        let token = CancellationToken::new();
        let worker = token.clone();
        assert!(!worker.is_cancelled());
        token.cancel();
        token.cancel();
        assert!(worker.is_cancelled());
        assert_eq!(format!("{worker:?}"), "CancellationToken(true)");

        let thread_token = CancellationToken::new();
        let seen = std::thread::scope(|s| {
            let t = thread_token.clone();
            let handle = s.spawn(move || {
                while !t.is_cancelled() {
                    std::hint::spin_loop();
                }
                true
            });
            thread_token.cancel();
            handle.join().unwrap()
        });
        assert!(seen);
    }
}
