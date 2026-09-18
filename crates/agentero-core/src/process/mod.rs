pub mod discover;

pub use discover::{path_entries, probe_command, resolve_command, resolve_command_in_paths};

/// Remove the extended-length prefix from local Windows drive paths before
/// handing them to cmd.exe or MSYS2 shells. UNC/device paths retain their own
/// semantics and must not be turned into relative paths by stripping `\\?\`.
pub fn windows_shell_path(path: &std::path::Path) -> std::path::PathBuf {
    let value = path.to_string_lossy();
    match value.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.into(),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_shell_path_strips_extended_prefix() {
        // Rust canonicalize() hands back extended-length drive paths; cmd.exe
        // and MSYS2 shells cannot cd into them, so callers get the plain form.
        assert_eq!(
            windows_shell_path(std::path::Path::new(r"\\?\D:\Documents\Zotero")),
            std::path::PathBuf::from(r"D:\Documents\Zotero")
        );
        assert_eq!(
            windows_shell_path(std::path::Path::new(r"D:\Documents\Zotero")),
            std::path::PathBuf::from(r"D:\Documents\Zotero")
        );
        // UNC layouts have no plain drive form and stay unchanged.
        assert_eq!(
            windows_shell_path(std::path::Path::new(r"\\?\UNC\server\share")),
            std::path::Path::new(r"\\?\UNC\server\share")
        );
        // POSIX paths pass through untouched.
        assert_eq!(
            windows_shell_path(std::path::Path::new("/home/user/vault")),
            std::path::Path::new("/home/user/vault")
        );
    }
}
