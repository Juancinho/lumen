//! Contextual actions of a result (T108) and request preparation (T109): which registered
//! actions a result offers, in Action Panel order, and the policy check before an executor
//! runs one. Executors themselves are OS/shell adapters.

use lumen_core::{
    ActionDescriptor, ActionLookup, ActionRequest, AuthorizationError, CancellationToken,
    ExecutionContext, ResultItem,
};

/// Offered actions whose capabilities the result has: the primary action first, then the
/// rest by panel group (docs/COMMAND_MODEL.md §6), keeping the provider's order in a group.
#[must_use]
pub fn available<'a>(
    item: &ResultItem,
    registry: &'a [ActionDescriptor],
) -> Vec<&'a ActionDescriptor> {
    let usable = |d: &&ActionDescriptor| item.capabilities.missing(d.requires).is_empty();
    let primary = registry.action(&item.primary_action).filter(usable);
    let mut rest: Vec<(usize, &ActionDescriptor)> = item
        .secondary_actions
        .iter()
        .filter(|id| **id != item.primary_action)
        .filter_map(|id| registry.action(id))
        .filter(usable)
        .enumerate()
        .collect();
    rest.sort_by_key(|(i, d)| (d.group, *i));
    primary
        .into_iter()
        .chain(rest.into_iter().map(|(_, d)| d))
        .collect()
}

/// Why an action request cannot run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// The result is not among the recent results Lumen produced.
    UnknownResult,
    /// No registered action has this id.
    UnknownAction,
    Refused(AuthorizationError),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownResult => f.write_str("result not found (stale or unknown)"),
            Self::UnknownAction => f.write_str("unknown action"),
            Self::Refused(e) => write!(f, "refused: {e}"),
        }
    }
}

impl std::error::Error for ActionError {}

/// Looks up the descriptor and authorizes `request` against `item`.
///
/// # Errors
/// Unknown action or a failed policy check.
pub fn prepare<'a>(
    request: ActionRequest,
    item: &ResultItem,
    registry: &'a [ActionDescriptor],
) -> Result<(ExecutionContext, &'a ActionDescriptor), ActionError> {
    let descriptor = registry
        .action(&request.action)
        .ok_or(ActionError::UnknownAction)?;
    let ctx = ExecutionContext::authorize(request, item, descriptor, CancellationToken::new())
        .map_err(ActionError::Refused)?;
    Ok((ctx, descriptor))
}

#[cfg(test)]
mod tests {
    use lumen_core::builtin::{COPY_PATH, DESCRIPTORS, LAUNCH, OPEN, REVEAL};
    use lumen_core::{ActionId, Capability, CapabilitySet, Invocation, ProviderId, QueryId};

    use super::*;
    use crate::coordinator::tests::item;

    static REGISTRY: [ActionDescriptor; 8] = DESCRIPTORS;

    fn file() -> ResultItem {
        let mut it = item("item:1", &ProviderId::new("test.p").unwrap(), 0.5);
        it.capabilities = CapabilitySet::of(&[Capability::LocalPath]);
        it.primary_action = OPEN;
        it.secondary_actions = vec![COPY_PATH, REVEAL];
        it
    }

    fn request(action: ActionId, invocation: Invocation) -> ActionRequest {
        ActionRequest {
            query: QueryId::new(1).unwrap(),
            result: file().id,
            action,
            invocation,
            confirmed: false,
        }
    }

    #[test]
    fn primary_first_then_panel_groups() {
        let ids: Vec<&str> = available(&file(), &REGISTRY)
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(ids, ["lumen.open", "lumen.copy-path", "lumen.reveal"]);
    }

    #[test]
    fn actions_missing_capabilities_are_hidden() {
        let mut app = file();
        app.capabilities = CapabilitySet::of(&[Capability::Launchable]);
        app.primary_action = LAUNCH;
        let ids: Vec<&str> = available(&app, &REGISTRY)
            .iter()
            .map(|d| d.id.as_str())
            .collect();
        assert_eq!(ids, ["lumen.launch"]);
    }

    #[test]
    fn prepare_applies_the_policy() {
        assert!(prepare(request(OPEN, Invocation::Primary), &file(), &DESCRIPTORS).is_ok());
        assert!(prepare(request(REVEAL, Invocation::Shortcut), &file(), &DESCRIPTORS).is_ok());
        assert_eq!(
            prepare(request(REVEAL, Invocation::Primary), &file(), &DESCRIPTORS).err(),
            Some(ActionError::Refused(AuthorizationError::NotPrimary))
        );
        assert_eq!(
            prepare(
                request(LAUNCH, Invocation::ActionPanel),
                &file(),
                &DESCRIPTORS
            )
            .err(),
            Some(ActionError::Refused(AuthorizationError::NotOffered))
        );
        let unknown = ActionId::new("test.nope").unwrap();
        assert_eq!(
            prepare(
                request(unknown, Invocation::ActionPanel),
                &file(),
                &DESCRIPTORS
            )
            .err(),
            Some(ActionError::UnknownAction)
        );
    }

    #[test]
    fn pdf_page_action_requires_an_offered_trusted_page_capability() {
        use lumen_core::builtin::OPEN_PDF_PAGE;
        let mut pdf = file();
        assert!(
            prepare(
                request(OPEN_PDF_PAGE, Invocation::ActionPanel),
                &pdf,
                &DESCRIPTORS
            )
            .is_err()
        );
        pdf.secondary_actions.push(OPEN_PDF_PAGE);
        assert!(
            prepare(
                request(OPEN_PDF_PAGE, Invocation::ActionPanel),
                &pdf,
                &DESCRIPTORS
            )
            .is_err()
        );
        pdf.capabilities = pdf.capabilities.with(Capability::PdfPage);
        assert!(
            prepare(
                request(OPEN_PDF_PAGE, Invocation::ActionPanel),
                &pdf,
                &DESCRIPTORS
            )
            .is_ok()
        );
        assert!(
            prepare(
                request(OPEN_PDF_PAGE, Invocation::Primary),
                &pdf,
                &DESCRIPTORS
            )
            .is_err()
        );
    }

    #[test]
    fn code_actions_require_offered_capabilities_and_panel_invocation() {
        use lumen_core::builtin::{COPY_SYMBOL, REVEAL_REPOSITORY};
        let mut code = file();
        code.secondary_actions
            .extend([COPY_SYMBOL, REVEAL_REPOSITORY]);
        assert!(
            prepare(
                request(COPY_SYMBOL, Invocation::ActionPanel),
                &code,
                &DESCRIPTORS
            )
            .is_err()
        );
        code.capabilities = code.capabilities.with(Capability::CodeSymbol);
        assert!(
            prepare(
                request(COPY_SYMBOL, Invocation::ActionPanel),
                &code,
                &DESCRIPTORS
            )
            .is_ok()
        );
        assert!(
            prepare(
                request(REVEAL_REPOSITORY, Invocation::ActionPanel),
                &code,
                &DESCRIPTORS
            )
            .is_err()
        );
        code.capabilities = code.capabilities.with(Capability::Repository);
        assert!(
            prepare(
                request(REVEAL_REPOSITORY, Invocation::ActionPanel),
                &code,
                &DESCRIPTORS
            )
            .is_ok()
        );
        assert_eq!(
            prepare(
                request(COPY_SYMBOL, Invocation::Primary),
                &code,
                &DESCRIPTORS
            )
            .err(),
            Some(ActionError::Refused(AuthorizationError::NotPrimary))
        );
    }
}
