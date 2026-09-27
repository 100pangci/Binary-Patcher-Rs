use crate::cli::PatchFormat;
use crate::cli::PatchMode;
use crate::file_map::FileMap;
use crate::fmt::format_size;
use crate::fs::relative_maps;
use crate::hash::sha256_of_file;
use crate::hdiffpatch::{get_diff_thread_count, run_hdiffz, run_hdiffz_mem, run_hdiffz_stream};
use crate::manifest::{AddedEntry, ChangedEntry, DeletedEntry, INSTRUCTIONS_NAME, Manifest};
use crate::patch::patch_dir_for_name;
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
    if patch_dir.exists() {
        eprintln!("{}", t!("bundle.will-clear-patch", patch_dir.display()));
        std::fs::remove_dir_all(&patch_dir)?;
    }
    std::fs::create_dir_all(&patch_dir)?;

    let (old_files, old_dirs) = relative_maps(&old_dir);
    let (new_files, new_dirs) = relative_maps(&new_dir);

    let file_map = crate::file_map::load_file_map(base_dir)?.unwrap_or_default();
    crate::file_map::validate_file_map(&file_map, &old_dir, &new_dir)?;

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

    // Step 1: 先处理显式映射，生成 rename-aware diff。
    let mappings = resolve_mappings(&file_map, &old_files, &new_files)?;
    let mut mapped_old: BTreeSet<String> = BTreeSet::new();
    let mut mapped_new: BTreeSet<String> = BTreeSet::new();
    for (old_rel, new_rel) in &mappings {
        let old_file = old_files
            .get(old_rel)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.old-not-found", old_rel)))?;
        let new_file = new_files
            .get(new_rel)
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.new-not-found", new_rel)))?;

        mapped_old.insert(old_rel.clone());
        mapped_new.insert(new_rel.clone());
        println!("{}", t!("bundle.mapping", old_rel, new_rel));

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
        };
        if let Some(entry) = process_changed(&task, fast_format, &mode)? {
            manifest.changed.push(entry);
            changed_count += 1;
        }
    }

    // 映射源路径若在 New 中仍存在（准备 New 时未删除旧名文件），
    // 属于已由映射消费的文件，必须整体忽略；否则会被全量复制为“新增”。
    for old_rel in &mapped_old {
        if new_files.contains_key(old_rel) {
            println!("{}", t!("bundle.mapping-source-leftover", old_rel));
        }
    }

    // Step 2: 已参与映射的路径（源与目标）从普通扫描中整体排除，
    // 避免误判为删除 + 新增，也避免映射目标被 deleted 再次删除。
    for relative_path in all_paths {
        if mapped_old.contains(&relative_path) || mapped_new.contains(&relative_path) {
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
                };
                match process_changed(&task, fast_format, &mode)? {
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
        if !new_dirs.contains_key(rel_path) {
            manifest.deleted_dirs.push(rel_path.clone());
            println!("{}", t!("bundle.deleted-dir", &rel_path));
            deleted_dirs_count += 1;
        }
    }
    manifest
        .deleted_dirs
        .sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));

    manifest.save(&patch_dir)?;
    write_patch_instructions(&patch_dir)?;

    println!("\n{}", t!("bundle.complete"));
    println!("{}", t!("bundle.changed-count", changed_count));
    println!("{}", t!("bundle.added-count", added_count));
    println!("{}", t!("bundle.deleted-count", deleted_count));
    println!("{}", t!("bundle.deleted-dir-count", deleted_dirs_count));
    println!("{}", t!("bundle.output-dir", patch_dir.display()));

    Ok(())
}

/// 将 file-map.json 中的用户路径解析为实际扫描到的相对路径。
///
/// 完全匹配优先；Windows 文件系统大小写不敏感，因此额外做一次
/// ASCII 大小写不敏感回退，避免因大小写差异漏掉映射。
fn resolve_mappings(
    file_map: &FileMap,
    old_files: &BTreeMap<String, PathBuf>,
    new_files: &BTreeMap<String, PathBuf>,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut mappings = Vec::with_capacity(file_map.mappings.len());
    for mapping in &file_map.mappings {
        let old_rel = find_scanned_key(old_files, &mapping.old_path())
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.old-not-found", mapping.old_path())))?;
        let new_rel = find_scanned_key(new_files, &mapping.new_path())
            .ok_or_else(|| anyhow::anyhow!(t!("filemap.new-not-found", mapping.new_path())))?;
        mappings.push((old_rel, new_rel));
    }
    mappings.sort();
    Ok(mappings)
}

fn find_scanned_key(files: &BTreeMap<String, PathBuf>, relative_path: &str) -> Option<String> {
    if files.contains_key(relative_path) {
        return Some(relative_path.to_string());
    }
    #[cfg(windows)]
    {
        files
            .keys()
            .find(|key| key.eq_ignore_ascii_case(relative_path))
            .cloned()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

struct ChangeTask<'a> {
    old_path: &'a Path,
    new_path: &'a Path,
    patch_output: &'a Path,
    old_relative_path: &'a str,
    new_relative_path: &'a str,
    renamed: bool,
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

fn write_patch_instructions(patch_dir: &Path) -> anyhow::Result<()> {
    let patch_dir_name = patch_dir.file_name().map_or_else(
        || "Patch".to_string(),
        |name| name.to_string_lossy().to_string(),
    );
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
