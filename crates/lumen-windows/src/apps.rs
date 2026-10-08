//! Every application in the Start menu ("All apps"), packaged (Store/UWP) and desktop, from
//! the shell's `AppsFolder` — the same list as PowerShell's `Get-StartApps`.
//!
//! Each entry's parsing name launches it: `explorer.exe shell:AppsFolder\<parsing name>`
//! (packaged apps: their AppUserModelID; desktop apps: a path or known-folder GUID path).

/// One Start-menu application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartApp {
    /// Display name ("Calculator", "Visual Studio Code").
    pub name: String,
    /// Shell parsing name inside `shell:AppsFolder` (stable launch key).
    pub parsing_name: String,
}

impl StartApp {
    /// URI that opens the app through the shell.
    #[must_use]
    pub fn shell_uri(&self) -> String {
        format!("shell:AppsFolder\\{}", self.parsing_name)
    }
}

/// Enumerates the Start-menu applications of the current user.
///
/// # Errors
/// A message when COM or the shell enumeration fails, or on non-Windows platforms.
pub fn start_apps() -> Result<Vec<StartApp>, String> {
    platform::start_apps()
}

#[cfg(windows)]
#[allow(unsafe_code)] // Shell/COM calls; see SAFETY notes.
mod platform {
    use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize, IBindCtx,
    };
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, KF_FLAG_DEFAULT,
        SHGetKnownFolderItem, SIGDN, SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };

    use super::StartApp;

    /// Balances a successful `CoInitializeEx` on drop.
    struct ComGuard(bool);

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: paired with the successful CoInitializeEx on this thread.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn display_name(item: &IShellItem, kind: SIGDN) -> Option<String> {
        // SAFETY: `item` is a live COM object; GetDisplayName returns a CoTaskMem string
        // that we copy and free exactly once.
        unsafe {
            let raw = item.GetDisplayName(kind).ok()?;
            let text = raw.to_string().ok();
            CoTaskMemFree(Some(raw.as_ptr() as *const core::ffi::c_void));
            text
        }
    }

    pub(super) fn start_apps() -> Result<Vec<StartApp>, String> {
        // SAFETY: plain COM initialization for this thread; S_FALSE (already initialized)
        // still needs a matching CoUninitialize, RPC_E_CHANGED_MODE (MTA thread) does not.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let _com = if hr == RPC_E_CHANGED_MODE {
            ComGuard(false)
        } else if hr.is_ok() {
            ComGuard(true)
        } else {
            return Err(format!("CoInitializeEx: {hr:?}"));
        };

        // SAFETY: standard shell calls on an initialized COM thread; every interface is a
        // reference-counted wrapper released on drop.
        let enumerator: IEnumShellItems = unsafe {
            let folder: IShellItem =
                SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)
                    .map_err(|e| format!("AppsFolder: {e}"))?;
            folder
                .BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)
                .map_err(|e| format!("enumerate AppsFolder: {e}"))?
        };

        let mut apps = Vec::new();
        loop {
            let mut batch: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            // SAFETY: `batch` has room for exactly the one element requested.
            let next = unsafe { enumerator.Next(&mut batch, Some(&raw mut fetched)) };
            if next.is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };
            if let (Some(name), Some(parsing_name)) = (
                display_name(&item, SIGDN_NORMALDISPLAY),
                // Relative to AppsFolder: the AUMID (packaged) or path/GUID path (desktop).
                display_name(&item, SIGDN_PARENTRELATIVEPARSING),
            ) && !name.trim().is_empty()
                && !parsing_name.is_empty()
            {
                apps.push(StartApp { name, parsing_name });
            }
        }
        Ok(apps)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::StartApp;

    pub(super) fn start_apps() -> Result<Vec<StartApp>, String> {
        Err("the Start-menu application catalog is only available on Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_uri_prefixes_apps_folder() {
        let app = StartApp {
            name: "Calculator".into(),
            parsing_name: "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
        };
        assert_eq!(
            app.shell_uri(),
            "shell:AppsFolder\\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        );
    }

    #[cfg(windows)]
    #[test]
    fn enumerates_some_start_apps() {
        let apps = start_apps().unwrap();
        assert!(!apps.is_empty(), "AppsFolder returned no applications");
        assert!(
            apps.iter()
                .all(|a| !a.name.is_empty() && !a.parsing_name.is_empty())
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_elsewhere() {
        assert!(start_apps().is_err());
    }
}
