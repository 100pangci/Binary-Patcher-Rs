use std::path::Path;
use walkdir::WalkDir;
use crate::manifest::Manifest;
use crate::utils::{resolve_safe_path, restore_backup, display_path, ensure_parent_dir, BACKUP_SUFFIX};

fn restore_from_staging(target_path: &Path, patch_dir: &Path) -> anyhow::Result<bool> {
    let staging_dir = patch_dir.join(".backup_staging");
    if !staging_dir.exists() {
        return Ok(false);
    }

    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("无效的文件路径: {}", target_path.display()))?;

    let backup_prefix = format!("{file_name}{BACKUP_SUFFIX}");

    // Search staging dir recursively for matching backup file
    for entry in WalkDir::new(&staging_dir).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let fname = entry.file_name().to_string_lossy();
            if fname == backup_prefix || fname.starts_with(&format!("{backup_prefix}.")) {
                ensure_parent_dir(target_path)?;
                std::fs::copy(entry.path(), target_path)?;
                std::fs::remove_file(entry.path())?;
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn cleanup_empty_dirs(start: &Path, base_dir: &Path) -> anyhow::Result<()> {
    let base_abs = std::path::absolute(base_dir)?;
    let mut current = start.to_path_buf();
    loop {
        if current == base_abs {
            break;
        }
        if current.is_dir() {
            let has_entries = current.read_dir()?.next().is_some();
            if !has_entries {
                std::fs::remove_dir(&current)?;
                println!("  清理空目录: {}", display_path(&current, base_dir));
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
    Ok(())
}

pub fn rollback_bundle(base_dir: &Path) -> anyhow::Result<()> {
    let patch_dir = base_dir.join("Patch");

    if !patch_dir.exists() {
        anyhow::bail!(
            "当前目录下未找到 Patch 文件夹: {}\n\
             请把 Patch 文件夹复制到旧版本根目录后，再运行 rollback_patch。",
            patch_dir.display()
        );
    }

    let manifest = Manifest::load(&patch_dir)?;

    let changed = &manifest.changed;
    let added = &manifest.added;
    let deleted = &manifest.deleted;

    println!("检测到可回滚内容: 变更 {}，新增 {}，删除 {}", changed.len(), added.len(), deleted.len());

    let mut restored_count = 0u32;
    let mut removed_count = 0u32;

    // Recreate deleted directories before restoring files (shallowest first)
    let mut deleted_dirs = manifest.deleted_dirs.clone();
    deleted_dirs.sort();
    for dir_path in &deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir)?;
            println!("[重建目录] {dir_path}");
        }
    }

    for item in changed {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("[恢复变更] {}", item.path);
        if restore_backup(&target_path)? {
            restored_count += 1;
        } else {
            println!("  跳过：未找到备份文件");
        }
    }

    for item in deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("[恢复删除] {}", item.path);
        if restore_backup(&target_path)? {
            restored_count += 1;
        } else if restore_from_staging(&target_path, &patch_dir)? {
            restored_count += 1;
            println!("  已从备份暂存区恢复");
        } else {
            println!("  跳过：未找到备份文件");
        }
    }

    for item in added {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("[删除新增] {}", item.path);
        if target_path.exists() {
            if target_path.is_file() {
                std::fs::remove_file(&target_path)?;
                removed_count += 1;
                println!("  已删除新增文件: {}", target_path.display());
                // Clean up empty parent directories created by this added file
                if let Some(parent) = target_path.parent() {
                    cleanup_empty_dirs(parent, base_dir)?;
                }
            } else {
                println!("  跳过：目标是目录，未删除 {}", target_path.display());
            }
        } else {
            println!("  跳过：新增文件不存在 {}", target_path.display());
        }
    }

    println!("\n补丁回滚完成！");
    println!("- 恢复备份文件: {restored_count}");
    println!("- 删除新增文件: {removed_count}");
    println!("说明：已恢复的 *.backup_before_patch 备份文件会被自动删除。");

    Ok(())
}
