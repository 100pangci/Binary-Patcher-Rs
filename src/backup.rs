use crate::t;
use std::path::{Path, PathBuf};

pub const BACKUP_SUFFIX: &str = ".backup_before_patch";

pub fn backup_root_dir(patch_dir: &Path) -> PathBuf {
    patch_dir.join(".backup_before_patch")
}

pub fn create_backup(
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<PathBuf> {
    let data = std::fs::read(target_path)?;
    write_backup(&data, target_path, base_dir, backup_root)
}

pub fn write_backup(
    data: &[u8],
    target_path: &Path,
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<PathBuf> {
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("{}", t!("backup.invalid-path", target_path.display())))?;

    let rel = target_path
        .parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));

    let backup_dir = backup_root.join(rel);
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
            Ok(mut f) => {
                std::io::Write::write_all(&mut f, data)?;
                return Ok(backup_path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                retry += 1;
                if retry >= max_retries {
                    anyhow::bail!("{}", t!("backup.retry-exhausted", max_retries, backup_path.display()));
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
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(&backup_prefix))
            })
            .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
    };

    let do_restore = |backup_path: &Path| -> anyhow::Result<bool> {
        crate::path::ensure_parent_dir(target_path)?;
        if target_path.exists() {
            std::fs::remove_file(target_path)?;
        }
        match std::fs::rename(backup_path, target_path) {
            Ok(()) => Ok(true),
            Err(_) => {
                std::fs::copy(backup_path, target_path)?;
                std::fs::remove_file(backup_path)?;
                Ok(true)
            }
        }
    };

    let rel = target_path
        .parent()
        .and_then(|p| p.strip_prefix(base_dir).ok())
        .unwrap_or(Path::new(""));
    let backup_dir = backup_root.join(rel);
    if let Some(path) = find_newest(&backup_dir) {
        return do_restore(&path);
    }

    let parent = target_path.parent().unwrap_or(Path::new("."));
    if let Some(path) = find_newest(parent) {
        return do_restore(&path);
    }

    Ok(false)
}
