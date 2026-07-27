use crate::backup::{backup_root_dir, create_backup, restore_backup, write_backup};
use crate::fs::{cleanup_empty_dirs, copy_file};
use crate::hash::{sha256_of_bytes, sha256_of_file};
use crate::hdiffpatch::{apply_patch_auto, run_hpatchz};
use crate::manifest::Manifest;
use crate::path::{ensure_parent_dir, resolve_safe_path};
use crate::t;
use std::path::{Path, PathBuf};

enum JournalEntry {
    Patched { target: PathBuf },
    Added { target: PathBuf, had_backup: bool },
    Deleted { target: PathBuf },
    DeletedDir { target: PathBuf },
}

struct ChangeJournal {
    entries: Vec<JournalEntry>,
    base_dir: PathBuf,
    backup_root: PathBuf,
}

impl ChangeJournal {
    fn new(base_dir: &Path, backup_root: &Path) -> Self {
        ChangeJournal {
            entries: Vec::new(),
            base_dir: base_dir.to_path_buf(),
            backup_root: backup_root.to_path_buf(),
        }
    }

    fn push(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
    }

    fn rollback(&self) {
        for entry in self.entries.iter().rev() {
            match entry {
                JournalEntry::Patched { target }
                | JournalEntry::Deleted { target } => {
                    if let Err(e) = restore_backup(target, &self.base_dir, &self.backup_root) {
                        eprintln!("  [rollback] {}: {e}", target.display());
                    } else {
                        println!("  [rollback] restored: {}", target.display());
                    }
                }
                JournalEntry::Added {
                    target,
                    had_backup: true,
                } => {
                    if let Err(e) = restore_backup(target, &self.base_dir, &self.backup_root) {
                        eprintln!("  [rollback] {}: {e}", target.display());
                    } else {
                        println!("  [rollback] restored: {}", target.display());
                    }
                }
                JournalEntry::Added {
                    target,
                    had_backup: false,
                } => {
                    if target.exists() {
                        if let Err(e) = std::fs::remove_file(target) {
                            eprintln!("  [rollback] remove {}: {e}", target.display());
                        } else {
                            println!("  [rollback] removed: {}", target.display());
                        }
                    }
                    if let Some(parent) = target.parent() {
                        let _ = cleanup_empty_dirs(parent, &self.base_dir);
                    }
                }
                JournalEntry::DeletedDir { target } => {
                    if !target.exists() {
                        if let Err(e) = std::fs::create_dir_all(target) {
                            eprintln!("  [rollback] recreate dir {}: {e}", target.display());
                        } else {
                            println!("  [rollback] recreated dir: {}", target.display());
                        }
                    }
                }
            }
        }
        println!("  [rollback] All changes have been undone.");
    }
}

pub fn apply_bundle(base_dir: &Path) -> anyhow::Result<()> {
    let patch_dir = base_dir.join("Patch");

    if !patch_dir.exists() {
        anyhow::bail!("{}", t!("apply.no-patch-dir", patch_dir.display()));
    }

    let manifest = Manifest::load(&patch_dir)?;
    let backup_root = backup_root_dir(&patch_dir);
    let mut journal = ChangeJournal::new(base_dir, &backup_root);

    check_version_compat_or_prompt(&manifest)?;
    print_apply_summary(&manifest);

    let result = (|| -> anyhow::Result<()> {
        apply_changed_files(base_dir, &patch_dir, &manifest, &mut journal)?;
        apply_added_files(base_dir, &patch_dir, &manifest, &mut journal)?;
        apply_deleted_files(base_dir, &manifest, &mut journal)?;
        remove_deleted_dirs(base_dir, &manifest, &mut journal)?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            println!("{}", t!("apply.complete"));
            println!("{}", t!("apply.rollback-hint"));
            Ok(())
        }
        Err(e) => {
            eprintln!("\n{}", t!("apply.rollback-triggered"));
            journal.rollback();
            Err(e)
        }
    }
}

fn check_version_compat_or_prompt(manifest: &Manifest) -> anyhow::Result<()> {
    match crate::manifest::check_version_compat(&manifest.format) {
        crate::manifest::VersionCompat::Compatible => Ok(()),
        crate::manifest::VersionCompat::Incompatible {
            manifest: mver,
            tool: tver,
        } => {
            eprintln!("{}", t!("apply.version-warning", mver));
            eprintln!("{}", t!("apply.version-warning2", tver));
            print!("{}", t!("apply.version-prompt"));
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if !input.trim().eq_ignore_ascii_case("y") {
                anyhow::bail!("{}", t!("apply.version-cancelled"));
            }
            Ok(())
        }
    }
}

fn print_apply_summary(manifest: &Manifest) {
    println!(
        "{}",
        t!(
            "apply.summary",
            manifest.changed.len(),
            manifest.added.len(),
            manifest.deleted.len()
        )
    );
}

fn apply_changed_files(
    base_dir: &Path,
    patch_dir: &Path,
    manifest: &Manifest,
    journal: &mut ChangeJournal,
) -> anyhow::Result<()> {
    let total = manifest.changed.len();
    for (idx, item) in manifest.changed.iter().enumerate() {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let patch_file = resolve_safe_path(patch_dir, &item.patch_file)?;

        if !target_path.exists() {
            eprintln!("{}", t!("apply.missing-old", target_path.display()));
            eprintln!("{}", t!("apply.missing-hint", idx, total));
            eprintln!("{}", t!("apply.missing-hint-restore"));
            anyhow::bail!("{}", t!("apply.missing-bail", idx + 1, total, item.path))
        }

        let old_data = std::fs::read(&target_path).map_err(|e| {
            anyhow::anyhow!(
                "{} {}",
                t!("bundle.failed-read-old", target_path.display()),
                e
            )
        })?;

        let current_hash = sha256_of_bytes(&old_data);
        if current_hash != item.old_sha256 {
            eprintln!("{}", t!("apply.sha256-mismatch", item.path));
            eprintln!("{}", t!("apply.sha256-current", current_hash));
            eprintln!("{}", t!("apply.sha256-expected", item.old_sha256));
            eprintln!("{}", t!("apply.missing-hint", idx, total));
            anyhow::bail!("{}", t!("apply.sha256-bail", idx + 1, total))
        }

        let backup_path = write_backup(&old_data, &target_path, base_dir, &journal.backup_root)?;
        let backup_name = backup_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".to_string());
        println!("{}", t!("apply.changed", item.path));
        println!("{}", t!("apply.backed-up", backup_name));

        let patch_data = std::fs::read(&patch_file)
            .map_err(|e| anyhow::anyhow!("{}: {}", patch_file.display(), e))?;

        let thread_count = crate::hdiffpatch::get_recommended_thread_count();

        let new_data = apply_patch_auto(
            &old_data,
            &backup_path,
            &patch_data,
            &target_path,
            thread_count,
        )?;

        let new_hash = sha256_of_bytes(&new_data);
        if new_hash != item.new_sha256 {
            if let Err(be) = restore_backup(&target_path, base_dir, &journal.backup_root) {
                anyhow::bail!(
                    "{}",
                    t!("apply.sha256-fail-restore", item.path, be, target_path.display())
                );
            }
            anyhow::bail!("{}", t!("apply.sha256-fail-auto-restore", item.path));
        }

        journal.push(JournalEntry::Patched {
            target: target_path,
        });
    }
    Ok(())
}

fn apply_added_files(
    base_dir: &Path,
    patch_dir: &Path,
    manifest: &Manifest,
    journal: &mut ChangeJournal,
) -> anyhow::Result<()> {
    for item in &manifest.added {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        let source_file = resolve_safe_path(patch_dir, &item.file)?;
        println!("{}", t!("apply.added", item.path));

        let had_backup = if target_path.exists() {
            let backup_name = create_backup(&target_path, base_dir, &journal.backup_root)?
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("{}", t!("apply.target-exists-backup", backup_name));
            true
        } else {
            false
        };

        copy_file(&source_file, &target_path)?;

        let new_hash = sha256_of_file(&target_path)?;
        if new_hash != item.new_sha256 {
            anyhow::bail!("{}", t!("apply.added-verify-fail", item.path));
        }

        journal.push(JournalEntry::Added {
            target: target_path,
            had_backup,
        });
    }
    Ok(())
}

fn apply_deleted_files(
    base_dir: &Path,
    manifest: &Manifest,
    journal: &mut ChangeJournal,
) -> anyhow::Result<()> {
    for item in &manifest.deleted {
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        if target_path.exists() {
            let backup_path = create_backup(&target_path, base_dir, &journal.backup_root)?;
            let backup_name = backup_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());
            println!("{}", t!("apply.deleted", item.path));
            println!("{}", t!("apply.backed-up", backup_name));
            std::fs::remove_file(&target_path)?;
            journal.push(JournalEntry::Deleted {
                target: target_path,
            });
        }
    }
    Ok(())
}

fn remove_deleted_dirs(
    base_dir: &Path,
    manifest: &Manifest,
    journal: &mut ChangeJournal,
) -> anyhow::Result<()> {
    for dir_path in &manifest.deleted_dirs {
        let target_dir = resolve_safe_path(base_dir, dir_path)?;
        if target_dir.exists() && target_dir.is_dir() {
            std::fs::remove_dir_all(&target_dir)?;
            println!("{}", t!("apply.deleted-dir", dir_path));
            journal.push(JournalEntry::DeletedDir { target: target_dir });
        }
    }
    Ok(())
}

pub fn apply_single_patch(
    old_file: &str,
    patch_file: &str,
    output_file: &str,
) -> anyhow::Result<()> {
    let old_path = std::path::Path::new(old_file);
    let patch_path = std::path::Path::new(patch_file);
    let output_path = std::path::Path::new(output_file);

    println!("{}", t!("main.reading-old", old_file));
    println!("{}", t!("bp.reading-patch", patch_file));

    ensure_parent_dir(output_path)?;
    println!("{}", t!("main.calling-hdiff"));
    run_hpatchz(old_path, patch_path, output_path)?;

    let output_size = std::fs::metadata(output_path)?.len();

    println!("{}", "-".repeat(30));
    println!("{}", t!("main.patch-created"));
    println!(
        "  - {} '{}'",
        t!("apply.output-generated"),
        output_file
    );
    println!(
        "  - {}: {}",
        t!("main.patch-size"),
        crate::fmt::format_size(output_size)
    );
    println!("{}", "-".repeat(30));

    Ok(())
}
