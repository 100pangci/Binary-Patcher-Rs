//! 文件系统工具：目录递归遍历、文件/目录相对路径映射、文件复制、空目录清理。

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// 从 start_dir 向上逐级清理空目录，直到 base_dir 为止。
pub fn cleanup_empty_dirs(start_dir: &Path, base_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let base_abs = std::path::absolute(base_dir)?;
    let mut current = start_dir.to_path_buf();
    let mut removed = Vec::new();
    loop {
        if current == base_abs {
            break;
        }
        if current.is_dir() {
            if current.read_dir()?.next().is_none() {
                std::fs::remove_dir(&current)?;
                removed.push(current.clone());
            } else {
                break;
            }
        } else {
            break;
        }
        match current.parent() {
            Some(parent) => current = parent.to_path_buf(),
            None => break,
        }
    }
    Ok(removed)
}

pub fn iter_files(base_dir: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(base_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().is_file() {
                Some(entry.path().to_path_buf())
            } else {
                None
            }
        })
}

pub fn relative_dir_map(base_dir: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    let (_, dirs) = relative_maps(base_dir);
    dirs
}

pub fn relative_maps(
    base_dir: &Path,
) -> (
    std::collections::BTreeMap<String, PathBuf>,
    std::collections::BTreeMap<String, PathBuf>,
) {
    let mut files = std::collections::BTreeMap::new();
    let mut dirs = std::collections::BTreeMap::new();
    let base = base_dir.to_path_buf();
    for entry in WalkDir::new(base_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Ok(rel) = entry.path().strip_prefix(&base) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if rel_str.is_empty() {
                continue;
            }
            if entry.file_type().is_file() {
                files.insert(rel_str, entry.path().to_path_buf());
            } else if entry.file_type().is_dir() {
                dirs.insert(rel_str, entry.path().to_path_buf());
            }
        }
    }
    (files, dirs)
}

pub fn relative_file_map(base_dir: &Path) -> std::collections::BTreeMap<String, PathBuf> {
    let (files, _) = relative_maps(base_dir);
    files
}

pub fn copy_file(src: &Path, dst: &Path) -> anyhow::Result<()> {
    crate::path::ensure_parent_dir(dst)?;
    std::fs::copy(src, dst)?;
    Ok(())
}
