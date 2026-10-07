//! Typed identifiers shared by providers, the coordinator, actions and shells.
//!
//! Two families:
//!
//! - **Namespaced names** ([`ProviderId`], [`ActionId`]): stable, human-readable,
//!   dot-separated (`lumen.files`, `lumen.reveal`). Built-ins are `'static` and
//!   validated at compile time via `from_static` in a `const`. The `lumen.`
//!   namespace is reserved for built-ins; extension naming is decided by T802.
//! - **Instance identifiers** ([`ResultId`], [`QueryId`]): identify one entity or
//!   one search request at runtime.
//!
//! An invalid built-in name is a compile error:
//!
//! ```compile_fail
//! const BAD: lumen_core::ProviderId = lumen_core::ProviderId::from_static("Files");
//! ```

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

/// Maximum length in bytes of a namespaced name.
pub const MAX_NAME_LEN: usize = 64;

/// Maximum length in bytes of a [`ResultId`].
pub const MAX_RESULT_ID_LEN: usize = 1024;

/// Why an identifier was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdError {
    Empty,
    TooLong {
        max: usize,
    },
    /// A dot-separated segment is empty (`a..b`, `.a`, `a.`).
    EmptySegment,
    /// A segment does not start with `[a-z0-9]`.
    BadSegmentStart,
    /// Byte outside `[a-z0-9_-]` (or `.` as separator).
    InvalidChar,
    /// Needs at least `namespace.name`.
    MissingNamespace,
    /// Result ids must not contain control characters.
    ControlChar,
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("identifier is empty"),
            Self::TooLong { max } => write!(f, "identifier longer than {max} bytes"),
            Self::EmptySegment => f.write_str("identifier has an empty dot-separated segment"),
            Self::BadSegmentStart => f.write_str("identifier segment must start with [a-z0-9]"),
            Self::InvalidChar => f.write_str("identifier may only contain [a-z0-9_-] and '.'"),
            Self::MissingNamespace => f.write_str("identifier needs a namespace: `namespace.name`"),
            Self::ControlChar => f.write_str("identifier contains a control character"),
        }
    }
}

impl std::error::Error for IdError {}

/// Validates the namespaced-name grammar:
/// `segment ("." segment)+`, `segment = [a-z0-9] [a-z0-9_-]*`, total length <= [`MAX_NAME_LEN`].
///
/// `const` so built-in ids are checked at compile time.
pub const fn validate_name(s: &str) -> Result<(), IdError> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(IdError::Empty);
    }
    if bytes.len() > MAX_NAME_LEN {
        return Err(IdError::TooLong { max: MAX_NAME_LEN });
    }
    let mut segments = 1;
    let mut at_segment_start = true;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'.' {
            if at_segment_start {
                return Err(IdError::EmptySegment);
            }
            segments += 1;
            at_segment_start = true;
        } else if at_segment_start {
            if !(b.is_ascii_lowercase() || b.is_ascii_digit()) {
                return Err(IdError::BadSegmentStart);
            }
            at_segment_start = false;
        } else if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-') {
            return Err(IdError::InvalidChar);
        }
        i += 1;
    }
    if at_segment_start {
        return Err(IdError::EmptySegment);
    }
    if segments < 2 {
        return Err(IdError::MissingNamespace);
    }
    Ok(())
}

macro_rules! namespaced_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            /// Built-in identifier. Use in a `const` so an invalid name fails compilation:
            ///
            /// ```
            #[doc = concat!("const ID: lumen_core::", stringify!($name), " = lumen_core::", stringify!($name), "::from_static(\"lumen.example\");")]
            /// ```
            ///
            /// # Panics
            /// If `s` violates the grammar of [`validate_name`]; at compile time when
            /// evaluated in a `const` context.
            #[must_use]
            pub const fn from_static(s: &'static str) -> Self {
                match validate_name(s) {
                    Ok(()) => Self(Cow::Borrowed(s)),
                    Err(_) => panic!(concat!("invalid ", stringify!($name), " literal")),
                }
            }

            /// Runtime identifier (e.g. loaded from configuration).
            ///
            /// # Errors
            /// If `s` violates the grammar of [`validate_name`].
            pub fn new(s: impl Into<String>) -> Result<Self, IdError> {
                let s = s.into();
                validate_name(&s)?;
                Ok(Self(Cow::Owned(s)))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// First segment, e.g. `lumen` for `lumen.files`.
            #[must_use]
            pub fn namespace(&self) -> &str {
                self.0.split('.').next().unwrap_or_default()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

namespaced_id! {
    /// Stable identity of a result source (`lumen.files`, `lumen.apps`, `lumen.calculator`).
    ProviderId
}

namespaced_id! {
    /// Stable identity of an action (`lumen.open`, `lumen.reveal`, `lumen.copy-path`).
    /// Versioning of action semantics, when needed, belongs in the name (`lumen.foo-v2`).
    ActionId
}

/// Stable identity of the **entity** a result represents, independent of which
/// provider or progressive batch produced it.
///
/// Invariant: two providers returning the same entity (e.g. the filename provider
/// and the semantic provider both returning one file) MUST produce equal
/// `ResultId`s. The coordinator merges on it and the UI preserves keyboard
/// selection across re-ranking with it (docs/SEARCH_AND_INDEXING.md §6).
///
/// Convention: `<entity-kind>:<stable key>`, e.g. `file:<item id>`, `app:<app key>`,
/// `calc:<normalized expression>`. Prefer stable storage ids over paths so renames
/// do not change identity.
///
/// Cheap to clone (`Arc<str>`): ids are copied into caches, DTOs and selection state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResultId(Arc<str>);

impl ResultId {
    /// # Errors
    /// If `s` is empty, longer than [`MAX_RESULT_ID_LEN`] bytes or contains control characters.
    pub fn new(s: impl AsRef<str>) -> Result<Self, IdError> {
        let s = s.as_ref();
        if s.is_empty() {
            return Err(IdError::Empty);
        }
        if s.len() > MAX_RESULT_ID_LEN {
            return Err(IdError::TooLong {
                max: MAX_RESULT_ID_LEN,
            });
        }
        if s.chars().any(char::is_control) {
            return Err(IdError::ControlChar);
        }
        Ok(Self(Arc::from(s)))
    }

    /// Convenience for the `<entity-kind>:<key>` convention.
    ///
    /// # Errors
    /// As [`ResultId::new`]; additionally `kind` must be a non-empty `[a-z0-9-]` word.
    pub fn from_parts(kind: &str, key: &str) -> Result<Self, IdError> {
        if kind.is_empty() {
            return Err(IdError::Empty);
        }
        if !kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(IdError::InvalidChar);
        }
        if key.is_empty() {
            return Err(IdError::Empty);
        }
        Self::new(format!("{kind}:{key}"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ResultId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identity of one search request (one query revision typed by the user).
///
/// Monotonic per session: a newer query has a larger id. Anything computed for an
/// older id is stale and must be ignored (docs/ARCHITECTURE.md §4). Ids travel to
/// the UI, so producers must keep them <= 2^53 - 1 (JavaScript safe integer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueryId(u64);

impl QueryId {
    /// Largest id representable exactly in JavaScript (`Number.MAX_SAFE_INTEGER`).
    pub const MAX: Self = Self((1 << 53) - 1);

    /// # Errors
    /// Returns `None` above [`QueryId::MAX`].
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value <= Self::MAX.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// `true` if work tagged with `self` must be discarded because `latest` superseded it.
    #[must_use]
    pub const fn is_stale(self, latest: Self) -> bool {
        self.0 < latest.0
    }
}

impl fmt::Display for QueryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "q{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: ProviderId = ProviderId::from_static("lumen.files");
    const COPY_PATH: ActionId = ActionId::from_static("lumen.copy-path");

    #[test]
    fn static_ids_are_usable_in_const() {
        assert_eq!(FILES.as_str(), "lumen.files");
        assert_eq!(FILES.namespace(), "lumen");
        assert_eq!(COPY_PATH.to_string(), "lumen.copy-path");
    }

    #[test]
    fn accepts_valid_names() {
        for name in [
            "lumen.files",
            "a.b",
            "lumen.copy-path",
            "x1.y_2.z-3",
            "9a.0b",
        ] {
            assert_eq!(validate_name(name), Ok(()), "{name}");
            assert!(ProviderId::new(name).is_ok());
        }
        let longest = format!("a.{}", "b".repeat(MAX_NAME_LEN - 2));
        assert_eq!(validate_name(&longest), Ok(()));
    }

    #[test]
    fn rejects_invalid_names() {
        let too_long = format!("a.{}", "b".repeat(MAX_NAME_LEN - 1));
        let cases: [(&str, IdError); 10] = [
            ("", IdError::Empty),
            (&too_long, IdError::TooLong { max: MAX_NAME_LEN }),
            ("files", IdError::MissingNamespace),
            ("lumen..files", IdError::EmptySegment),
            (".lumen", IdError::EmptySegment),
            ("lumen.", IdError::EmptySegment),
            ("Lumen.files", IdError::BadSegmentStart),
            ("lumen.-files", IdError::BadSegmentStart),
            ("lumen.fi les", IdError::InvalidChar),
            ("lumen.fíles", IdError::InvalidChar),
        ];
        for (name, expected) in cases {
            assert_eq!(validate_name(name), Err(expected), "{name:?}");
            assert_eq!(ActionId::new(name), Err(expected));
        }
    }

    #[test]
    fn static_and_owned_ids_compare_equal() {
        assert_eq!(FILES, ProviderId::new("lumen.files").unwrap());
    }

    #[test]
    #[should_panic(expected = "invalid ProviderId literal")]
    fn from_static_panics_at_runtime_too() {
        let name = std::hint::black_box("NotValid");
        let _ = ProviderId::from_static(name);
    }

    #[test]
    fn result_id_rules() {
        assert_eq!(ResultId::new(""), Err(IdError::Empty));
        assert_eq!(ResultId::new("file:\n1"), Err(IdError::ControlChar));
        assert_eq!(
            ResultId::new("x".repeat(MAX_RESULT_ID_LEN + 1)),
            Err(IdError::TooLong {
                max: MAX_RESULT_ID_LEN
            })
        );
        // Paths and unicode are fine inside the key.
        let id =
            ResultId::from_parts("file", r"vol1/0x1f:C:\Users\Joao\Documentos\notas.md").unwrap();
        assert!(id.as_str().starts_with("file:"));
        assert_eq!(ResultId::from_parts("File", "1"), Err(IdError::InvalidChar));
        assert_eq!(ResultId::from_parts("file", ""), Err(IdError::Empty));
    }

    #[test]
    fn result_id_clone_is_shared() {
        let a = ResultId::new("app:spotify").unwrap();
        let b = a.clone();
        assert!(std::ptr::eq(a.as_str(), b.as_str()));
    }

    #[test]
    fn query_id_staleness_and_js_bound() {
        let q1 = QueryId::new(1).unwrap();
        let q2 = QueryId::new(2).unwrap();
        assert!(q1.is_stale(q2));
        assert!(!q2.is_stale(q2));
        assert!(!q2.is_stale(q1));
        assert_eq!(QueryId::new(QueryId::MAX.get()), Some(QueryId::MAX));
        assert_eq!(QueryId::new(QueryId::MAX.get() + 1), None);
        assert_eq!(q2.to_string(), "q2");
    }
}
