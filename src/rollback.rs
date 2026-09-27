use crate::backup::{checked_backup_root_dir, restore_backup};
use crate::fs::cleanup_empty_dirs;
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
            if restore_backup(&target_path, base_dir, &backup_root)? {
                restored_count += 1;
            }

            // 3. delete_source=true 时源文件在应用成功后已被删除，从备份恢复。
            if item.delete_source {
                let source_path = resolve_safe_path(base_dir, item.old_relative_path())?;
                if restore_backup(&source_path, base_dir, &backup_root)? {
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
            if restore_backup(&target_path, base_dir, &backup_root)? {
                restored_count += 1;
            } else {
                println!("{}", t!("rollback.skip-no-backup"));
            }
        }
    }

    for item in deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("{}", t!("rollback.restore-deleted", item.path));
        if restore_backup(&target_path, base_dir, &backup_root)? {
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
                if let Some(parent) = target_path.parent() {
                    for dir in cleanup_empty_dirs(parent, base_dir)? {
                        println!(
                            "{}",
                            t!("rollback.removed-empty-dir", display_path(&dir, base_dir))
                        );
                    }
                }
            } else if target_path.is_dir() {
                if target_path.read_dir()?.next().is_none() {
                    std::fs::remove_dir(&target_path)?;
                    removed_count += 1;
                    println!(
                        "{}",
                        t!("rollback.removed-empty-dir", target_path.display())
                    );
                    if let Some(parent) = target_path.parent() {
                        for dir in cleanup_empty_dirs(parent, base_dir)? {
                            println!(
                                "{}",
                                t!("rollback.removed-empty-dir", display_path(&dir, base_dir))
                            );
                        }
                    }
                } else {
                    std::fs::remove_dir_all(&target_path)?;
                    removed_count += 1;
                    println!("{}", t!("rollback.removed-dir", target_path.display()));
                }
            }
        } else {
            println!("{}", t!("rollback.skip-not-exists", target_path.display()));
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
