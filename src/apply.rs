use std::path::Path;
use crate::ffi;
use crate::hdiffpatch::run_hpatchz;
use crate::manifest::Manifest;
use crate::utils::{sha256_of_bytes, sha256_of_file, ensure_parent_dir, create_backup, write_backup, restore_backup, resolve_safe_path, copy_file, backup_root_dir};

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
    let backup_root = backup_root_dir(&patch_dir);

    let changed = &manifest.changed;
    let added = &manifest.added;
    let deleted = &manifest.deleted;

    println!("检测到补丁内容: 变更 {}，新增 {}，删除 {}", changed.len(), added.len(), deleted.len());

    let total = changed.len();
    for (idx, item) in changed.iter().enumerate() {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let patch_file = resolve_safe_path(&patch_dir, &item.patch_file)?;

        if !target_path.exists() {
            eprintln!("错误: 缺少需要打补丁的旧文件: {}", target_path.display());
            eprintln!("提示: 已成功处理 {idx}/{total} 个文件，已处理文件不会自动回滚。");
            eprintln!("      如需恢复，请使用 rollback_patch 或从备份文件手动恢复。");
            anyhow::bail!("已处理 {idx}/{total} 个文件后失败，缺少文件")
        }

        // 一次读取，用于 SHA256 校验和内存补丁
        let old_data = std::fs::read(&target_path)
            .map_err(|e| anyhow::anyhow!("读取旧文件失败 {}: {e}", target_path.display()))?;

        let current_hash = sha256_of_bytes(&old_data);
        if current_hash != item.old_sha256 {
            eprintln!("错误: 文件校验不匹配，无法应用补丁: {}", item.path);
            eprintln!("  - 当前 SHA256: {}", current_hash);
            eprintln!("  - 预期 SHA256: {}", item.old_sha256);
            eprintln!("提示: 已成功处理 {idx}/{total} 个文件，已处理文件不会自动回滚。");
            anyhow::bail!("SHA256 校验失败，中断于第 {}/{} 个文件", idx, total)
        }

        let backup_path = write_backup(&old_data, &target_path, base_dir, &backup_root)?;
        let backup_name = backup_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".to_string());
        println!("[变更] {}", item.path);
        println!("  已备份到: {backup_name}");

        let patch_data = std::fs::read(&patch_file)
            .map_err(|e| anyhow::anyhow!("读取补丁文件失败 {}: {e}", patch_file.display()))?;

        let thread_count = crate::hdiffpatch::get_recommended_thread_count();

        let new_data = match crate::hdiffpatch::apply_patch_with_retry(&old_data, &patch_data, thread_count) {
            Ok(data) => data,
            Err(e) if e.is_oom() => {
                eprintln!("注意: 内存不足，自动切换为流式模式");
                ffi::apply_patch_file(
                    &backup_path.to_string_lossy(),
                    &patch_data,
                    &target_path.to_string_lossy(),
                    thread_count,
                )?;
                // 流式模式已直接写出到 target_path，读回校验
                let new_bytes = match std::fs::read(&target_path) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("错误: 流式补丁写入后读取校验失败: {}", target_path.display());
                        eprintln!("       IO 错误: {e} (可能是磁盘空间不足)");
                        if let Err(be) = restore_backup(&target_path, base_dir, &backup_root) {
                            anyhow::bail!("自动恢复也失败: {be} — 文件可能已损坏")
                        }
                        anyhow::bail!("已自动恢复原始文件，请检查磁盘空间。")
                    }
                };
                let new_hash = sha256_of_bytes(&new_bytes);
                if new_hash != item.new_sha256 {
                    if let Err(be) = restore_backup(&target_path, base_dir, &backup_root) {
                        anyhow::bail!(
                            "错误: 补丁应用后校验失败: {}\n自动恢复也失败: {}\n文件可能已损坏: {}",
                            item.path, be, target_path.display()
                        );
                    }
                    anyhow::bail!(
                        "错误: 补丁应用后校验失败: {}\n已自动恢复原始文件。",
                        item.path
                    );
                }
                continue;
            }
            Err(e) => {
                anyhow::bail!("应用补丁失败: {e}");
            }
        };

        std::fs::write(&target_path, &new_data)
            .map_err(|e| anyhow::anyhow!("写入输出文件失败 {}: {e}", target_path.display()))?;

        let new_hash = sha256_of_bytes(&new_data);
        if new_hash != item.new_sha256 {
            if let Err(be) = restore_backup(&target_path, base_dir, &backup_root) {
                anyhow::bail!(
                    "错误: 补丁应用后校验失败: {}\n自动恢复也失败: {}\n文件可能已损坏: {}",
                    item.path, be, target_path.display()
                );
            }
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
        if target_path.exists() {
            let backup_name = create_backup(&target_path, base_dir, &backup_root)?
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("  目标已存在，已备份到: {backup_name}");
        }
        copy_file(&source_file, &target_path)?;

        let new_hash = sha256_of_file(&target_path)?;
        if new_hash != item.new_sha256 {
            anyhow::bail!("错误: 新增文件校验失败: {}", item.path);
        }
    }

    for item in deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        if target_path.exists() {
            let backup_path = create_backup(&target_path, base_dir, &backup_root)?;
            let backup_name = backup_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("[删除] {}", item.path);
            println!("  已备份到: {backup_name}");
            std::fs::remove_file(&target_path)?;
        }
    }

    // Remove directories that exist in Old but not in New (already sorted deepest-first)
    for dir_path in &manifest.deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if target_dir.exists() && target_dir.is_dir() {
            std::fs::remove_dir_all(&target_dir)?;
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
