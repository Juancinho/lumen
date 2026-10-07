//! Static build identity of the core, used by shells for diagnostics/About.

/// Product name shown by shells. Working codename until branding is final.
const PRODUCT_NAME: &str = "Lumen";

/// Identity of the compiled core library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreInfo {
    /// Human-readable product name.
    pub product_name: &'static str,
    /// Semantic version of the core crate (from `Cargo.toml`).
    pub version: &'static str,
}

/// Returns the identity of this core build. Pure and allocation-free.
#[must_use]
pub const fn core_info() -> CoreInfo {
    CoreInfo {
        product_name: PRODUCT_NAME,
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_product_name() {
        assert_eq!(core_info().product_name, "Lumen");
    }

    #[test]
    fn version_is_semver_like() {
        let version = core_info().version;
        let parts: Vec<&str> = version.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH, got {version}");
        assert!(
            parts
                .iter()
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
            "non-numeric version component in {version}"
        );
    }
}
