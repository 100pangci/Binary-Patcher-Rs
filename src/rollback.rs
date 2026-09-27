use crate::backup::{checked_backup_root_dir, find_backup, restore_backup_copy};
use crate::fs::cleanup_empty_dirs;
use crate::hash::sha256_of_file;
use crate::manifest::Manifest;
use crate::path::{display_path, resolve_safe_path};
use crate::t;
use std::io::Write;
use std::path::Path;

pub fn rollback_bundle(base_dir: &Path) -> anyhow::Result<()> {
    rollback_bundle_at(base_dir, &base_dir.join("Patch"))
}

pub fn rollback_bundle_at(base_dir: &Path, patch_dir: &Path) -> anyhow::Result<()> {
    if !patch_dir.exists() {
        anyhow::bail!("{}", t!("rollback.no-patch-dir", patch_dir.display()));
    }
    crate::patch::validate_patch_dir(base_dir, patch_dir)?;

    let manifest = Manifest::load(patch_dir)?;
    // 与 apply 同一防护：manifest 路径不得指向补丁目录内部。
    // 必须在 journal 恢复或任何文件修改之前完成校验。
    crate::apply::ensure_manifest_paths_outside_patch_dir(base_dir, patch_dir, &manifest)?;
    let backup_root = checked_backup_root_dir(patch_dir)?;

    // 统一保护规则（适用于所有 patch，而非仅 mapping）：
    // - 存在 journal：apply 中途崩溃，按 journal 做精准恢复；
    // - 无 journal 且无有效应用标记：从未 Apply 过（或标记无效），
    //   按完整 manifest 回滚会误删/误改用户的现有文件，直接拒绝且不修改任何文件。
    let journal_path = resolve_safe_path(patch_dir, crate::apply::JOURNAL_FILE_NAME)?;
    crate::path::ensure_no_symlink_components(&journal_path)?;
    if journal_path.exists() {
        crate::apply::rollback_from_journal(base_dir, patch_dir)?;
        // apply 已完成、marker 已写入但 journal 尚未删除时崩溃：
        // 恢复成功后一并清除残留 marker，否则会阻止再次 apply。
        // marker 不存在时安全忽略。
        crate::patch::remove_applied_marker(patch_dir)?;
        return Ok(());
    }
    if crate::patch::load_applied_marker(patch_dir)?.is_none() {
        anyhow::bail!("{}", t!("rollback.not-applied", patch_dir.display()));
    }

    // 完整 preflight：所有「apply 结束时应有的状态」或「该条目已完成
    // rollback 的状态」都必须成立才允许回滚；这样中途失败的 rollback
    // 可以再次运行继续完成。一旦任何文件被用户改动，直接拒绝且不修改任何文件。
    preflight_rollback_state(base_dir, &manifest, &backup_root)?;

    let changed = &manifest.changed;
    let added = &manifest.added;
    let deleted = &manifest.deleted;

    println!(
        "{}",
        t!(
            "rollback.summary",
            changed.len(),
            added.len(),
            deleted.len()
        )
    );

    let mut restored_count = 0u32;
    let mut removed_count = 0u32;

    let mut deleted_dirs = manifest.deleted_dirs.clone();
    deleted_dirs.sort();
    for dir_path in &deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if !target_dir.exists() {
            std::fs::create_dir_all(&target_dir)?;
            println!("{}", t!("rollback.recreate-dir", dir_path));
        }
    }

    for item in changed {
        if item.is_renamed() {
            let target_path = resolve_safe_path(base_dir, &item.path)?;
            println!(
                "{}",
                t!(
                    "rollback.restore-renamed",
                    item.path,
                    item.old_relative_path()
                )
            );

            // 映射补丁不修改源文件，回滚只需撤销 target：
            // 1. 删除补丁生成的 target（应用前 target 不存在时的产物）。
            if target_path.exists() {
                std::fs::remove_file(&target_path)?;
                removed_count += 1;
                println!("{}", t!("rollback.removed-file", target_path.display()));
            }

            // 2. 如果应用前 target 已存在，恢复其原始备份。
            // 复制恢复并保留备份：中途失败后重跑仍可通过备份校验继续完成。
            if restore_backup_copy(&target_path, base_dir, &backup_root)? {
                restored_count += 1;
            }

            // 3. delete_source=true 时源文件在应用成功后已被删除，从备份恢复。
            if item.delete_source {
                let source_path = resolve_safe_path(base_dir, item.old_relative_path())?;
                if restore_backup_copy(&source_path, base_dir, &backup_root)? {
                    restored_count += 1;
                } else {
                    println!("{}", t!("rollback.skip-no-backup"));
                }
            }

            // 4. 清理因删除 target 而变空的目录。
            if let Some(parent) = target_path.parent() {
                for dir in cleanup_empty_dirs(parent, base_dir)? {
                    println!(
                        "{}",
                        t!("rollback.removed-empty-dir", display_path(&dir, base_dir))
                    );
                }
            }
        } else {
            let target_path = resolve_safe_path(base_dir, &item.path)?;
            println!("{}", t!("rollback.restore-changed", item.path));
            if restore_backup_copy(&target_path, base_dir, &backup_root)? {
                restored_count += 1;
            } else {
                println!("{}", t!("rollback.skip-no-backup"));
            }
        }
    }

    for item in deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("{}", t!("rollback.restore-deleted", item.path));
        if restore_backup_copy(&target_path, base_dir, &backup_root)? {
            restored_count += 1;
        } else {
            println!("{}", t!("rollback.skip-no-backup"));
        }
    }

    for item in added {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("{}", t!("rollback.remove-added", item.path));
        if target_path.exists() {
            if target_path.is_file() {
                std::fs::remove_file(&target_path)?;
                removed_count += 1;
                println!("{}", t!("rollback.removed-file", target_path.display()));
            } else if target_path.is_dir() {
                // added 文件后来变成目录（可能是用户数据）：绝不递归删除。
                anyhow::bail!("{}", t!("rollback.added-is-dir", item.path));
            }
        }
        // 应用前已存在同名文件（added 覆盖用户文件）时必须恢复原文件；
        // 已恢复完成的条目重复恢复也是幂等的（复制恢复，备份保留）。
        if restore_backup_copy(&target_path, base_dir, &backup_root)? {
            restored_count += 1;
        } else if !target_path.exists() {
            println!("{}", t!("rollback.skip-not-exists", target_path.display()));
        }
        if let Some(parent) = target_path.parent() {
            for dir in cleanup_empty_dirs(parent, base_dir)? {
                println!(
                    "{}",
                    t!("rollback.removed-empty-dir", display_path(&dir, base_dir))
                );
            }
        }
    }

    println!("{}", t!("rollback.complete"));
    println!("{}", t!("rollback.restored-count", restored_count));
    println!("{}", t!("rollback.removed-count", removed_count));

    if backup_root.exists() {
        let is_terminal = std::io::IsTerminal::is_terminal(&std::io::stdin());
        let should_clean = if is_terminal {
            print!("{}", t!("rollback.cleanup-prompt", backup_root.display()));
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            input.trim().eq_ignore_ascii_case("y")
        } else {
            true
        };
        if should_clean {
            if !backup_root.starts_with(patch_dir) {
                anyhow::bail!("{}", t!("rollback.path-unsafe", backup_root.display()));
            }
            std::fs::remove_dir_all(&backup_root)?;
            println!("{}", t!("rollback.cleanup-done"));
        } else {
            println!("{}", t!("rollback.cleanup-skipped"));
        }
    }

    let staging_dir = resolve_safe_path(patch_dir, ".backup_staging")?;
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)?;
    }

    let journal_path = resolve_safe_path(patch_dir, crate::apply::JOURNAL_FILE_NAME)?;
    if journal_path.exists() {
        std::fs::remove_file(&journal_path)?;
        println!("{}", t!("rollback.journal-removed"));
    }

    crate::patch::remove_applied_marker(patch_dir)?;

    Ok(())
}

/// rollback 前状态校验（只读）：
///
/// 每个条目都必须处于「已 Apply」或「该条目已完成 rollback」两种状态之一，
/// 这样中途失败的 rollback 可以再次运行继续完成，而不会破坏现有状态：
///
/// - 路径不存在：允许（`added`/映射 target 已删除，或 `deleted` 已恢复为删除态）；
/// - 普通文件且 SHA256 命中「apply 结果或 rollback 后应有内容」：允许；
/// - 普通文件且内容与其最新备份一致：允许（上一轮 rollback 已恢复、备份保留）。
///
/// 目录、符号链接与用户改动后的内容一律拒绝；调用方不得在失败前修改文件。
fn preflight_rollback_state(
    base_dir: &Path,
    manifest: &Manifest,
    backup_root: &Path,
) -> anyhow::Result<()> {
    for item in &manifest.changed {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        // 普通 changed 的 target 可处于 apply 结果（new）或已恢复（old）；
        // 映射 target 的 old_sha256 是 source 文件的哈希，不是 target 的合法
        // 状态，只能允许 new、不存在、或与 target 自身备份一致。
        let allowed: &[&str] = if item.is_renamed() {
            &[item.new_sha256.as_str()]
        } else {
            &[item.new_sha256.as_str(), item.old_sha256.as_str()]
        };
        if !rollback_state_allowed(&target_path, allowed, base_dir, backup_root)? {
            anyhow::bail!("{}", t!("rollback.preflight-changed", item.path));
        }
        if item.is_renamed() && item.delete_source {
            let source_path = resolve_safe_path(base_dir, item.old_relative_path())?;
            let allowed = [item.old_sha256.as_str()];
            if !rollback_state_allowed(&source_path, &allowed, base_dir, backup_root)? {
                anyhow::bail!(
                    "{}",
                    t!("rollback.preflight-source", item.old_relative_path())
                );
            }
        }
    }

    for item in &manifest.added {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let allowed = [item.new_sha256.as_str()];
        if !rollback_state_allowed(&target_path, &allowed, base_dir, backup_root)? {
            anyhow::bail!("{}", t!("rollback.preflight-added", item.path));
        }
    }

    for item in &manifest.deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let allowed = [item.old_sha256.as_str()];
        if !rollback_state_allowed(&target_path, &allowed, base_dir, backup_root)? {
            anyhow::bail!("{}", t!("rollback.preflight-deleted", item.path));
        }
    }

    Ok(())
}

/// 判断路径是否处于可安全回滚的状态，见 [`preflight_rollback_state`]。
fn rollback_state_allowed(
    target_path: &Path,
    applied_or_restored_hashes: &[&str],
    base_dir: &Path,
    backup_root: &Path,
) -> anyhow::Result<bool> {
    let metadata = match std::fs::symlink_metadata(target_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Ok(false);
    }

    let current_hash = sha256_of_file(target_path)?;
    if applied_or_restored_hashes.contains(&current_hash.as_str()) {
        return Ok(true);
    }

    if let Some(backup_path) = find_backup(target_path, base_dir, backup_root)? {
        return Ok(sha256_of_file(&backup_path)? == current_hash);
    }
    Ok(false)
}
