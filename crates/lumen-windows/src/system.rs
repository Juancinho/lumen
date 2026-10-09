//! System state for the indexing resource policy (T202, ADR-019/029): power source, free
//! memory, and how long the user has been idle. Every probe returns `None` when unknown
//! (and off Windows), and the policy treats unknown conservatively.

use std::time::Duration;

/// Power source as Windows reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerStatus {
    /// `Some(true)` on battery, `Some(false)` on AC, `None` unknown.
    pub on_battery: Option<bool>,
    /// 0–100 when known.
    pub battery_percent: Option<u8>,
}

#[must_use]
pub fn power_status() -> Option<PowerStatus> {
    imp::power_status()
}

/// Physical memory available to processes, MiB.
#[must_use]
pub fn available_memory_mib() -> Option<u64> {
    imp::available_memory_mib()
}

/// Time since the last keyboard/mouse input in this session.
#[must_use]
pub fn input_idle() -> Option<Duration> {
    imp::input_idle()
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::time::Duration;

    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::SystemInformation::{
        GetTickCount, GlobalMemoryStatusEx, MEMORYSTATUSEX,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    use super::PowerStatus;

    pub(super) fn power_status() -> Option<PowerStatus> {
        let mut s = SYSTEM_POWER_STATUS::default();
        // SAFETY: out-pointer to a live local.
        unsafe { GetSystemPowerStatus(&raw mut s) }.ok()?;
        Some(PowerStatus {
            on_battery: match s.ACLineStatus {
                0 => Some(true),
                1 => Some(false),
                _ => None,
            },
            battery_percent: (s.BatteryLifePercent <= 100).then_some(s.BatteryLifePercent),
        })
    }

    pub(super) fn available_memory_mib() -> Option<u64> {
        let mut m = MEMORYSTATUSEX {
            dwLength: u32::try_from(size_of::<MEMORYSTATUSEX>()).ok()?,
            ..MEMORYSTATUSEX::default()
        };
        // SAFETY: `dwLength` set as required; out-pointer to a live local.
        unsafe { GlobalMemoryStatusEx(&raw mut m) }.ok()?;
        Some(m.ullAvailPhys / (1024 * 1024))
    }

    pub(super) fn input_idle() -> Option<Duration> {
        let mut info = LASTINPUTINFO {
            cbSize: u32::try_from(size_of::<LASTINPUTINFO>()).ok()?,
            dwTime: 0,
        };
        // SAFETY: `cbSize` set as required; out-pointer to a live local.
        if !unsafe { GetLastInputInfo(&raw mut info) }.as_bool() {
            return None;
        }
        // SAFETY: no arguments. Both values are 32-bit tick counts: wrapping difference.
        let now = unsafe { GetTickCount() };
        Some(Duration::from_millis(u64::from(
            now.wrapping_sub(info.dwTime),
        )))
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn power_status() -> Option<super::PowerStatus> {
        None
    }
    pub(super) fn available_memory_mib() -> Option<u64> {
        None
    }
    pub(super) fn input_idle() -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn probes_answer_on_windows_only() {
        assert_eq!(super::available_memory_mib().is_some(), cfg!(windows));
        assert_eq!(super::power_status().is_some(), cfg!(windows));
    }
}
