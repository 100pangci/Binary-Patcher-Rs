use crate::backup::{backup_root_dir, restore_backup};
use crate::fs::cleanup_empty_dirs;
use crate::manifest::Manifest;
use crate::path::{display_path, resolve_safe_path};
use crate::t;
use std::path::Path;

pub fn rollback_bundle(base_dir: &Path) -> anyhow::Result<()> {
    let patch_dir = base_dir.join("Patch");

    if !patch_dir.exists() {
        anyhow::bail!("{}", t!("rollback.no-patch-dir", patch_dir.display()));
    }

    let manifest = Manifest::load(&patch_dir)?;
    let backup_root = backup_root_dir(&patch_dir);

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
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        println!("{}", t!("rollback.restore-changed", item.path));
        if restore_backup(&target_path, base_dir, &backup_root)? {
            restored_count += 1;
        } else {
            println!("{}", t!("rollback.skip-no-backup"));
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
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            input.trim().eq_ignore_ascii_case("y")
        } else {
            true
        };
        if should_clean {
            if !backup_root.starts_with(&patch_dir) {
                anyhow::bail!("{}", t!("rollback.path-unsafe", backup_root.display()));
            }
            std::fs::remove_dir_all(&backup_root)?;
            println!("{}", t!("rollback.cleanup-done"));
        } else {
            println!("{}", t!("rollback.cleanup-skipped"));
        }
    }

    let staging_dir = patch_dir.join(".backup_staging");
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)?;
    }

    Ok(())
}
