//! Stable file identity (docs/ARCHITECTURE.md §5): survives renames and moves within a
//! volume, so a renamed file keeps its item, chunks and vectors instead of being re-indexed.
//!
//! - Windows: volume serial number + 128-bit file id (`GetFileInformationByHandleEx`), stable
//!   on NTFS/ReFS. FAT/exFAT and some network shares synthesize ids that may change; callers
//!   must fall back to path + size + mtime when an identity looks inconsistent (T207).
//! - Unix: device + inode. Inodes are reused quickly after deletion, so identity alone never
//!   proves "same content": combine with size/mtime/fingerprint.

use std::fmt;
use std::io;
use std::path::Path;

use file_id::FileId;

/// Volume + file id. Text form for storage: `items.volume_id` / `items.file_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileIdentity {
    pub volume: u64,
    pub file: u128,
}

impl FileIdentity {
    /// `items.volume_id` value (lowercase hex).
    #[must_use]
    pub fn volume_key(&self) -> String {
        format!("{:x}", self.volume)
    }

    /// `items.file_id` value (lowercase hex).
    #[must_use]
    pub fn file_key(&self) -> String {
        format!("{:x}", self.file)
    }
}

impl fmt::Display for FileIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x}:{:x}", self.volume, self.file)
    }
}

impl From<FileId> for FileIdentity {
    fn from(id: FileId) -> Self {
        match id {
            FileId::Inode {
                device_id,
                inode_number,
            } => Self {
                volume: device_id,
                file: u128::from(inode_number),
            },
            FileId::LowRes {
                volume_serial_number,
                file_index,
            } => Self {
                volume: u64::from(volume_serial_number),
                file: u128::from(file_index),
            },
            FileId::HighRes {
                volume_serial_number,
                file_id,
            } => Self {
                volume: volume_serial_number,
                file: file_id,
            },
        }
    }
}

/// Identity of the entry at `path` (follows a final symlink; callers skip links).
///
/// On Windows this opens a handle with no data access and full sharing, so it works on files
/// locked by other programs and does not read content.
///
/// # Errors
/// Missing file, permission denied.
pub fn identity_of(path: &Path) -> io::Result<FileIdentity> {
    match file_id::get_file_id(path) {
        Ok(id) => Ok(id.into()),
        // Win32 path normalization strips trailing dots/spaces and maps reserved names
        // (`aux.txt`), so names NTFS allows can fail to open; the `\\?\` form skips it.
        #[cfg(windows)]
        Err(e) => match crate::winpath::verbatim(path) {
            Some(v) => file_id::get_file_id(&v).map(Into::into).map_err(|_| e),
            None => Err(e),
        },
        #[cfg(not(windows))]
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_and_move_keep_identity_copy_does_not() {
        let dir = std::env::temp_dir().join(format!("lumen-identity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let a = dir.join("a.txt");
        std::fs::write(&a, b"hello").unwrap();
        let original = identity_of(&a).unwrap();

        let b = dir.join("b.txt");
        std::fs::rename(&a, &b).unwrap();
        assert_eq!(
            identity_of(&b).unwrap(),
            original,
            "rename changed identity"
        );

        let moved = dir.join("sub").join("b.txt");
        std::fs::rename(&b, &moved).unwrap();
        assert_eq!(
            identity_of(&moved).unwrap(),
            original,
            "move changed identity"
        );

        let copy = dir.join("copy.txt");
        std::fs::copy(&moved, &copy).unwrap();
        assert_ne!(
            identity_of(&copy).unwrap(),
            original,
            "copy shares identity"
        );

        assert!(!original.volume_key().is_empty() && !original.file_key().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn directories_have_identity() {
        let dir = std::env::temp_dir();
        assert!(identity_of(&dir).is_ok());
        assert!(identity_of(&dir.join("definitely-missing-lumen-xyz")).is_err());
    }
}
