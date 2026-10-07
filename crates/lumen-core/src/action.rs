//! Action metadata (docs/COMMAND_MODEL.md §5–§6, §9).
//!
//! A descriptor says *what* an action is, how risky it is, where it sorts in the
//! Action Panel and which result capabilities it needs. Executing actions is the
//! job of the action registry/executors (T108/T109), not of this module.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::capability::CapabilitySet;
use crate::ids::ActionId;

/// Risk class of an action (docs/COMMAND_MODEL.md §9, docs/PRIVACY_SECURITY.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionSafety {
    /// Reads or presents; no side effects beyond the obvious (open, reveal, preview, copy).
    SafeRead,
    /// Changes local state in an easily undone way (pin, add to collection).
    SafeReversible,
    /// Elevation or system-level change (run as administrator, change a setting).
    Privileged,
    /// Irreversible or data-losing (delete, terminate process, overwrite).
    Destructive,
    /// Sends local data off-device (web search with the query, share).
    ExternalData,
}

impl ActionSafety {
    /// Whether the user must explicitly confirm before execution.
    ///
    /// Destructive and privileged actions always require it. External-data actions
    /// currently do not (e.g. a web-search quicklink is the expected result of the
    /// query); their disclosure policy is decided with quicklinks (T404).
    #[must_use]
    pub const fn requires_confirmation(self) -> bool {
        matches!(self, Self::Destructive | Self::Privileged)
    }

    /// Whether an action of this class may be a result's primary (Enter) action.
    /// Enter must be safe and predictable: never destructive, never elevated.
    #[must_use]
    pub const fn allowed_as_primary(self) -> bool {
        !matches!(self, Self::Destructive | Self::Privileged)
    }
}

/// Action Panel ordering bucket (docs/COMMAND_MODEL.md §6). `Ord` follows display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionGroup {
    /// Primary/default actions (open, launch, run).
    Primary,
    /// Common safe contextual actions (Quick Look, find similar, pin).
    Common,
    /// Navigation and copy (reveal, copy path, copy value).
    Navigation,
    /// Advanced (open with, run as administrator).
    Advanced,
    /// Destructive; rendered visually separated and explicit.
    Destructive,
}

/// Static description of one action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionDescriptor {
    pub id: ActionId,
    /// Short imperative label shown in the Action Panel ("Open", "Reveal in Explorer").
    /// English for now; localization readiness is T805.
    pub title: Cow<'static, str>,
    pub safety: ActionSafety,
    pub group: ActionGroup,
    /// Capabilities a result must have for this action to apply.
    pub requires: CapabilitySet,
}

impl ActionDescriptor {
    /// `const` constructor for built-in descriptors.
    #[must_use]
    pub const fn new_static(
        id: ActionId,
        title: &'static str,
        safety: ActionSafety,
        group: ActionGroup,
        requires: CapabilitySet,
    ) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            safety,
            group,
            requires,
        }
    }

    /// Whether this action can apply to a result with `capabilities`.
    #[must_use]
    pub const fn applies_to(&self, capabilities: CapabilitySet) -> bool {
        capabilities.contains_all(self.requires)
    }

    /// Descriptor-level consistency: destructive actions live in the destructive group
    /// and only there, so the panel can always separate them.
    ///
    /// # Errors
    /// Describes the inconsistency.
    pub fn check(&self) -> Result<(), &'static str> {
        let destructive_safety = self.safety == ActionSafety::Destructive;
        let destructive_group = self.group == ActionGroup::Destructive;
        match (destructive_safety, destructive_group) {
            (true, false) => Err("destructive action must be in ActionGroup::Destructive"),
            (false, true) => Err("only destructive actions may use ActionGroup::Destructive"),
            _ if self.title.trim().is_empty() => Err("action title is empty"),
            _ => Ok(()),
        }
    }
}

/// Read access to known action descriptors (implemented by the future registry, T108).
pub trait ActionLookup {
    fn action(&self, id: &ActionId) -> Option<&ActionDescriptor>;
}

impl ActionLookup for [ActionDescriptor] {
    fn action(&self, id: &ActionId) -> Option<&ActionDescriptor> {
        self.iter().find(|a| &a.id == id)
    }
}

impl<const N: usize> ActionLookup for [ActionDescriptor; N] {
    fn action(&self, id: &ActionId) -> Option<&ActionDescriptor> {
        self.as_slice().action(id)
    }
}

impl<S: std::hash::BuildHasher> ActionLookup for HashMap<ActionId, ActionDescriptor, S> {
    fn action(&self, id: &ActionId) -> Option<&ActionDescriptor> {
        self.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;

    const DELETE: ActionDescriptor = ActionDescriptor::new_static(
        ActionId::from_static("test.delete"),
        "Delete",
        ActionSafety::Destructive,
        ActionGroup::Destructive,
        CapabilitySet::of(&[Capability::LocalPath]),
    );

    #[test]
    fn safety_policy() {
        use ActionSafety::*;
        for s in [SafeRead, SafeReversible, ExternalData] {
            assert!(s.allowed_as_primary(), "{s:?}");
            assert!(!s.requires_confirmation(), "{s:?}");
        }
        for s in [Privileged, Destructive] {
            assert!(!s.allowed_as_primary(), "{s:?}");
            assert!(s.requires_confirmation(), "{s:?}");
        }
    }

    #[test]
    fn groups_sort_in_panel_order() {
        let mut groups = [
            ActionGroup::Destructive,
            ActionGroup::Navigation,
            ActionGroup::Primary,
            ActionGroup::Advanced,
            ActionGroup::Common,
        ];
        groups.sort();
        assert_eq!(
            groups,
            [
                ActionGroup::Primary,
                ActionGroup::Common,
                ActionGroup::Navigation,
                ActionGroup::Advanced,
                ActionGroup::Destructive
            ]
        );
    }

    #[test]
    fn descriptor_consistency() {
        assert_eq!(DELETE.check(), Ok(()));
        let mut hidden_delete = DELETE.clone();
        hidden_delete.group = ActionGroup::Common;
        assert!(hidden_delete.check().is_err());
        let mut fake_destructive = DELETE.clone();
        fake_destructive.safety = ActionSafety::SafeRead;
        assert!(fake_destructive.check().is_err());
        let mut untitled = DELETE.clone();
        untitled.title = "  ".into();
        assert!(untitled.check().is_err());
    }

    #[test]
    fn applicability_and_lookup() {
        assert!(DELETE.applies_to(CapabilitySet::of(&[
            Capability::LocalPath,
            Capability::Pinnable
        ])));
        assert!(!DELETE.applies_to(CapabilitySet::of(&[Capability::TextValue])));

        let table = [DELETE];
        assert!(table.action(&DELETE.id).is_some());
        assert!(
            table
                .action(&ActionId::from_static("test.missing"))
                .is_none()
        );
        let map: HashMap<_, _> = [(DELETE.id.clone(), DELETE)].into_iter().collect();
        assert_eq!(
            map.action(&DELETE.id).map(|a| a.safety),
            Some(ActionSafety::Destructive)
        );
    }
}
