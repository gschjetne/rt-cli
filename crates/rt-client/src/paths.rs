//! Where rt keeps its files. `RT_HOME` puts everything under one directory instead.
//!
//! Without a home directory (no `HOME`), the files go under the temporary directory, in a
//! directory named for the user's id. Another user could make that directory first, so
//! rt trusts a directory only if it is the user's own and nobody else can write to it:
//! [`private_dir`] refuses to use one that is not, and [`read_own`] reads nothing from it.

use std::path::{Path, PathBuf};

fn home_override() -> Option<PathBuf> {
    std::env::var_os("RT_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn fallback() -> PathBuf {
    #[cfg(unix)]
    let name = format!("rt-{}", unsafe { libc::geteuid() });
    #[cfg(not(unix))]
    let name = "rt".to_owned();
    std::env::temp_dir().join(name)
}

/// Whether `dir` is the user's own: a real directory (not a link to one), theirs, and not
/// writable by anyone else. A directory that does not exist yet holds nothing to distrust.
pub fn is_own_dir(dir: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::symlink_metadata(dir) {
            Ok(m) => m.is_dir() && m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o022 == 0,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}

/// A file's text, if it is in a directory of the user's own (see [`is_own_dir`]).
pub fn read_own(path: &Path) -> Option<String> {
    if !path.parent().is_none_or(is_own_dir) {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Machine-local state: the client's remembered sessions.
pub fn state_dir() -> PathBuf {
    match home_override() {
        Some(home) => home.join("state"),
        None => dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .map_or_else(fallback, |d| d.join("rt")),
    }
}

/// The client configuration: `endpoint` and `token`.
pub fn config_file() -> PathBuf {
    match home_override() {
        Some(home) => home.join("config.toml"),
        None => dirs::config_dir()
            .map_or_else(fallback, |d| d.join("rt"))
            .join("config.toml"),
    }
}

/// Create a directory, and its missing parents, readable by its owner only: tokens and
/// remembered sessions live in these.
pub fn private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let m = std::fs::symlink_metadata(dir)?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "{} is not a directory of your own; set RT_HOME to one",
                    dir.display()
                ),
            ));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Whether the config directory is the person's own, rather than the shared fallback
/// under the temporary directory, which must never hold tokens.
pub fn config_is_private() -> bool {
    home_override().is_some() || dirs::config_dir().is_some()
}

pub fn client_state_file() -> PathBuf {
    state_dir().join("client.json")
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn only_own_directories_are_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("own");
        private_dir(&own).unwrap();
        assert!(is_own_dir(&own));
        assert!(is_own_dir(&dir.path().join("not yet")));

        let shared = dir.path().join("shared");
        std::fs::create_dir(&shared).unwrap();
        std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(!is_own_dir(&shared), "writable by others");
        std::fs::write(shared.join("config.toml"), "").unwrap();
        assert_eq!(read_own(&shared.join("config.toml")), None);

        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&own, &link).unwrap();
        assert!(!is_own_dir(&link), "a link to a directory");
        assert!(private_dir(&link).is_err());

        if unsafe { libc::geteuid() } != 0 {
            assert!(!is_own_dir(Path::new("/")), "someone else's");
        }
    }
}
