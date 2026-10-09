//! Supported registered PDF viewer transport. No registry command line is executed.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageLaunch {
    pub executable: PathBuf,
    pub arguments: Vec<OsString>,
}

/// Only the documented SumatraPDF transport is supported for now. Unknown handlers
/// keep ordinary file-open and Lumen's matching-page preview available.
#[must_use]
pub fn plan(executable: &Path, file: &Path, page: u32) -> Option<PageLaunch> {
    if !executable.is_absolute() || !file.is_absolute() || page == 0 || page > super::pdf::MAX_PAGES
    {
        return None;
    }
    if !executable
        .file_name()?
        .to_str()?
        .eq_ignore_ascii_case("SumatraPDF.exe")
    {
        return None;
    }
    Some(PageLaunch {
        executable: executable.to_owned(),
        arguments: vec![
            "-reuse-instance".into(),
            "-page".into(),
            page.to_string().into(),
            file.as_os_str().to_owned(),
        ],
    })
}

/// OS-selected .pdf executable. Bounded query, no parsing of registry templates.
#[must_use]
pub fn registered() -> Option<PathBuf> {
    platform::registered()
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod platform {
    use super::*;
    use windows::Win32::UI::Shell::{ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, AssocQueryStringW};
    use windows::core::{PWSTR, w};
    pub(super) fn registered() -> Option<PathBuf> {
        let mut buffer = vec![0u16; 32768];
        let mut count = buffer.len() as u32;
        // SAFETY: NUL-terminated constant inputs and live bounded UTF-16 output buffer.
        unsafe {
            AssocQueryStringW(
                ASSOCF_NOTRUNCATE,
                ASSOCSTR_EXECUTABLE,
                w!(".pdf"),
                w!("open"),
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut count,
            )
        }
        .ok()
        .ok()?;
        let end = buffer.iter().position(|&v| v == 0)?;
        use std::os::windows::ffi::OsStringExt;
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }
}
#[cfg(not(windows))]
mod platform {
    pub(super) fn registered() -> Option<std::path::PathBuf> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_documented_viewer_with_separate_arguments_is_supported() {
        let root = std::env::temp_dir();
        let exe = root.join("SumatraPDF.exe");
        let pdf = root.join("notes & costs 東京.pdf");
        let launch = plan(&exe, &pdf, 7).unwrap();
        assert_eq!(
            launch.arguments,
            [
                OsString::from("-reuse-instance"),
                "-page".into(),
                "7".into(),
                pdf.into_os_string()
            ]
        );
        assert!(plan(&root.join("msedge.exe"), &root.join("x.pdf"), 7).is_none());
        assert!(plan(Path::new("SumatraPDF.exe"), &root.join("x.pdf"), 7).is_none());
        assert!(plan(&exe, &root.join("x.pdf"), 0).is_none());
    }
}
