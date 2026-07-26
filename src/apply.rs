use std::path::Path;
use crate::hdiffpatch::run_hpatchz;
use crate::manifest::Manifest;
use crate::utils::{sha256_of_file, ensure_parent_dir, create_backup, resolve_safe_path, copy_file};

pub fn apply_bundle(base_dir: &Path) -> anyhow::Result<()> {
    let patch_dir = base_dir.join("Patch");

    if !patch_dir.exists() {
        anyhow::bail!(
            "当前目录下未找到 Patch 文件夹: {}\n\
             请把 Patch 文件夹复制到旧版本根目录后，再运行 apply_patch。",
            patch_dir.display()
        );
    }

    let manifest = Manifest::load(&patch_dir)?;

    let changed = &manifest.changed;
    let added = &manifest.added;
    let deleted = &manifest.deleted;

    println!("检测到补丁内容: 变更 {}，新增 {}，删除 {}", changed.len(), added.len(), deleted.len());

    for item in changed {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let patch_file = resolve_safe_path(&patch_dir, &item.patch_file)?;

        if !target_path.exists() {
            anyhow::bail!("错误: 缺少需要打补丁的旧文件: {}", target_path.display());
        }

        let current_hash = sha256_of_file(&target_path)?;
        if current_hash != item.old_sha256 {
            anyhow::bail!(
                "错误: 文件校验不匹配，无法应用补丁: {}\n\
                 - 当前 SHA256: {}\n\
                 - 预期 SHA256: {}",
                item.path, current_hash, item.old_sha256
            );
        }

        let backup_path = create_backup(&target_path)?;
        let backup_name = backup_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".to_string());
        println!("[变更] {}", item.path);
        println!("  已备份到: {backup_name}");

        run_hpatchz(&backup_path, &patch_file, &target_path)?;

        let new_hash = sha256_of_file(&target_path)?;
        if new_hash != item.new_sha256 {
            std::fs::copy(&backup_path, &target_path)?;
            anyhow::bail!(
                "错误: 补丁应用后校验失败: {}\n已自动恢复原始文件。",
                item.path
            );
        }
    }

    for item in added {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let source_file = resolve_safe_path(&patch_dir, &item.file)?;
        println!("[新增] {}", item.path);
        copy_file(&source_file, &target_path)?;

        let new_hash = sha256_of_file(&target_path)?;
        if new_hash != item.new_sha256 {
            anyhow::bail!("错误: 新增文件校验失败: {}", item.path);
        }
    }

    for item in deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        if target_path.exists() {
            let backup_path = create_backup(&target_path)?;
            let backup_name = backup_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("[删除] {}", item.path);
            println!("  已备份到: {backup_name}");
            std::fs::remove_file(&target_path)?;
        }
    }

    // Move backup files from directories that will be deleted into Patch/.backup_staging/
    // so those directories can be safely removed while preserving rollback capability
    let backup_staging = patch_dir.join(".backup_staging");
    for dir_path in &manifest.deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if target_dir.exists() && target_dir.is_dir() {
            for entry in walkdir::WalkDir::new(&target_dir).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    let fname = entry.file_name().to_string_lossy();
                    if fname.contains(".backup_before_patch") {
                        let rel = entry.path().strip_prefix(&target_dir)
                            .expect("entry should be under target_dir");
                        let staging_path = backup_staging.join(dir_path).join(rel);
                        std::fs::create_dir_all(staging_path.parent().unwrap())?;
                        std::fs::rename(entry.path(), &staging_path)?;
                    }
                }
            }
        }
    }

    // Remove directories that exist in Old but not in New (already sorted deepest-first)
    for dir_path in &manifest.deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if target_dir.exists() && target_dir.is_dir() {
            std::fs::remove_dir(&target_dir)?;
            println!("[删除目录] {dir_path}");
        }
    }

    println!("\n整包补丁应用完成！");
    println!("如果需要回滚，请使用同目录下的 rollback_patch 恢复。");

    Ok(())
}

pub fn apply_single_patch(old_file: &str, patch_file: &str, output_file: &str) -> anyhow::Result<()> {
    let old_path = std::path::Path::new(old_file);
    let patch_path = std::path::Path::new(patch_file);
    let output_path = std::path::Path::new(output_file);

    println!("正在读取旧文件: {old_file}");
    println!("正在读取补丁文件: {patch_file}");

    ensure_parent_dir(output_path)?;
    println!("正在调用 HDiffPatch 应用补丁...");
    run_hpatchz(old_path, patch_path, output_path)?;

    let output_size = std::fs::metadata(output_path)?.len();

    println!("{}", "-".repeat(30));
    println!("补丁应用成功！");
    println!("  - 输出文件 '{output_file}' 已生成。");
    println!("  - 输出文件大小: {}", crate::utils::format_size(output_size));
    println!("{}", "-".repeat(30));

    Ok(())
}
