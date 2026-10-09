//! Built-in action identities and descriptors (docs/COMMAND_MODEL.md §5–§6).
//!
//! Executors live in the shell/OS layer (T109); the registry is T108. Providers use
//! these ids so their results validate against [`DESCRIPTORS`].

use crate::action::{ActionDescriptor, ActionGroup, ActionSafety};
use crate::capability::{Capability, CapabilitySet};
use crate::ids::ActionId;

/// Open a file or folder with its default handler.
pub const OPEN: ActionId = ActionId::from_static("lumen.open");
/// Launch an application.
pub const LAUNCH: ActionId = ActionId::from_static("lumen.launch");
/// Show a file or folder selected in Explorer.
pub const REVEAL: ActionId = ActionId::from_static("lumen.reveal");
/// Copy the full path to the clipboard.
pub const COPY_PATH: ActionId = ActionId::from_static("lumen.copy-path");
/// Leave a folder out of Lumen's locations (T111); undone from the exclusions list.
pub const EXCLUDE_FOLDER: ActionId = ActionId::from_static("lumen.exclude-folder");
pub const EXCLUDE_FILE: ActionId = ActionId::from_static("lumen.exclude-file");
pub const EXCLUDE_EXTENSION: ActionId = ActionId::from_static("lumen.exclude-extension");
pub const COPY_SYMBOL: ActionId = ActionId::from_static("lumen.copy-symbol");
pub const REVEAL_REPOSITORY: ActionId = ActionId::from_static("lumen.reveal-repository");
pub const OPEN_PDF_PAGE: ActionId = ActionId::from_static("lumen.open-pdf-page");

const LOCAL_PATH: CapabilitySet = CapabilitySet::of(&[Capability::LocalPath]);
const LAUNCHABLE: CapabilitySet = CapabilitySet::of(&[Capability::Launchable]);

/// Descriptors of the built-in actions, in panel order.
pub const DESCRIPTORS: [ActionDescriptor; 10] = [
    ActionDescriptor::new_static(
        OPEN,
        "Open",
        ActionSafety::SafeRead,
        ActionGroup::Primary,
        LOCAL_PATH,
    ),
    ActionDescriptor::new_static(
        LAUNCH,
        "Launch",
        ActionSafety::SafeRead,
        ActionGroup::Primary,
        LAUNCHABLE,
    ),
    ActionDescriptor::new_static(
        REVEAL,
        "Reveal in Explorer",
        ActionSafety::SafeRead,
        ActionGroup::Navigation,
        LOCAL_PATH,
    ),
    ActionDescriptor::new_static(
        COPY_PATH,
        "Copy path",
        ActionSafety::SafeRead,
        ActionGroup::Navigation,
        LOCAL_PATH,
    ),
    ActionDescriptor::new_static(
        COPY_SYMBOL,
        "Copy symbol",
        ActionSafety::SafeRead,
        ActionGroup::Common,
        CapabilitySet::of(&[Capability::CodeSymbol]),
    ),
    ActionDescriptor::new_static(
        REVEAL_REPOSITORY,
        "Reveal repository in Explorer",
        ActionSafety::SafeRead,
        ActionGroup::Navigation,
        CapabilitySet::of(&[Capability::Repository]),
    ),
    ActionDescriptor::new_static(
        OPEN_PDF_PAGE,
        "Open matched PDF page",
        ActionSafety::SafeRead,
        ActionGroup::Common,
        CapabilitySet::of(&[Capability::LocalPath, Capability::PdfPage]),
    ),
    ActionDescriptor::new_static(
        EXCLUDE_FOLDER,
        "Exclude folder from Lumen",
        ActionSafety::SafeReversible,
        ActionGroup::Advanced,
        LOCAL_PATH,
    ),
    ActionDescriptor::new_static(
        EXCLUDE_FILE,
        "Exclude this file from Lumen",
        ActionSafety::SafeReversible,
        ActionGroup::Advanced,
        LOCAL_PATH,
    ),
    ActionDescriptor::new_static(
        EXCLUDE_EXTENSION,
        "Exclude this file type from Lumen",
        ActionSafety::SafeReversible,
        ActionGroup::Advanced,
        LOCAL_PATH,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionLookup;

    #[test]
    fn descriptors_are_consistent_and_unique() {
        for (i, d) in DESCRIPTORS.iter().enumerate() {
            d.check().unwrap();
            assert!(d.safety.allowed_as_primary(), "{}", d.id);
            assert!(DESCRIPTORS[i + 1..].iter().all(|o| o.id != d.id));
        }
        assert_eq!(
            DESCRIPTORS.action(&REVEAL).map(|d| d.group),
            Some(ActionGroup::Navigation)
        );
    }
}
