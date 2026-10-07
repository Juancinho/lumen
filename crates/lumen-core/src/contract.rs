//! Provider contract checks (docs/TESTING.md §3).
//!
//! [`validate_result`] is what every provider test runs over its output, and what
//! the coordinator runs in debug builds before results reach the UI.

use std::fmt;

use crate::action::ActionLookup;
use crate::capability::CapabilitySet;
use crate::ids::ActionId;
use crate::result::ResultItem;

/// A result that breaks the universal result/action contract.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContractViolation {
    EmptyTitle,
    /// An offered action is not known to the action registry.
    UnknownAction(ActionId),
    /// The primary action is destructive or privileged.
    UnsafePrimary(ActionId),
    /// An offered action needs capabilities the result does not have.
    MissingCapabilities {
        action: ActionId,
        missing: CapabilitySet,
    },
    /// An action is offered twice (including primary repeated as secondary).
    DuplicateAction(ActionId),
    /// The action descriptor itself is inconsistent (see `ActionDescriptor::check`).
    BadDescriptor {
        action: ActionId,
        reason: &'static str,
    },
}

impl fmt::Display for ContractViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTitle => f.write_str("result title is empty"),
            Self::UnknownAction(a) => write!(f, "action `{a}` is not registered"),
            Self::UnsafePrimary(a) => {
                write!(f, "primary action `{a}` is destructive or privileged")
            }
            Self::MissingCapabilities { action, missing } => {
                write!(
                    f,
                    "action `{action}` requires missing capabilities {missing:?}"
                )
            }
            Self::DuplicateAction(a) => write!(f, "action `{a}` is offered more than once"),
            Self::BadDescriptor { action, reason } => {
                write!(f, "descriptor of `{action}` is inconsistent: {reason}")
            }
        }
    }
}

impl std::error::Error for ContractViolation {}

/// Checks one result against the contract, returning every violation found.
///
/// Rules:
/// - non-empty title;
/// - every offered action is registered, has a consistent descriptor and its
///   required capabilities are present on the result;
/// - the primary action is allowed as primary (never destructive/privileged);
/// - no action is offered twice.
#[must_use]
pub fn validate_result<L: ActionLookup + ?Sized>(
    item: &ResultItem,
    actions: &L,
) -> Vec<ContractViolation> {
    let mut violations = Vec::new();
    if item.title.trim().is_empty() {
        violations.push(ContractViolation::EmptyTitle);
    }

    for (index, id) in item.actions().enumerate() {
        if item.actions().take(index).any(|earlier| earlier == id) {
            violations.push(ContractViolation::DuplicateAction(id.clone()));
            continue;
        }
        let Some(descriptor) = actions.action(id) else {
            violations.push(ContractViolation::UnknownAction(id.clone()));
            continue;
        };
        if let Err(reason) = descriptor.check() {
            violations.push(ContractViolation::BadDescriptor {
                action: id.clone(),
                reason,
            });
        }
        if index == 0 && !descriptor.safety.allowed_as_primary() {
            violations.push(ContractViolation::UnsafePrimary(id.clone()));
        }
        let missing = item.capabilities.missing(descriptor.requires);
        if !missing.is_empty() {
            violations.push(ContractViolation::MissingCapabilities {
                action: id.clone(),
                missing,
            });
        }
    }
    violations
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Shared test fixtures: a small action table and a valid file result.

    use std::path::PathBuf;

    use crate::action::{ActionDescriptor, ActionGroup, ActionSafety};
    use crate::capability::{Capability, CapabilitySet};
    use crate::ids::{ActionId, ProviderId, ResultId};
    use crate::result::{Confidence, IconRef, MatchKind, Payload, ResultItem, ResultKind, Score};

    pub(crate) const OPEN: ActionId = ActionId::from_static("test.open");
    pub(crate) const REVEAL: ActionId = ActionId::from_static("test.reveal");
    pub(crate) const COPY: ActionId = ActionId::from_static("test.copy-value");
    pub(crate) const RUN_AS_ADMIN: ActionId = ActionId::from_static("test.run-as-admin");
    pub(crate) const DELETE: ActionId = ActionId::from_static("test.delete");

    const PATH: CapabilitySet = CapabilitySet::of(&[Capability::LocalPath]);

    pub(crate) fn actions() -> [ActionDescriptor; 5] {
        use ActionGroup as G;
        use ActionSafety as S;
        [
            ActionDescriptor::new_static(OPEN, "Open", S::SafeRead, G::Primary, PATH),
            ActionDescriptor::new_static(REVEAL, "Reveal", S::SafeRead, G::Navigation, PATH),
            ActionDescriptor::new_static(
                COPY,
                "Copy value",
                S::SafeRead,
                G::Navigation,
                CapabilitySet::of(&[Capability::TextValue]),
            ),
            ActionDescriptor::new_static(
                RUN_AS_ADMIN,
                "Run as administrator",
                S::Privileged,
                G::Advanced,
                CapabilitySet::of(&[Capability::Launchable]),
            ),
            ActionDescriptor::new_static(DELETE, "Delete", S::Destructive, G::Destructive, PATH),
        ]
    }

    pub(crate) fn file_result() -> ResultItem {
        ResultItem {
            id: ResultId::from_parts("file", "vol1:42").unwrap(),
            provider: ProviderId::from_static("test.files"),
            kind: ResultKind::File,
            title: "notes.md".into(),
            subtitle: None,
            detail: Some(r"C:\Users\Joao\Documents".into()),
            icon: IconRef::FileExtension("md".into()),
            score: Score::new(Confidence::CERTAIN, MatchKind::Exact),
            capabilities: CapabilitySet::of(&[Capability::LocalPath, Capability::Pinnable]),
            primary_action: OPEN,
            secondary_actions: vec![REVEAL, DELETE],
            payload: Payload::Path(PathBuf::from(r"C:\Users\Joao\Documents\notes.md")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::action::{ActionDescriptor, ActionGroup, ActionSafety};
    use crate::capability::{Capability, CapabilitySet};

    #[test]
    fn valid_result_passes() {
        assert_eq!(validate_result(&file_result(), &actions()), vec![]);
    }

    #[test]
    fn destructive_primary_is_rejected() {
        let mut item = file_result();
        item.primary_action = DELETE;
        item.secondary_actions = vec![REVEAL];
        assert_eq!(
            validate_result(&item, &actions()),
            vec![ContractViolation::UnsafePrimary(DELETE)]
        );
    }

    #[test]
    fn privileged_primary_is_rejected() {
        let mut item = file_result();
        item.capabilities = item.capabilities.with(Capability::Launchable);
        item.primary_action = RUN_AS_ADMIN;
        item.secondary_actions = vec![];
        assert_eq!(
            validate_result(&item, &actions()),
            vec![ContractViolation::UnsafePrimary(RUN_AS_ADMIN)]
        );
    }

    #[test]
    fn unknown_duplicate_and_capability_violations() {
        let mut item = file_result();
        item.title = "   ".into();
        let ghost = ActionId::from_static("test.ghost");
        item.secondary_actions = vec![REVEAL, ghost.clone(), OPEN, COPY, REVEAL];
        let got = validate_result(&item, &actions());
        assert_eq!(
            got,
            vec![
                ContractViolation::EmptyTitle,
                ContractViolation::UnknownAction(ghost),
                ContractViolation::DuplicateAction(OPEN),
                ContractViolation::MissingCapabilities {
                    action: COPY,
                    missing: CapabilitySet::of(&[Capability::TextValue]),
                },
                ContractViolation::DuplicateAction(REVEAL),
            ]
        );
        // Messages are human-readable for test failure output.
        assert_eq!(
            got[2].to_string(),
            "action `test.open` is offered more than once"
        );
    }

    #[test]
    fn inconsistent_descriptor_is_reported() {
        let mut table = actions().to_vec();
        table[4] = ActionDescriptor::new_static(
            DELETE,
            "Delete",
            ActionSafety::Destructive,
            ActionGroup::Common, // hides a destructive action among safe ones
            CapabilitySet::of(&[Capability::LocalPath]),
        );
        let got = validate_result(&file_result(), table.as_slice());
        assert!(matches!(
            got.as_slice(),
            [ContractViolation::BadDescriptor { action, .. }] if *action == DELETE
        ));
    }
}
