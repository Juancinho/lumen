//! What the global shortcut does, given the overlay's current state. Pure, so the
//! keyboard contract is unit-tested independently of the window system.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutDecision {
    /// Position on the active monitor, show and focus.
    Show,
    /// Already visible but not focused (e.g. focus was refused): just take focus.
    Focus,
    /// Visible and focused: dismiss.
    Hide,
}

pub(crate) fn on_shortcut(visible: bool, focused: bool) -> ShortcutDecision {
    match (visible, focused) {
        (false, _) => ShortcutDecision::Show,
        (true, false) => ShortcutDecision::Focus,
        (true, true) => ShortcutDecision::Hide,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_toggles_and_recovers_focus() {
        assert_eq!(on_shortcut(false, false), ShortcutDecision::Show);
        assert_eq!(on_shortcut(false, true), ShortcutDecision::Show);
        assert_eq!(on_shortcut(true, false), ShortcutDecision::Focus);
        assert_eq!(on_shortcut(true, true), ShortcutDecision::Hide);
    }
}
