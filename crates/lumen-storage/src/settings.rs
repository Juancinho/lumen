//! Typed-by-the-caller settings (docs/ARCHITECTURE.md §17): one table, JSON values, keys like
//! `shortcut.toggle`. The owner of a key (shell or core) parses and validates its value.

use rusqlite::{OptionalExtension, params};

use crate::{Result, Store};

impl Store {
    /// The JSON value stored for `key`, if any.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .prepare_cached("SELECT value FROM settings WHERE key = ?1")?
            .query_row([key], |r| r.get(0))
            .optional()?)
    }

    /// Stores `value_json` for `key` (replacing any previous value).
    ///
    /// # Errors
    /// SQLite failure.
    pub fn set_setting(&self, key: &str, value_json: &str) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
            )?
            .execute(params![key, value_json])?;
        Ok(())
    }

    /// Removes `key`; `false` if it was not set.
    ///
    /// # Errors
    /// SQLite failure.
    pub fn remove_setting(&self, key: &str) -> Result<bool> {
        Ok(self
            .conn
            .prepare_cached("DELETE FROM settings WHERE key = ?1")?
            .execute([key])?
            > 0)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    #[test]
    fn set_get_replace_remove() {
        let dir = std::env::temp_dir().join(format!("lumen-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open_writer(&dir.join("s.db")).unwrap();
        assert_eq!(store.setting("shortcut.toggle").unwrap(), None);
        store
            .set_setting("shortcut.toggle", "\"Alt+Space\"")
            .unwrap();
        store
            .set_setting("shortcut.toggle", "\"Ctrl+Space\"")
            .unwrap();
        assert_eq!(
            store.setting("shortcut.toggle").unwrap().as_deref(),
            Some("\"Ctrl+Space\"")
        );
        assert!(store.remove_setting("shortcut.toggle").unwrap());
        assert!(!store.remove_setting("shortcut.toggle").unwrap());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
