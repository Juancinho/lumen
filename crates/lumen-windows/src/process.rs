//! Process CPU time (T014): user + kernel time of a process, used by the indexing
//! throughput benchmarks to report chunks/s together with the CPU share they cost.

use std::time::Duration;

/// Total CPU time (user + kernel) consumed so far by `pid`, or by this process when `pid`
/// is `None`. `None` when the process cannot be queried (gone, access denied) or off
/// Windows.
#[must_use]
pub fn cpu_time(pid: Option<u32>) -> Option<Duration> {
    imp::cpu_time(pid)
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use std::time::Duration;

    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    pub(super) fn cpu_time(pid: Option<u32>) -> Option<Duration> {
        let (handle, owned) = match pid {
            // SAFETY: pseudo-handle of the current process; always valid, never closed.
            None => (unsafe { GetCurrentProcess() }, false),
            // SAFETY: plain query-rights open; the handle is closed below.
            Some(pid) => (
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?,
                true,
            ),
        };
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: valid handle with query rights; the four out-pointers are live locals.
        let ok = unsafe {
            GetProcessTimes(
                handle,
                &raw mut creation,
                &raw mut exit,
                &raw mut kernel,
                &raw mut user,
            )
        }
        .is_ok();
        if owned {
            // SAFETY: handle opened above and not used afterwards.
            let _ = unsafe { CloseHandle(handle) };
        }
        // FILETIME durations are in 100 ns units.
        ok.then(|| Duration::from_nanos((ticks(kernel) + ticks(user)).saturating_mul(100)))
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn cpu_time(_pid: Option<u32>) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn own_cpu_time_is_reported_on_windows() {
        let t = super::cpu_time(None);
        assert_eq!(t.is_some(), cfg!(windows));
    }
}
