use crate::cli::PatchFormat;
use crate::cli::PatchMode;
use crate::file_map::{FileMap, comparison_key};
use crate::fmt::format_size;
use crate::fs::relative_maps;
use crate::hash::sha256_of_file;
use crate::hdiffpatch::{get_diff_thread_count, run_hdiffz, run_hdiffz_mem, run_hdiffz_stream};
use crate::manifest::{AddedEntry, ChangedEntry, DeletedEntry, INSTRUCTIONS_NAME, Manifest};
use crate::patch::{DEFAULT_PATCH_DIR_NAME, patch_dir_for_name};
use crate::path::ensure_parent_dir;
use crate::t;
use anyhow::Context;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[allow(clippy::needless_pass_by_value)]
pub fn build_patch_bundle(
    base_dir: &Path,
    mode: PatchMode,
    format: PatchFormat,
) -> anyhow::Result<()> {
    build_patch_bundle_with_name(base_dir, mode, format, None)
}

#[allow(clippy::needless_pass_by_value)]
pub fn build_patch_bundle_with_name(
    base_dir: &Path,
    mode: PatchMode,
    format: PatchFormat,
    patch_name: Option<&str>,
) -> anyhow::Result<()> {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");
    let patch_dir = patch_dir_for_name(base_dir, patch_name)?;

    crate::path::ensure_no_symlink_components(&old_dir)?;
    crate::path::ensure_no_symlink_components(&new_dir)?;
    crate::path::ensure_no_symlink_components(&patch_dir)?;

    // 所有 preflight 校验（file-map、路径等）必须在改动正式 Patch 之前完成。
    let file_map = crate::file_map::load_file_map(base_dir)?.unwrap_or_default();
    crate::file_map::validate_file_map(&file_map, &old_dir, &new_dir)?;

    // 完整生成到临时 staging 目录，成功后再替换正式 Patch：
    // 任何失败都只清理 staging，上一份有效 Patch 保持不变。
    let staging_dir = staging_dir_for(&patch_dir);
    crate::path::ensure_no_symlink_components(&staging_dir)?;
    if staging_dir.exists() {
        std::fs::remove_dir_all(&staging_dir)?;
    }
    std::fs::create_dir_all(&staging_dir)?;

    let patch_dir_name = patch_dir.file_name().map_or_else(
        || DEFAULT_PATCH_DIR_NAME.to_string(),
        |name| name.to_string_lossy().to_string(),
    );
    let counts = match build_bundle_into(
        &staging_dir,
        &old_dir,
        &new_dir,
        &patch_dir_name,
        &file_map,
        &mode,
        format,
    ) {
        Ok(counts) => counts,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_dir);
            return Err(error);
        }
    };

    if let Err(error) = replace_patch_dir(&patch_dir, &staging_dir) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err(error);
    }

    println!("\n{}", t!("bundle.complete"));
    println!("{}", t!("bundle.changed-count", counts.changed));
    println!("{}", t!("bundle.added-count", counts.added));
    println!("{}", t!("bundle.deleted-count", counts.deleted));
    println!("{}", t!("bundle.deleted-dir-count", counts.deleted_dirs));
    println!("{}", t!("bundle.output-dir", patch_dir.display()));

    Ok(())
}

struct BundleCounts {
    changed: usize,
    added: usize,
    deleted: usize,
    deleted_dirs: usize,
}

#[allow(clippy::needless_pass_by_value)]
fn build_bundle_into(
    patch_dir: &Path,
    old_dir: &Path,
    new_dir: &Path,
    patch_dir_name: &str,
    file_map: &FileMap,
    mode: &PatchMode,
    format: PatchFormat,
) -> anyhow::Result<BundleCounts> {
    let (old_files, old_dirs) = relative_maps(old_dir);
    let (new_files, new_dirs) = relative_maps(new_dir);

    let fast_format = matches!(format, PatchFormat::Fast);

    let mut all_paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for k in old_files.keys() {
        all_paths.insert(k.clone());
    }
    for k in new_files.keys() {
        all_paths.insert(k.clone());
    }

    let mut manifest = Manifest::default();
    let mut changed_count = 0;
    let mut added_count = 0;
    let mut deleted_count = 0;
    let mut deleted_dirs_count = 0;
    let mut patch_resources: BTreeSet<String> = BTreeSet::new();

    println!("{}", t!("bundle.scanning"));

    // Step 1: 先处理显式映射，生成 mapping-aware diff。
    let mappings = resolve_mappings(file_map, &old_files, &new_files)?;
    let mut mapped_old_keys: BTreeSet<String> = BTreeSet::new();
    let mut mapped_new_keys: BTreeSet<String> = BTreeSet::new();
    for mapping in &mappings {
        mapped_old_keys.insert(comparison_key(&mapping.old_rel));
        mapped_new_keys.insert(comparison_key(&mapping.new_rel));
    }

    // 映射源是「保持不变的差分基础文件」（delete_source=false 时）：
    // - New 中存在同名源文件 → 内容必须与 Old 一致（delete_source=false），
    //   或必须不存在（delete_source=true，否则 apply 后无法与 New 一致）。
    for mapping in &mappings {
        let old_rel = &mapping.old_rel;
        // Windows 大小写不敏感：New 中的同名源可能使用不同大小写，
        // 统一通过比较键匹配，再以真实路径访问文件。
        let Some(new_source_key) = find_scanned_key(&new_files, old_rel) else {
            continue;
        };
        if mapping.delete_source {
            anyhow::bail!(
                "{}",
                t!("filemap.source-present-with-delete-source", old_rel)
            );
        }
        let old_source = old_files
            .get(old_rel)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.old-not-found", old_rel)))?;
        let new_source = new_files
            .get(&new_source_key)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.new-not-found", old_rel)))?;
        if sha256_of_file(old_source)? != sha256_of_file(new_source)? {
            anyhow::bail!("{}", t!("filemap.source-modified-in-new", old_rel));
        }
        println!("{}", t!("bundle.mapping-source-leftover", old_rel));
    }

    // 保留的映射源所在目录及其全部祖先目录必须保留：
    // delete_source=true 的源会被删除，其目录允许进入 deleted_dirs。
    let protected_dirs = protected_dirs_of(&mappings);

    for mapping in &mappings {
        let old_rel = &mapping.old_rel;
        let new_rel = &mapping.new_rel;
        let old_file = old_files
            .get(old_rel)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.old-not-found", old_rel)))?;
        let new_file = new_files
            .get(new_rel)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.new-not-found", new_rel)))?;

        println!("{}", t!("bundle.mapping", old_rel, new_rel));
        if mapping.delete_source {
            println!("{}", t!("bundle.mapping-delete-source", old_rel));
        }

        let patch_rel = format!("{new_rel}.patch");
        if !patch_resources.insert(patch_rel.clone()) {
            anyhow::bail!("{}", t!("bundle.patch-conflict", patch_rel));
        }
        let patch_output = patch_dir.join(&patch_rel);
        let task = ChangeTask {
            old_path: old_file.as_path(),
            new_path: new_file.as_path(),
            patch_output: &patch_output,
            old_relative_path: old_rel,
            new_relative_path: new_rel,
            renamed: true,
            delete_source: mapping.delete_source,
        };
        if let Some(entry) = process_changed(&task, fast_format, mode)? {
            manifest.changed.push(entry);
            changed_count += 1;
        }
    }

    // Windows / macOS 下（默认大小写不敏感）同一路径仅大小写不同的普通文件
    // 会被扫描误判为 deleted + added：apply 会先写入新路径（同一物理文件），
    // 再按旧路径删除，最终删掉刚写入的文件。映射路径（源与目标）已由比较键
    // 单独处理，其余情况在扫描阶段直接拒绝；暂不支持 case-only rename，
    // 请统一 Old/New 中的路径大小写。
    #[cfg(any(windows, target_os = "macos"))]
    for old_rel in old_files.keys() {
        let old_key = comparison_key(old_rel);
        if mapped_old_keys.contains(&old_key) || mapped_new_keys.contains(&old_key) {
            continue;
        }
        if let Some(new_rel) = find_scanned_key(&new_files, old_rel)
            && new_rel.as_str() != old_rel.as_str()
        {
            anyhow::bail!("{}", t!("bundle.case-only-change", old_rel, new_rel));
        }
    }

    // Step 2: 已参与映射的路径（源与目标）从普通扫描中整体排除，
    // 避免误判为删除 + 新增，也避免映射目标被 deleted 再次删除。
    for relative_path in all_paths {
        let relative_key = comparison_key(&relative_path);
        if mapped_old_keys.contains(&relative_key) || mapped_new_keys.contains(&relative_key) {
            continue;
        }
        let old_path = old_files.get(&relative_path);
        let new_path = new_files.get(&relative_path);

        match (old_path, new_path) {
            (None, None) => {}
            (Some(old), Some(new)) => {
                let patch_rel = format!("{relative_path}.patch");
                if !patch_resources.insert(patch_rel.clone()) {
                    anyhow::bail!("{}", t!("bundle.patch-conflict", patch_rel));
                }
                let patch_output = patch_dir.join(&patch_rel);
                let task = ChangeTask {
                    old_path: old.as_path(),
                    new_path: new.as_path(),
                    patch_output: &patch_output,
                    old_relative_path: &relative_path,
                    new_relative_path: &relative_path,
                    renamed: false,
                    delete_source: false,
                };
                match process_changed(&task, fast_format, mode)? {
                    Some(entry) => {
                        manifest.changed.push(entry);
                        changed_count += 1;
                    }
                    None => {
                        patch_resources.remove(&patch_rel);
                    }
                }
            }
            (None, Some(new)) => {
                let added_output = patch_dir.join(format!("{relative_path}.new"));
                ensure_parent_dir(&added_output)?;
                std::fs::copy(new, &added_output)?;
                let new_hash = sha256_of_file(new)?;
                println!("{}", t!("bundle.added", &relative_path));
                manifest.added.push(AddedEntry {
                    path: relative_path.clone(),
                    new_sha256: new_hash,
                    file: format!("{relative_path}.new"),
                });
                added_count += 1;
            }
            (Some(old), None) => {
                let old_hash = sha256_of_file(old)?;
                println!("{}", t!("bundle.deleted", &relative_path));
                manifest.deleted.push(DeletedEntry {
                    path: relative_path.clone(),
                    old_sha256: old_hash,
                });
                deleted_count += 1;
            }
        }
    }

    for rel_path in old_dirs.keys() {
        // 映射源的目录链受保护，即使 New 中不存在也不能删除，
        // 否则 apply 会连保留的 source 一起递归删除。
        // New 中目录的存在性同样按比较键判断（Windows 大小写不敏感）。
        if find_scanned_key(&new_dirs, rel_path).is_none() && !protected_dirs.contains(rel_path) {
            manifest.deleted_dirs.push(rel_path.clone());
            println!("{}", t!("bundle.deleted-dir", &rel_path));
            deleted_dirs_count += 1;
        }
    }
    manifest
        .deleted_dirs
        .sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));

    manifest.save(patch_dir)?;
    write_patch_instructions(patch_dir, patch_dir_name)?;

    Ok(BundleCounts {
        changed: changed_count,
        added: added_count,
        deleted: deleted_count,
        deleted_dirs: deleted_dirs_count,
    })
}

/// staging 目录：正式 Patch 的同级隐藏目录，构建失败可整体丢弃。
fn staging_dir_for(patch_dir: &Path) -> PathBuf {
    patch_dir.with_file_name(staging_name(patch_dir, "staging"))
}

/// 替换使用的暂存目录：先将旧 Patch 改名到此处，再换入新 Patch。
fn retired_dir_for(patch_dir: &Path) -> PathBuf {
    patch_dir.with_file_name(staging_name(patch_dir, "retired"))
}

fn staging_name(patch_dir: &Path, suffix: &str) -> String {
    let name = patch_dir.file_name().map_or_else(
        || DEFAULT_PATCH_DIR_NAME.to_string(),
        |n| n.to_string_lossy().to_string(),
    );
    format!(".{name}.{suffix}")
}

/// 用 staging 目录替换正式 Patch；失败时尽力恢复旧 Patch。
fn replace_patch_dir(patch_dir: &Path, staging_dir: &Path) -> anyhow::Result<()> {
    if !patch_dir.exists() {
        std::fs::rename(staging_dir, patch_dir)?;
        return Ok(());
    }

    eprintln!("{}", t!("bundle.will-clear-patch", patch_dir.display()));
    let retired_dir = retired_dir_for(patch_dir);
    crate::path::ensure_no_symlink_components(&retired_dir)?;
    if retired_dir.exists() {
        std::fs::remove_dir_all(&retired_dir)?;
    }
    std::fs::rename(patch_dir, &retired_dir)?;
    match std::fs::rename(staging_dir, patch_dir) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&retired_dir);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::rename(&retired_dir, patch_dir);
            Err(error.into())
        }
    }
}

struct ResolvedMapping {
    old_rel: String,
    new_rel: String,
    delete_source: bool,
}

/// 将 file-map.json 中的用户路径解析为实际扫描到的相对路径。
///
/// 完全匹配优先；其余情况统一使用 [`comparison_key`] 匹配
/// （Windows 下大小写不敏感），避免因大小写差异漏掉映射。
fn resolve_mappings(
    file_map: &FileMap,
    old_files: &BTreeMap<String, PathBuf>,
    new_files: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<Vec<ResolvedMapping>> {
    let mut mappings = Vec::with_capacity(file_map.mappings.len());
    for mapping in &file_map.mappings {
        let old = mapping.old_path()?;
        let new = mapping.new_path()?;
        let old_rel = find_scanned_key(old_files, old.relative())
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.old-not-found", old.relative())))?;
        let new_rel = find_scanned_key(new_files, new.relative())
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.new-not-found", new.relative())))?;
        mappings.push(ResolvedMapping {
            old_rel,
            new_rel,
            delete_source: mapping.delete_source,
        });
    }
    mappings.sort_by(|a, b| a.old_rel.cmp(&b.old_rel).then(a.new_rel.cmp(&b.new_rel)));
    Ok(mappings)
}

/// 保留的映射源（delete_source=false）所在目录及其所有祖先目录，
/// 这些目录不得进入 deleted_dirs。
fn protected_dirs_of(mappings: &[ResolvedMapping]) -> BTreeSet<String> {
    let mut protected = BTreeSet::new();
    for mapping in mappings {
        if mapping.delete_source {
            continue;
        }
        let mut current = Path::new(&mapping.old_rel).parent();
        while let Some(dir) = current {
            let dir_str = dir.to_string_lossy().replace('\\', "/");
            if dir_str.is_empty() {
                break;
            }
            protected.insert(dir_str);
            current = dir.parent();
        }
    }
    protected
}

/// 在扫描结果中查找与 `relative_path` 匹配的真实键。
///
/// 完全匹配优先，否则使用 [`comparison_key`] 回退（Windows 大小写不敏感）；
/// 返回的始终是磁盘上的真实相对路径，用于实际文件访问。
fn find_scanned_key(files: &BTreeMap<String, PathBuf>, relative_path: &str) -> Option<String> {
    if let Some((key, _)) = files.get_key_value(relative_path) {
        return Some(key.clone());
    }
    let target_key = comparison_key(relative_path);
    files
        .keys()
        .find(|key| comparison_key(key) == target_key)
        .cloned()
}

struct ChangeTask<'a> {
    old_path: &'a Path,
    new_path: &'a Path,
    patch_output: &'a Path,
    old_relative_path: &'a str,
    new_relative_path: &'a str,
    renamed: bool,
    delete_source: bool,
}

fn process_changed(
    task: &ChangeTask<'_>,
    fast_format: bool,
    mode: &PatchMode,
) -> anyhow::Result<Option<ChangedEntry>> {
    let old_hash = sha256_of_file(task.old_path)?;
    let new_hash = sha256_of_file(task.new_path)?;
    if old_hash == new_hash && !task.renamed {
        return Ok(None);
    }

    if task.renamed {
        println!(
            "{}",
            t!(
                "bundle.mapped-changed",
                task.old_relative_path,
                task.new_relative_path
            )
        );
        if old_hash == new_hash {
            println!(
                "{}",
                t!(
                    "bundle.mapped-same-hash",
                    task.old_relative_path,
                    task.new_relative_path
                )
            );
        }
    } else {
        println!("{}", t!("bundle.changed", task.new_relative_path));
    }

    let old_size = std::fs::metadata(task.old_path)?.len();
    let new_size = std::fs::metadata(task.new_path)?.len();

    let thread_count = match mode {
        PatchMode::Stream => {
            if !fast_format {
                eprintln!("{}", t!("hdiff.stream-fast-forced"));
            }
            run_hdiffz_stream(
                task.old_path,
                task.new_path,
                task.patch_output,
                get_diff_thread_count(),
                true,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?
        }
        PatchMode::Memory => {
            let old_data = std::fs::read(task.old_path)
                .with_context(|| t!("bundle.failed-read-old", task.old_path.display()))?;
            let new_data = std::fs::read(task.new_path)
                .with_context(|| t!("bundle.failed-read-new", task.new_path.display()))?;
            run_hdiffz_mem(
                &old_data,
                &new_data,
                task.patch_output,
                get_diff_thread_count(),
                fast_format,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?
        }
        PatchMode::Auto => {
            run_hdiffz(task.old_path, task.new_path, task.patch_output, fast_format)?
        }
    };

    print_patch_result(old_size, new_size, task.patch_output, thread_count)?;
    Ok(Some(ChangedEntry {
        path: task.new_relative_path.to_string(),
        source_path: task.renamed.then(|| task.old_relative_path.to_string()),
        delete_source: task.renamed && task.delete_source,
        old_sha256: old_hash,
        new_sha256: new_hash,
        patch_file: format!("{}.patch", task.new_relative_path),
    }))
}

fn print_patch_result(
    old_size: u64,
    new_size: u64,
    patch_file: &Path,
    thread_count: u32,
) -> anyhow::Result<()> {
    let patch_size = std::fs::metadata(patch_file)?.len();
    println!("  {}", "-".repeat(30));
    println!("  {}", t!("bundle.patch-success"));
    println!("    - {}", t!("main.threads-used", thread_count));
    println!("    - {}", t!("main.old-size", format_size(old_size)));
    println!("    - {}", t!("main.new-size", format_size(new_size)));
    println!("    - {}", t!("main.patch-size", format_size(patch_size)));
    println!("  {}", "-".repeat(30));
    Ok(())
}

fn write_patch_instructions(patch_dir: &Path, patch_dir_name: &str) -> anyhow::Result<()> {
    let lines = [
        "This is an auto-generated patch bundle by binary_patcher.".to_string(),
        String::new(),
        "Usage:".to_string(),
        format!("1. Copy the entire {patch_dir_name} folder to the old version root directory."),
        "2. Place apply_patch in the old version root directory and run it.".to_string(),
        "3. The program will apply patches according to manifest.json.".to_string(),
    ];
    let instructions_path = crate::path::resolve_safe_path(patch_dir, INSTRUCTIONS_NAME)?;
    crate::path::ensure_parent_dir(&instructions_path)?;
    std::fs::write(instructions_path, lines.join("\n"))?;
    Ok(())
}

pub fn init_workspace(base_dir: &Path) -> anyhow::Result<bool> {
    init_workspace_with_name(base_dir, None)
}

pub fn init_workspace_with_name(base_dir: &Path, patch_name: Option<&str>) -> anyhow::Result<bool> {
    let patch_dir = patch_dir_for_name(base_dir, patch_name)?;
    let mut created = Vec::new();

    for folder_path in [
        base_dir.join("Old"),
        base_dir.join("New"),
        patch_dir.clone(),
    ] {
        crate::path::ensure_no_symlink_components(&folder_path)?;
        if !folder_path.exists() {
            std::fs::create_dir_all(&folder_path)?;
            created.push(folder_path.file_name().map_or_else(
                || folder_path.display().to_string(),
                |name| name.to_string_lossy().to_string(),
            ));
        }
    }

    if !created.is_empty() {
        println!("{}", t!("bundle.workspace-initialized", created.join(", ")));
    }

    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");

    let old_empty = std::fs::read_dir(&old_dir)?.next().is_none();
    let new_empty = std::fs::read_dir(&new_dir)?.next().is_none();

    if old_empty || new_empty {
        println!("\n{}", t!("bundle.workspace-instructions"));
        println!("{}", t!("bundle.workspace-old"));
        println!("{}", t!("bundle.workspace-new"));
        let output_name = patch_dir.file_name().map_or_else(
            || patch_dir.display().to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        println!("{}", t!("bundle.workspace-output", output_name));
        println!("\n{}", t!("bundle.workspace-ready"));
        return Ok(false);
    }

    Ok(true)
}
