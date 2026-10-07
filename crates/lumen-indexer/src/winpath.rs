//! Windows path forms (only compiled on Windows).

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf, Prefix};

/// `C:\a\b.` → `\\?\C:\a\b.`; `\\srv\share\x` → `\\?\UNC\srv\share\x`.
///
/// Verbatim paths bypass Win32 normalization, which is needed to open names that NTFS stores
/// but Win32 rewrites: trailing dots or spaces, reserved device names (`con`, `aux.txt`).
/// `None` for relative, already-verbatim or `.`/`..`-containing paths.
#[must_use]
pub fn verbatim(path: &Path) -> Option<PathBuf> {
    let mut comps = path.components();
    let Some(Component::Prefix(prefix)) = comps.next() else {
        return None;
    };
    let mut out = match prefix.kind() {
        Prefix::Disk(drive) => PathBuf::from(format!("\\\\?\\{}:\\", char::from(drive))),
        Prefix::UNC(server, share) => {
            let mut s = OsString::from("\\\\?\\UNC\\");
            s.push(server);
            s.push("\\");
            s.push(share);
            s.push("\\");
            PathBuf::from(s)
        }
        _ => return None,
    };
    for c in comps {
        match c {
            Component::RootDir => {}
            Component::Normal(name) => out.push(name),
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_and_unc_paths_become_verbatim() {
        assert_eq!(
            verbatim(Path::new("C:\\a\\b.")).unwrap(),
            PathBuf::from("\\\\?\\C:\\a\\b.")
        );
        assert_eq!(
            verbatim(Path::new("\\\\srv\\share\\x")).unwrap(),
            PathBuf::from("\\\\?\\UNC\\srv\\share\\x")
        );
        assert!(verbatim(Path::new("rel\\x")).is_none());
        assert!(verbatim(Path::new("\\\\?\\C:\\x")).is_none());
        assert!(verbatim(Path::new("C:\\a\\..\\b")).is_none());
    }
}
