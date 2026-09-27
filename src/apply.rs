use crate::backup::{
    backup_root_dir, checked_backup_root_dir, create_backup, restore_backup, write_backup,
};
use crate::fs::{cleanup_empty_dirs, copy_file};
use crate::hash::{sha256_of_bytes, sha256_of_file};
use crate::hdiffpatch::{apply_patch_auto, run_hpatchz};
use crate::manifest::{ChangedEntry, Manifest};
use crate::path::{ensure_parent_dir, resolve_safe_path};
use crate::t;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 持久化的应用日志文件名，位于 Patch 目录内。
/// 用于崩溃恢复：应用中途被杀（断电/进程终止）后，再次运行 apply_patch
/// 会检测到该文件并提示回滚上次未完成的更改。
pub const JOURNAL_FILE_NAME: &str = ".apply_journal.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum JournalEntrySer {
    Patched {
        path: String,
    },
    Added {
        path: String,
        had_backup: bool,
    },
    Deleted {
        path: String,
    },
    DeletedDir {
        path: String,
    },
    /// 映射补丁：`source -> target` 的路径迁移。
    RenamedPatched {
        source: String,
        target: String,
        target_had_backup: bool,
        #[serde(default)]
        delete_source: bool,
    },
}

#[derive(Debug)]
enum JournalEntry {
    Patched {
        target: PathBuf,
    },
    Added {
        target: PathBuf,
        had_backup: bool,
    },
    Deleted {
        target: PathBuf,
    },
    DeletedDir {
        target: PathBuf,
    },
    RenamedPatched {
        source: PathBuf,
        target: PathBuf,
        target_had_backup: bool,
        delete_source: bool,
    },
}

struct ChangeJournal {
    entries: Vec<JournalEntry>,
    base_dir: PathBuf,
    backup_root: PathBuf,
    journal_path: PathBuf,
}

impl ChangeJournal {
    fn new(base_dir: &Path, backup_root: &Path, journal_path: &Path) -> Self {
        ChangeJournal {
            entries: Vec::new(),
            base_dir: base_dir.to_path_buf(),
            backup_root: backup_root.to_path_buf(),
            journal_path: journal_path.to_path_buf(),
        }
    }

    fn push(&mut self, entry: JournalEntry) -> anyhow::Result<()> {
        self.entries.push(entry);
        self.persist()
    }

    /// 将日志原子写入磁盘（先写临时文件再 rename）。
    /// 崩溃时最多丢失一条尚未落盘的记录，已落盘的记录保证可恢复。
    fn persist(&self) -> anyhow::Result<()> {
        let ser_entries: Vec<JournalEntrySer> = self
            .entries
            .iter()
            .map(|e| match e {
                JournalEntry::Patched { target } => JournalEntrySer::Patched {
                    path: rel_path_of(&self.base_dir, target),
                },
                JournalEntry::Added { target, had_backup } => JournalEntrySer::Added {
                    path: rel_path_of(&self.base_dir, target),
                    had_backup: *had_backup,
                },
                JournalEntry::Deleted { target } => JournalEntrySer::Deleted {
                    path: rel_path_of(&self.base_dir, target),
                },
                JournalEntry::DeletedDir { target } => JournalEntrySer::DeletedDir {
                    path: rel_path_of(&self.base_dir, target),
                },
                JournalEntry::RenamedPatched {
                    source,
                    target,
                    target_had_backup,
                    delete_source,
                } => JournalEntrySer::RenamedPatched {
                    source: rel_path_of(&self.base_dir, source),
                    target: rel_path_of(&self.base_dir, target),
                    target_had_backup: *target_had_backup,
                    delete_source: *delete_source,
                },
            })
            .collect();

        let tmp_path = self.journal_path.with_extension("json.tmp");
        crate::path::ensure_no_symlink_components(&self.journal_path)?;
        crate::path::ensure_no_symlink_components(&tmp_path)?;
        std::fs::write(&tmp_path, serde_json::to_string(&ser_entries)?)?;
        std::fs::rename(&tmp_path, &self.journal_path)?;
        Ok(())
    }

    fn rollback(&self) {
        for entry in self.entries.iter().rev() {
            match entry {
                JournalEntry::Patched { target }
                | JournalEntry::Deleted { target }
                | JournalEntry::Added {
                    target,
                    had_backup: true,
                } => {
                    if let Err(e) = restore_backup(target, &self.base_dir, &self.backup_root) {
                        eprintln!("{}", t!("journal.error", target.display(), e));
                    } else {
                        println!("{}", t!("journal.restored", target.display()));
                    }
                }
                JournalEntry::Added {
                    target,
                    had_backup: false,
                } => {
                    if target.exists() {
                        if let Err(e) = std::fs::remove_file(target) {
                            eprintln!("{}", t!("journal.error", target.display(), e));
                        } else {
                            println!("{}", t!("journal.removed", target.display()));
                        }
                    }
                    if let Some(parent) = target.parent() {
                        let _ = cleanup_empty_dirs(parent, &self.base_dir);
                    }
                }
                JournalEntry::DeletedDir { target } => {
                    if !target.exists() {
                        if let Err(e) = std::fs::create_dir_all(target) {
                            eprintln!("{}", t!("journal.error", target.display(), e));
                        } else {
                            println!("{}", t!("journal.recreated-dir", target.display()));
                        }
                    }
                }
                JournalEntry::RenamedPatched {
                    source,
                    target,
                    target_had_backup,
                    delete_source,
                } => {
                    // target_had_backup 为 false 时 target 是本次 patch 的产物，直接删除；
                    // 为 true 时 target 在应用前已存在，必须从备份恢复原文件。
                    if *target_had_backup {
                        if let Err(e) = restore_backup(target, &self.base_dir, &self.backup_root) {
                            eprintln!("{}", t!("journal.error", target.display(), e));
                        } else {
                            println!("{}", t!("journal.restored", target.display()));
                        }
                    } else if target.exists() {
                        if let Err(e) = std::fs::remove_file(target) {
                            eprintln!("{}", t!("journal.error", target.display(), e));
                        } else {
                            println!("{}", t!("journal.removed", target.display()));
                        }
                        if let Some(parent) = target.parent() {
                            let _ = cleanup_empty_dirs(parent, &self.base_dir);
                        }
                    }
                    // delete_source=false 时源文件从未改动；true 时已在校验成功后删除，
                    // 必须从删除前落盘的备份恢复。
                    if *delete_source {
                        if let Err(e) = restore_backup(source, &self.base_dir, &self.backup_root) {
                            eprintln!("{}", t!("journal.error", source.display(), e));
                        } else {
                            println!("{}", t!("journal.restored", source.display()));
                        }
                    }
                }
            }
        }
        println!("{}", t!("journal.rollback-complete"));
    }
}

fn rel_path_of(base_dir: &Path, target: &Path) -> String {
    target
        .strip_prefix(base_dir)
        .unwrap_or(target)
        .to_string_lossy()
        .replace('\\', "/")
}

fn load_journal(journal_path: &Path, base_dir: &Path) -> anyhow::Result<Vec<JournalEntry>> {
    let content = std::fs::read_to_string(journal_path)?;
    let ser_entries: Vec<JournalEntrySer> = serde_json::from_str(&content)?;
    ser_entries
        .into_iter()
        .map(|e| match e {
            JournalEntrySer::Patched { path } => Ok(JournalEntry::Patched {
                target: resolve_safe_path(base_dir, &path)?,
            }),
            JournalEntrySer::Added { path, had_backup } => Ok(JournalEntry::Added {
                target: resolve_safe_path(base_dir, &path)?,
                had_backup,
            }),
            JournalEntrySer::Deleted { path } => Ok(JournalEntry::Deleted {
                target: resolve_safe_path(base_dir, &path)?,
            }),
            JournalEntrySer::DeletedDir { path } => Ok(JournalEntry::DeletedDir {
                target: resolve_safe_path(base_dir, &path)?,
            }),
            JournalEntrySer::RenamedPatched {
                source,
                target,
                target_had_backup,
                delete_source,
            } => Ok(JournalEntry::RenamedPatched {
                source: resolve_safe_path(base_dir, &source)?,
                target: resolve_safe_path(base_dir, &target)?,
                target_had_backup,
                delete_source,
            }),
        })
        .collect()
}

fn do_journal_rollback(
    entries: Vec<JournalEntry>,
    base_dir: &Path,
    backup_root: &Path,
    journal_path: &Path,
) {
    let journal = ChangeJournal {
        entries,
        base_dir: base_dir.to_path_buf(),
        backup_root: backup_root.to_path_buf(),
        journal_path: journal_path.to_path_buf(),
    };
    journal.rollback();
    let _ = std::fs::remove_file(journal_path);
}

/// 从磁盘上的应用日志回滚一次未完成的 apply（崩溃恢复）。
/// 供 apply_patch 启动时的中断检测与 rollback_patch 复用。
pub fn rollback_from_journal(base_dir: &Path, patch_dir: &Path) -> anyhow::Result<()> {
    let journal_path = resolve_safe_path(patch_dir, JOURNAL_FILE_NAME)?;
    crate::path::ensure_no_symlink_components(&journal_path)?;
    if !journal_path.exists() {
        return Ok(());
    }
    let entries = load_journal(&journal_path, base_dir)?;
    do_journal_rollback(
        entries,
        base_dir,
        &checked_backup_root_dir(patch_dir)?,
        &journal_path,
    );
    Ok(())
}

fn handle_interrupted_apply(base_dir: &Path, patch_dir: &Path) -> anyhow::Result<()> {
    use std::io::Write;

    let journal_path = resolve_safe_path(patch_dir, JOURNAL_FILE_NAME)?;
    crate::path::ensure_no_symlink_components(&journal_path)?;
    if !journal_path.exists() {
        return Ok(());
    }

    let entries = match load_journal(&journal_path, base_dir) {
        Ok(entries) if entries.is_empty() => {
            let _ = std::fs::remove_file(&journal_path);
            return Ok(());
        }
        Ok(entries) => entries,
        Err(e) => {
            anyhow::bail!(
                "{}",
                t!(
                    "apply.journal-corrupt",
                    e,
                    journal_path.display(),
                    backup_root_dir(patch_dir).display()
                )
            )
        }
    };

    eprintln!("{}", t!("apply.journal-found"));
    loop {
        print!("{}", t!("apply.journal-prompt"));
        std::io::stdout().flush()?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            anyhow::bail!("{}", t!("apply.journal-abort"));
        }
        let answer = input.trim();
        if answer.eq_ignore_ascii_case("a") {
            anyhow::bail!("{}", t!("apply.journal-abort"));
        }
        if answer.is_empty() || answer.eq_ignore_ascii_case("r") {
            break;
        }
    }

    do_journal_rollback(
        entries,
        base_dir,
        &backup_root_dir(patch_dir),
        &journal_path,
    );
    println!("{}", t!("apply.journal-rolled-back"));
    Ok(())
}

pub fn apply_bundle(base_dir: &Path) -> anyhow::Result<()> {
    apply_bundle_at(base_dir, &base_dir.join("Patch"))
}

pub fn apply_bundle_at(base_dir: &Path, patch_dir: &Path) -> anyhow::Result<()> {
    if !patch_dir.exists() {
        anyhow::bail!("{}", t!("apply.no-patch-dir", patch_dir.display()));
    }
    crate::patch::validate_patch_dir(base_dir, patch_dir)?;

    // 已成功应用过的补丁拒绝重复应用：mapping source 保持不变的语义下，
    // 再次 apply 会把第一次生成的目标再次备份，破坏 rollback 语义。
    if let Some(marker) = crate::patch::load_applied_marker(patch_dir)? {
        anyhow::bail!("{}", t!("apply.already-applied", marker.apply_id));
    }

    let manifest = Manifest::load(patch_dir)?;
    let backup_root = checked_backup_root_dir(patch_dir)?;
    let journal_path = resolve_safe_path(patch_dir, JOURNAL_FILE_NAME)?;
    let mut journal = ChangeJournal::new(base_dir, &backup_root, &journal_path);

    // 必须在任何备份 / 写 target / 删 source 之前完成校验。
    validate_mapped_source_targets(base_dir, &manifest)?;
    handle_interrupted_apply(base_dir, patch_dir)?;
    check_version_compat_or_prompt(&manifest)?;
    print_apply_summary(&manifest);

    let result = (|| -> anyhow::Result<crate::patch::AppliedPatchMarker> {
        apply_changed_files(base_dir, patch_dir, &manifest, &mut journal)?;
        apply_added_files(base_dir, patch_dir, &manifest, &mut journal)?;
        apply_deleted_files(base_dir, &manifest, &mut journal)?;
        remove_deleted_dirs(base_dir, &manifest, &mut journal)?;
        let marker = crate::patch::write_applied_marker(base_dir, patch_dir)?;
        Ok(marker)
    })();

    match result {
        Ok(marker) => {
            let _ = std::fs::remove_file(&journal_path);
            println!(
                "{}",
                t!("apply.marker-created", marker.apply_id, marker.patch_dir)
            );
            println!("{}", t!("apply.complete"));
            println!("{}", t!("apply.rollback-hint"));
            Ok(())
        }
        Err(e) => {
            eprintln!("\n{}", t!("apply.rollback-triggered"));
            journal.rollback();
            let _ = std::fs::remove_file(&journal_path);
            Err(e)
        }
    }
}

fn check_version_compat_or_prompt(manifest: &Manifest) -> anyhow::Result<()> {
    use std::io::Write;
    match crate::manifest::check_version_compat(&manifest.format) {
        crate::manifest::VersionCompat::Compatible => Ok(()),
        crate::manifest::VersionCompat::Incompatible {
            manifest: mver,
            tool: tver,
        } => {
            eprintln!("{}", t!("apply.version-warning", mver));
            eprintln!("{}", t!("apply.version-warning2", tver));
            print!("{}", t!("apply.version-prompt"));
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

/// 应用前兜底校验：映射条目的 source 与 target 必须解析到不同文件。
///
/// manifest 校验已处理规范化（`./`、重复分隔符）与 Windows 大小写等价；
/// 这里在修改任何文件之前，对两边都存在的路径再用 `canonicalize` 确认不是
/// 同一物理文件（例如 Windows 尾随点/空格、8.3 短名等别名）。
fn validate_mapped_source_targets(base_dir: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    for item in &manifest.changed {
        if !item.is_renamed() {
            continue;
        }
        let source_path = resolve_safe_path(base_dir, item.old_relative_path())?;
        let target_path = resolve_safe_path(base_dir, &item.path)?;
        if same_existing_file(&source_path, &target_path) {
            anyhow::bail!(
                "{}",
                t!(
                    "apply.source-target-same",
                    item.old_relative_path(),
                    item.path
                )
            );
        }
    }
    Ok(())
}

/// 两个路径是否指向同一物理文件；仅在两边都能 canonicalize 时判定。
fn same_existing_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left_real), Ok(right_real)) => left_real == right_real,
        _ => false,
    }
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
        let source_relative = item.old_relative_path();
        let source_path = resolve_safe_path(base_dir, source_relative)?;
        let renamed = item.is_renamed();
        let patch_file = resolve_safe_path(patch_dir, &item.patch_file)?;

        if !source_path.exists() {
            eprintln!("{}", t!("apply.missing-old", source_path.display()));
            eprintln!("{}", t!("apply.missing-hint", idx, total));
            eprintln!("{}", t!("apply.missing-hint-restore"));
            anyhow::bail!(
                "{}",
                t!("apply.missing-bail", idx + 1, total, source_relative)
            )
        }

        let old_data = std::fs::read(&source_path).map_err(|e| {
            anyhow::anyhow!(
                "{} {}",
                t!("bundle.failed-read-old", source_path.display()),
                e
            )
        })?;

        let current_hash = sha256_of_bytes(&old_data);
        if current_hash != item.old_sha256 {
            eprintln!("{}", t!("apply.sha256-mismatch", source_relative));
            eprintln!("{}", t!("apply.sha256-current", current_hash));
            eprintln!("{}", t!("apply.sha256-expected", item.old_sha256));
            eprintln!("{}", t!("apply.missing-hint", idx, total));
            anyhow::bail!("{}", t!("apply.sha256-bail", idx + 1, total))
        }

        if renamed {
            apply_renamed_change(
                base_dir,
                &patch_file,
                item,
                &source_path,
                &target_path,
                old_data,
                journal,
            )?;
            continue;
        }

        let backup_path = write_backup(&old_data, &source_path, base_dir, &journal.backup_root)?;
        let backup_name = backup_path
            .file_name()
            .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
        println!("{}", t!("apply.changed", item.path));
        println!("{}", t!("apply.backed-up", backup_name));

        journal.push(JournalEntry::Patched {
            target: target_path.clone(),
        })?;

        let patch_data = std::fs::read(&patch_file)
            .map_err(|e| anyhow::anyhow!("{}: {}", patch_file.display(), e))?;

        let thread_count = crate::hdiffpatch::get_recommended_thread_count();

        let new_data = apply_patch_auto(
            old_data,
            &backup_path,
            patch_data,
            &target_path,
            thread_count,
        )?;

        let new_hash = sha256_of_bytes(&new_data);
        if new_hash != item.new_sha256 {
            if let Err(be) = restore_backup(&target_path, base_dir, &journal.backup_root) {
                anyhow::bail!(
                    "{}",
                    t!(
                        "apply.sha256-fail-restore",
                        item.path,
                        be,
                        target_path.display()
                    )
                );
            }
            anyhow::bail!("{}", t!("apply.sha256-fail-auto-restore", item.path));
        }
    }
    Ok(())
}

/// 应用映射补丁：source 作为差分输入。
///
/// - `delete_source=false`（默认）：source 只读，apply 后继续保留；
/// - `delete_source=true`：在 target 成功生成并通过 SHA256 校验后删除 source，
///   删除前必须先把 source 备份到补丁目录，供失败回滚与 rollback 恢复。
///
/// 目标路径可能已存在（例如 `Old/` 中已有同名文件，或用户目录里本来就有），
/// 此时先备份再覆盖；回滚时删除/恢复目标。
fn apply_renamed_change(
    base_dir: &Path,
    patch_file: &Path,
    item: &ChangedEntry,
    source_path: &Path,
    target_path: &Path,
    old_data: Vec<u8>,
    journal: &mut ChangeJournal,
) -> anyhow::Result<()> {
    println!(
        "{}",
        t!("apply.renamed-changed", item.old_relative_path(), item.path)
    );
    if item.delete_source {
        println!(
            "{}",
            t!("apply.rename-source-delete", item.old_relative_path())
        );
    } else {
        println!(
            "{}",
            t!("apply.rename-source-kept", item.old_relative_path())
        );
    }

    let target_had_backup = if target_path.exists() {
        let target_backup_name = create_backup(target_path, base_dir, &journal.backup_root)?
            .file_name()
            .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
        println!("{}", t!("apply.target-exists-backup", target_backup_name));
        true
    } else {
        false
    };

    // delete_source=true 时先落盘 source 备份，再写 journal：
    // 无论之后在哪一步中断，rollback 都能恢复 source。
    if item.delete_source {
        let source_backup = write_backup(&old_data, source_path, base_dir, &journal.backup_root)?;
        let source_backup_name = source_backup
            .file_name()
            .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
        println!("{}", t!("apply.backed-up", source_backup_name));
    }

    journal.push(JournalEntry::RenamedPatched {
        source: source_path.to_path_buf(),
        target: target_path.to_path_buf(),
        target_had_backup,
        delete_source: item.delete_source,
    })?;

    let patch_data = std::fs::read(patch_file)
        .map_err(|e| anyhow::anyhow!("{}: {}", patch_file.display(), e))?;

    let thread_count = crate::hdiffpatch::get_recommended_thread_count();

    let new_data = apply_patch_auto(old_data, source_path, patch_data, target_path, thread_count)?;

    let new_hash = sha256_of_bytes(&new_data);
    if new_hash != item.new_sha256 {
        // 撤销 target；source 此时尚未删除，保持原样。
        if target_had_backup {
            let _ = restore_backup(target_path, base_dir, &journal.backup_root);
        } else if target_path.exists() {
            let _ = std::fs::remove_file(target_path);
        }
        anyhow::bail!("{}", t!("apply.sha256-fail-auto-restore", item.path));
    }

    // 只有 target 生成并校验成功后才允许删除 source。
    if item.delete_source {
        std::fs::remove_file(source_path)?;
        println!(
            "{}",
            t!("apply.rename-source-deleted", item.old_relative_path())
        );
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
                .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
            println!("{}", t!("apply.target-exists-backup", backup_name));
            true
        } else {
            false
        };

        journal.push(JournalEntry::Added {
            target: target_path.clone(),
            had_backup,
        })?;

        copy_file(&source_file, &target_path)?;

        let new_hash = sha256_of_file(&target_path)?;
        if new_hash != item.new_sha256 {
            anyhow::bail!("{}", t!("apply.added-verify-fail", item.path));
        }
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
                .map_or_else(|| "?".to_string(), |n| n.to_string_lossy().to_string());
            println!("{}", t!("apply.deleted", item.path));
            println!("{}", t!("apply.backed-up", backup_name));
            journal.push(JournalEntry::Deleted {
                target: target_path.clone(),
            })?;
            std::fs::remove_file(&target_path)?;
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
            journal.push(JournalEntry::DeletedDir {
                target: target_dir.clone(),
            })?;
            std::fs::remove_dir_all(&target_dir)?;
            println!("{}", t!("apply.deleted-dir", dir_path));
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
    println!("  - {}", t!("apply.output-generated", output_file));
    println!(
        "  - {}",
        t!("main.patch-size", crate::fmt::format_size(output_size))
    );
    println!("{}", "-".repeat(30));

    Ok(())
}
