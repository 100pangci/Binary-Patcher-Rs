use crate::t;
use std::path::{Path, PathBuf};

fn reject_symlink(path: &Path, display_path: &str) -> anyhow::Result<()> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };

    if metadata.file_type().is_symlink() {
        anyhow::bail!("{}", t!("path.symlink", display_path));
    }
    Ok(())
}

/// Reject a path if any component it will create or traverse is a symbolic
/// link.
///
/// The path is walked upward from itself to the deepest component that
/// already exists.  If that component is a symbolic link the path is
/// rejected; components above it belong to the environment the caller chose
/// (for example macOS maps `/var` and `/tmp` through symlinks) and are
/// trusted, so symlinks planted by untrusted input below that anchor are the
/// only ones that matter.  Missing final components are allowed so callers
/// can safely create a new file or directory after this check.  The check is
/// intentionally performed before directory creation by `ensure_parent_dir`.
pub fn ensure_no_symlink_components(path: &Path) -> anyhow::Result<()> {
    let absolute = std::path::absolute(path)?;
    let display_path = path.display().to_string();
    let mut current: &Path = absolute.as_path();

    loop {
        match std::fs::symlink_metadata(current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    anyhow::bail!("{}", t!("path.symlink", display_path));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => match current.parent() {
                Some(parent) => current = parent,
                None => return Ok(()),
            },
            Err(error) => return Err(error.into()),
        }
    }
}

pub fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    ensure_no_symlink_components(path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    ensure_no_symlink_components(path)?;
    Ok(())
}

pub fn resolve_safe_path(base_dir: &Path, relative_path: &str) -> anyhow::Result<PathBuf> {
    let base_abs = std::path::absolute(base_dir)?;
    let mut result = base_abs.clone();

    for component in Path::new(relative_path).components() {
        match component {
            std::path::Component::ParentDir => {
                if !result.starts_with(&base_abs) || result == base_abs || !result.pop() {
                    anyhow::bail!("{}", t!("path.traversal", relative_path));
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(c) => {
                result.push(c);
                reject_symlink(&result, relative_path)?;
            }
            _ => {
                anyhow::bail!("{}", t!("path.absolute-component", relative_path));
            }
        }
    }

    if result.starts_with(&base_abs) {
        Ok(result)
    } else {
        anyhow::bail!("{}", t!("path.traversal", relative_path))
    }
}

pub fn display_path(path: &Path, base_dir: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(base_dir) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        path.to_string_lossy().to_string()
    }
}
