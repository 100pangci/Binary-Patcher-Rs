use crate::t;
use std::path::{Path, PathBuf};

pub const BACKUP_SUFFIX: &str = ".backup_before_patch";

pub fn backup_root_dir(patch_dir: &Path) -> PathBuf {
    patch_dir.join(".backup_before_patch")
}

pub fn checked_backup_root_dir(patch_dir: &Path) -> anyhow::Result<PathBuf> {
    let backup_root = backup_root_dir(patch_dir);
    crate::path::ensure_no_symlink_components(&backup_root)?;
    Ok(backup_root)
}

/// 流式备份文件：绝不把整个文件读入内存，适合大文件。
pub fn create_backup(
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<PathBuf> {
    let mut source = std::fs::File::open(target_path)?;
    let (mut backup_file, backup_path) = open_backup_file(target_path, base_dir, backup_root)?;
    if let Err(error) = std::io::copy(&mut source, &mut backup_file) {
        drop(backup_file);
        let _ = std::fs::remove_file(&backup_path);
        return Err(error.into());
    }
    Ok(backup_path)
}

/// 从内存数据写备份（小文件快速路径）。
pub fn write_backup(
    data: &[u8],
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<PathBuf> {
    let (mut backup_file, backup_path) = open_backup_file(target_path, base_dir, backup_root)?;
    std::io::Write::write_all(&mut backup_file, data)?;
    Ok(backup_path)
}

/// 在备份目录中创建唯一的备份文件（处理重名重试），返回文件句柄与路径。
fn open_backup_file(
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<(std::fs::File, PathBuf)> {
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("{}", t!("backup.invalid-path", target_path.display())))?;

    let rel = target_path
        .parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));

    crate::path::ensure_no_symlink_components(backup_root)?;
    let backup_dir = crate::path::resolve_safe_path(backup_root, &rel.to_string_lossy())?;
    crate::path::ensure_parent_dir(&backup_dir.join(file_name))?;

    let backup_name = format!("{file_name}{BACKUP_SUFFIX}");
    let mut backup_path = backup_dir.join(&backup_name);
    let mut retry = 0u32;
    let max_retries = 10;

    loop {
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&backup_path)
        {
            Ok(f) => return Ok((f, backup_path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                retry += 1;
                if retry >= max_retries {
                    anyhow::bail!(
                        "{}",
                        t!("backup.retry-exhausted", max_retries, backup_path.display())
                    );
                }
                let timestamp = chrono::Local::now().format(".%Y%m%d%H%M%S");
                backup_path =
                    backup_dir.join(format!("{file_name}{BACKUP_SUFFIX}{timestamp}_{retry}"));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

pub fn restore_backup(
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<bool> {
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("{}", t!("backup.invalid-path", target_path.display())))?;

    let backup_prefix = format!("{file_name}{BACKUP_SUFFIX}");

    let find_newest = |dir: &Path| -> Option<PathBuf> {
        std::fs::read_dir(dir)
            .ok()?
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                if file_type.is_symlink() || !file_type.is_file() {
                    return None;
                }
                let name = entry.file_name();
                if !name.to_string_lossy().starts_with(&backup_prefix) {
                    return None;
                }
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some((entry.path(), modified))
            })
            .max_by_key(|(_, modified)| *modified)
            .map(|(path, _)| path)
    };

    let do_restore = |backup_path: &Path| -> anyhow::Result<bool> {
        crate::path::ensure_parent_dir(target_path)?;
        if target_path.exists() {
            std::fs::remove_file(target_path)?;
        }
        if std::fs::rename(backup_path, target_path).is_err() {
            std::fs::copy(backup_path, target_path)?;
            std::fs::remove_file(backup_path)?;
        }
        Ok(true)
    };

    let rel = target_path
        .parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));
    crate::path::ensure_no_symlink_components(backup_root)?;
    let backup_dir = crate::path::resolve_safe_path(backup_root, &rel.to_string_lossy())?;
    if let Some(path) = find_newest(&backup_dir) {
        return do_restore(&path);
    }

    let parent = target_path.parent().unwrap_or(Path::new("."));
    if let Some(path) = find_newest(parent) {
        return do_restore(&path);
    }

    Ok(false)
}
