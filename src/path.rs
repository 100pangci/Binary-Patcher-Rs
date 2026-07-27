use crate::t;
use std::path::{Path, PathBuf};

pub fn ensure_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn resolve_safe_path(base_dir: &Path, relative_path: &str) -> anyhow::Result<PathBuf> {
    let base_abs = std::path::absolute(base_dir)?;
    let mut result = base_abs.clone();

    for component in Path::new(relative_path).components() {
        match component {
            std::path::Component::ParentDir => {
                if !result.pop() {
                    anyhow::bail!("{}", t!("path.traversal", relative_path));
                }
            }
            std::path::Component::CurDir => {}
            std::path::Component::Normal(c) => result.push(c),
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
