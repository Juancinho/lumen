//! What a result's target *supports*. Actions declare which capabilities they
//! require; an action applies to a result only if the result has all of them.
//!
//! Not to be confused with *permissions* an action needs from the system
//! (`filesystem.write`, `process.launch`, ...), which arrive with the workflow
//! permission model (T501).

use std::fmt;

/// One property of a result's target. Stored as a bit in [`CapabilitySet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum Capability {
    /// Target is a local filesystem path (file or folder): open/reveal/copy path.
    LocalPath = 0,
    /// Target can be launched or run (application, command, workflow).
    Launchable = 1,
    /// Target has a textual value worth copying/pasting (calculation, snippet, URL).
    TextValue = 2,
    /// Target can be pinned/favorited in the local usage store.
    Pinnable = 3,
    /// The matching passage has an extracted symbol name.
    CodeSymbol = 4,
    /// The code file has a known local repository root.
    Repository = 5,
}

impl Capability {
    /// Every capability, in bit order.
    pub const ALL: [Self; 6] = [
        Self::LocalPath,
        Self::Launchable,
        Self::TextValue,
        Self::Pinnable,
        Self::CodeSymbol,
        Self::Repository,
    ];

    const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// Small copyable bitset of [`Capability`]. Subset checks are a single AND.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CapabilitySet(u32);

impl CapabilitySet {
    pub const EMPTY: Self = Self(0);

    /// `const` builder: `CapabilitySet::of(&[Capability::LocalPath, Capability::Pinnable])`.
    #[must_use]
    pub const fn of(caps: &[Capability]) -> Self {
        let mut bits = 0;
        let mut i = 0;
        while i < caps.len() {
            bits |= caps[i].bit();
            i += 1;
        }
        Self(bits)
    }

    #[must_use]
    pub const fn with(self, cap: Capability) -> Self {
        Self(self.0 | cap.bit())
    }

    #[must_use]
    pub const fn contains(self, cap: Capability) -> bool {
        self.0 & cap.bit() != 0
    }

    /// `true` if every capability in `required` is present in `self`.
    #[must_use]
    pub const fn contains_all(self, required: Self) -> bool {
        self.0 & required.0 == required.0
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Capabilities in `required` that `self` lacks.
    #[must_use]
    pub const fn missing(self, required: Self) -> Self {
        Self(required.0 & !self.0)
    }

    pub fn iter(self) -> impl Iterator<Item = Capability> {
        Capability::ALL
            .into_iter()
            .filter(move |c| self.contains(*c))
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<I: IntoIterator<Item = Capability>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::with)
    }
}

impl fmt::Debug for CapabilitySet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: CapabilitySet = CapabilitySet::of(&[Capability::LocalPath, Capability::Pinnable]);

    #[test]
    fn bits_are_distinct() {
        let all: CapabilitySet = Capability::ALL.into_iter().collect();
        assert_eq!(all.iter().count(), Capability::ALL.len());
    }

    #[test]
    fn subset_logic() {
        let path_only = CapabilitySet::of(&[Capability::LocalPath]);
        assert!(FILE.contains_all(path_only));
        assert!(FILE.contains_all(CapabilitySet::EMPTY));
        assert!(!path_only.contains_all(FILE));
        assert_eq!(
            path_only.missing(FILE),
            CapabilitySet::of(&[Capability::Pinnable])
        );
        assert!(FILE.missing(path_only).is_empty());
    }

    #[test]
    fn debug_lists_members() {
        assert_eq!(format!("{FILE:?}"), "{LocalPath, Pinnable}");
    }
}
