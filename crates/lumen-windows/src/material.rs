//! Window material for the overlay (T004, ADR-024): which system backdrop to use, decided
//! from the Windows build and the user's accessibility settings, plus the DWM calls Tauri
//! does not cover (corner rounding).
//!
//! The decision ([`plan`]) is pure and platform-independent so it is unit-tested everywhere;
//! only [`system_appearance`] and [`round_corners`] touch Windows.
//!
//! Rules (docs/DESIGN_SYSTEM.md §4):
//! - High contrast or "Transparency effects" off → [`Material::Solid`], whatever was asked.
//! - Acrylic/Mica only through the documented `DWMWA_SYSTEMBACKDROP_TYPE`
//!   (Windows 11 22H2, build [`BACKDROP_MIN_BUILD`]); older builds get `Solid` rather than the
//!   undocumented `SetWindowCompositionAttribute`/`DWMWA_MICA_EFFECT` paths.
//! - `Auto` is Acrylic: Microsoft's material for transient, light-dismiss surfaces.
//! - Native rounded corners from Windows 11 (build [`ROUNDED_MIN_BUILD`]); square before.

/// First build with `DWMWA_SYSTEMBACKDROP_TYPE` in a release (Windows 11 22H2).
pub const BACKDROP_MIN_BUILD: u32 = 22621;

/// First build with `DWMWA_WINDOW_CORNER_PREFERENCE` (Windows 11 21H2).
pub const ROUNDED_MIN_BUILD: u32 = 22000;

/// What the user (setting/tray) or `LUMEN_MATERIAL` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MaterialChoice {
    #[default]
    Auto,
    Acrylic,
    Mica,
    Solid,
}

impl MaterialChoice {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Acrylic, Self::Mica, Self::Solid];

    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "acrylic" => Some(Self::Acrylic),
            "mica" => Some(Self::Mica),
            "solid" => Some(Self::Solid),
            _ => None,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Acrylic => "acrylic",
            Self::Mica => "mica",
            Self::Solid => "solid",
        }
    }
}

/// The material actually applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    /// `DWMSBT_TRANSIENTWINDOW`: blurs what is behind the window.
    Acrylic,
    /// `DWMSBT_MAINWINDOW`: tinted from the desktop wallpaper only.
    Mica,
    /// No system backdrop; the UI paints an opaque surface.
    Solid,
}

impl Material {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Acrylic => "acrylic",
            Self::Mica => "mica",
            Self::Solid => "solid",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corners {
    /// DWM-rounded (`DWMWCP_ROUND`, 8 px at 100 % scale), with the system shadow and border.
    Round,
    Square,
}

impl Corners {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Round => "round",
            Self::Square => "square",
        }
    }
}

/// System facts the decision depends on. `build` 0 means "not Windows / unknown".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemAppearance {
    pub build: u32,
    /// Settings → Personalization → Colors → Transparency effects.
    pub transparency_effects: bool,
    pub high_contrast: bool,
}

impl SystemAppearance {
    /// Non-Windows hosts (development, CI): solid, square.
    pub const UNKNOWN: Self = Self {
        build: 0,
        transparency_effects: false,
        high_contrast: false,
    };
}

/// Why the plan differs from what was asked (diagnostics, never user data).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    AsRequested,
    HighContrast,
    TransparencyOff,
    UnsupportedBuild,
}

impl Reason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AsRequested => "as-requested",
            Self::HighContrast => "high-contrast",
            Self::TransparencyOff => "transparency-off",
            Self::UnsupportedBuild => "unsupported-build",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub material: Material,
    pub corners: Corners,
    pub reason: Reason,
}

/// The material and corners to use for `choice` on `sys`.
#[must_use]
pub fn plan(choice: MaterialChoice, sys: SystemAppearance) -> Plan {
    let corners = if sys.build >= ROUNDED_MIN_BUILD {
        Corners::Round
    } else {
        Corners::Square
    };
    let wanted = match choice {
        MaterialChoice::Auto | MaterialChoice::Acrylic => Material::Acrylic,
        MaterialChoice::Mica => Material::Mica,
        MaterialChoice::Solid => Material::Solid,
    };
    let (material, reason) = if wanted == Material::Solid {
        (Material::Solid, Reason::AsRequested)
    } else if sys.high_contrast {
        (Material::Solid, Reason::HighContrast)
    } else if !sys.transparency_effects {
        (Material::Solid, Reason::TransparencyOff)
    } else if sys.build < BACKDROP_MIN_BUILD {
        (Material::Solid, Reason::UnsupportedBuild)
    } else {
        (wanted, Reason::AsRequested)
    };
    Plan {
        material,
        corners,
        reason,
    }
}

/// Reads the Windows build, "Transparency effects" and high contrast. Each WinRT query that
/// fails falls back to the conservative value (transparency off, not high contrast: solid).
#[cfg(windows)]
#[must_use]
pub fn system_appearance() -> SystemAppearance {
    use windows::UI::ViewManagement::{AccessibilitySettings, UISettings};
    let build = windows_version::OsVersion::current().build;
    let transparency_effects = UISettings::new()
        .and_then(|s| s.AdvancedEffectsEnabled())
        .unwrap_or(false);
    let high_contrast = AccessibilitySettings::new()
        .and_then(|s| s.HighContrast())
        .unwrap_or(false);
    SystemAppearance {
        build,
        transparency_effects,
        high_contrast,
    }
}

#[cfg(not(windows))]
#[must_use]
pub fn system_appearance() -> SystemAppearance {
    SystemAppearance::UNKNOWN
}

/// Asks DWM for rounded corners on the top-level window `hwnd` (the raw `HWND` value; an
/// integer so the API stays safe and independent of the caller's `windows` version).
///
/// # Errors
/// The HRESULT text when DWM refuses (e.g. before Windows 11).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn round_corners(hwnd: isize) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DwmSetWindowAttribute,
    };
    let preference: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    let size =
        u32::try_from(size_of::<DWM_WINDOW_CORNER_PREFERENCE>()).map_err(|e| e.to_string())?;
    // SAFETY: `hwnd` is a live top-level window handle owned by the caller's process;
    // `pvattribute` points to a `DWM_WINDOW_CORNER_PREFERENCE` (i32) that outlives the call,
    // and `cbattribute` is exactly its size, as DWMWA_WINDOW_CORNER_PREFERENCE requires.
    unsafe {
        DwmSetWindowAttribute(
            HWND(hwnd as *mut core::ffi::c_void),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            std::ptr::from_ref(&preference).cast(),
            size,
        )
    }
    .map_err(|e| e.message())
}

#[cfg(not(windows))]
/// # Errors
/// Always: there is no DWM off Windows.
pub fn round_corners(_hwnd: isize) -> Result<(), String> {
    Err("not Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIN11_24H2: SystemAppearance = SystemAppearance {
        build: 26100,
        transparency_effects: true,
        high_contrast: false,
    };

    #[test]
    fn choices_round_trip() {
        for c in MaterialChoice::ALL {
            assert_eq!(MaterialChoice::parse(c.as_str()), Some(c));
        }
        assert_eq!(MaterialChoice::parse(" Mica "), Some(MaterialChoice::Mica));
        assert_eq!(MaterialChoice::parse("blur"), None);
        assert_eq!(MaterialChoice::default(), MaterialChoice::Auto);
    }

    #[test]
    fn auto_is_acrylic_on_windows_11_22h2_and_later() {
        let p = plan(MaterialChoice::Auto, WIN11_24H2);
        assert_eq!(p.material, Material::Acrylic);
        assert_eq!(p.corners, Corners::Round);
        assert_eq!(p.reason, Reason::AsRequested);
        let at_min = SystemAppearance {
            build: BACKDROP_MIN_BUILD,
            ..WIN11_24H2
        };
        assert_eq!(plan(MaterialChoice::Mica, at_min).material, Material::Mica);
    }

    #[test]
    fn older_builds_fall_back_to_solid() {
        let win11_21h2 = SystemAppearance {
            build: 22000,
            ..WIN11_24H2
        };
        let p = plan(MaterialChoice::Acrylic, win11_21h2);
        assert_eq!(p.material, Material::Solid);
        assert_eq!(p.corners, Corners::Round);
        assert_eq!(p.reason, Reason::UnsupportedBuild);

        let win10 = SystemAppearance {
            build: 19045,
            ..WIN11_24H2
        };
        let p = plan(MaterialChoice::Auto, win10);
        assert_eq!((p.material, p.corners), (Material::Solid, Corners::Square));

        let p = plan(MaterialChoice::Auto, SystemAppearance::UNKNOWN);
        assert_eq!((p.material, p.corners), (Material::Solid, Corners::Square));
    }

    #[test]
    fn accessibility_settings_win_over_the_choice() {
        let hc = SystemAppearance {
            high_contrast: true,
            ..WIN11_24H2
        };
        assert_eq!(plan(MaterialChoice::Mica, hc).reason, Reason::HighContrast);
        assert_eq!(plan(MaterialChoice::Mica, hc).material, Material::Solid);
        let opaque = SystemAppearance {
            transparency_effects: false,
            ..WIN11_24H2
        };
        assert_eq!(
            plan(MaterialChoice::Acrylic, opaque).reason,
            Reason::TransparencyOff
        );
        // Asking for solid is never a fallback.
        assert_eq!(plan(MaterialChoice::Solid, hc).reason, Reason::AsRequested);
    }
}
