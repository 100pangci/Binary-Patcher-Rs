use crate::cli::PatchMode;
use crate::cli::PatchFormat;
use crate::hdiffpatch::{get_diff_thread_count, run_hdiffz_mem, run_hdiffz_stream};
use crate::manifest::{Manifest, ChangedEntry, AddedEntry, DeletedEntry, INSTRUCTIONS_NAME};
use crate::utils::{format_size, sha256_of_bytes, sha256_of_file, relative_file_map, relative_dir_map, ensure_parent_dir};
use std::path::Path;

pub fn build_patch_bundle(base_dir: &Path, use_compression: bool, mode: PatchMode, format: PatchFormat) -> anyhow::Result<()> {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");
    let patch_dir = base_dir.join("Patch");

    if patch_dir.exists() {
        std::fs::remove_dir_all(&patch_dir)?;
    }
    std::fs::create_dir_all(&patch_dir)?;

    let old_files = relative_file_map(&old_dir);
    let new_files = relative_file_map(&new_dir);

    let fast_format = matches!(format, PatchFormat::Fast);

    let mut all_paths: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
    for k in old_files.keys() { all_paths.insert(k); }
    for k in new_files.keys() { all_paths.insert(k); }

    let mut manifest = Manifest::new();
    let mut changed_count = 0;
    let mut added_count = 0;
    let mut deleted_count = 0;
    let mut deleted_dirs_count = 0;

    println!("开始扫描 Old / New 并计算 SHA256...");

    for relative_path in all_paths {
        let old_path = old_files.get(relative_path);
        let new_path = new_files.get(relative_path);

        match (old_path, new_path) {
            (Some(old), Some(new)) => {
                let (old_hash, new_hash);
                let patch_output = patch_dir.join(format!("{relative_path}.patch"));

                match mode {
                    PatchMode::Stream => {
                        old_hash = sha256_of_file(old)?;
                        new_hash = sha256_of_file(new)?;
                        if old_hash == new_hash { continue; }
                        println!("[变更] {relative_path}");
                        create_patch_stream(old, new, &patch_output, use_compression, fast_format)?;
                    }
                    PatchMode::Memory => {
                        let old_data = std::fs::read(old)
                            .map_err(|e| anyhow::anyhow!("读取旧文件失败 {}: {e}", old.display()))?;
                        let new_data = std::fs::read(new)
                            .map_err(|e| anyhow::anyhow!("读取新文件失败 {}: {e}", new.display()))?;
                        old_hash = sha256_of_bytes(&old_data);
                        new_hash = sha256_of_bytes(&new_data);
                        if old_hash == new_hash { continue; }
                        println!("[变更] {relative_path}");
                        create_patch_mem(&old_data, &new_data, &patch_output, use_compression, fast_format)?;
                    }
                    PatchMode::Auto => {
                        // Try memory first (best patch quality + single file read)
                        let try_mem = (|| -> anyhow::Result<(String, String, Vec<u8>, Vec<u8>)> {
                            let old_data = std::fs::read(old)
                                .map_err(|e| anyhow::anyhow!("读取旧文件 {}: {e}", old.display()))?;
                            let new_data = std::fs::read(new)
                                .map_err(|e| anyhow::anyhow!("读取新文件 {}: {e}", new.display()))?;
                            let oh = sha256_of_bytes(&old_data);
                            let nh = sha256_of_bytes(&new_data);
                            Ok((oh, nh, old_data, new_data))
                        })();

                        match try_mem {
                            Ok((oh, nh, old_data, new_data)) => {
                                if oh == nh { continue; }
                                old_hash = oh;
                                new_hash = nh;
                                println!("[变更] {relative_path}");
                                create_patch_mem(&old_data, &new_data, &patch_output, use_compression, fast_format)?;
                            }
                            Err(e) => {
                                let msg = e.to_string();
                                let old_sz = std::fs::metadata(old).map(|m| m.len()).unwrap_or(0);
                                let new_sz = std::fs::metadata(new).map(|m| m.len()).unwrap_or(0);
                                let total_gb = (old_sz + new_sz) as f64 / (1u64 << 30) as f64;
                                if msg.contains("内存") || msg.contains("memory") || msg.contains("OOM") {
                                    eprintln!("注意: 内存不足（{:.1}GB），自动切换为流式模式", total_gb);
                                    old_hash = sha256_of_file(old)?;
                                    new_hash = sha256_of_file(new)?;
                                    if old_hash == new_hash { continue; }
                                    println!("[变更] {relative_path}");
                        create_patch_stream(old, new, &patch_output, use_compression, fast_format)?;
                                } else {
                                    return Err(e);
                                }
                            }
                        }
                    }
                }
                manifest.changed.push(ChangedEntry {
                    path: relative_path.clone(),
                    old_sha256: old_hash,
                    new_sha256: new_hash,
                    patch_file: format!("{relative_path}.patch"),
                });
                changed_count += 1;
            }
            (None, Some(new)) => {
                let added_output = patch_dir.join(format!("{relative_path}.new"));
                ensure_parent_dir(&added_output)?;
                std::fs::copy(new, &added_output)?;
                let new_hash = crate::utils::sha256_of_file(new)?;
                println!("[新增] {relative_path}");
                manifest.added.push(AddedEntry {
                    path: relative_path.clone(),
                    new_sha256: new_hash,
                    file: format!("{relative_path}.new"),
                });
                added_count += 1;
            }
            (Some(old), None) => {
                let old_hash = crate::utils::sha256_of_file(old)?;
                println!("[删除] {relative_path}");
                manifest.deleted.push(DeletedEntry {
                    path: relative_path.clone(),
                    old_sha256: old_hash,
                });
                deleted_count += 1;
            }
            (None, None) => unreachable!(),
        }
    }

    // Scan for directories that exist in Old but not in New
    let old_dirs = relative_dir_map(&old_dir);
    let new_dirs = relative_dir_map(&new_dir);
    for (rel_path, _) in &old_dirs {
        if !new_dirs.contains_key(rel_path) {
            manifest.deleted_dirs.push(rel_path.clone());
            println!("[删除目录] {rel_path}");
            deleted_dirs_count += 1;
        }
    }
    // Sort deepest-first for deletion order at apply time
    manifest.deleted_dirs.sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));

    manifest.save(&patch_dir)?;
    write_patch_instructions(&patch_dir)?;

    println!("\n补丁包生成完成！");
    println!("- 变更文件: {changed_count}");
    println!("- 新增文件: {added_count}");
    println!("- 删除文件: {deleted_count}");
    println!("- 删除目录: {deleted_dirs_count}");
    println!("- 输出目录: {}", patch_dir.display());

    Ok(())
}

fn create_patch_mem(old_data: &[u8], new_data: &[u8], patch_file: &Path, use_compression: bool, fast_format: bool) -> anyhow::Result<()> {
    ensure_parent_dir(patch_file)?;
    let old_size = old_data.len();
    let new_size = new_data.len();

    println!("  正在调用 HDiffPatch 生成补丁...");
    let thread_count = run_hdiffz_mem(old_data, new_data, patch_file, get_diff_thread_count(), use_compression, fast_format)?;
    let patch_size = std::fs::metadata(patch_file)?.len();

    println!("  {}", "-".repeat(30));
    println!("  补丁创建成功！");
    println!("    - 使用线程数: {thread_count}");
    println!("    - 旧文件大小: {}", format_size(old_size as u64));
    println!("    - 新文件大小: {}", format_size(new_size as u64));
    println!("    - 补丁文件大小: {}", format_size(patch_size));
    println!("  {}", "-".repeat(30));

    Ok(())
}

fn create_patch_stream(old_file: &Path, new_file: &Path, patch_file: &Path, use_compression: bool, fast_format: bool) -> anyhow::Result<()> {
    ensure_parent_dir(patch_file)?;
    let old_size = std::fs::metadata(old_file)?.len();
    let new_size = std::fs::metadata(new_file)?.len();

    println!("  正在调用 HDiffPatch 生成补丁...");
    let thread_count = run_hdiffz_stream(old_file, new_file, patch_file, get_diff_thread_count(), use_compression, fast_format)?;
    let patch_size = std::fs::metadata(patch_file)?.len();

    println!("  {}", "-".repeat(30));
    println!("  补丁创建成功！");
    println!("    - 使用线程数: {thread_count}");
    println!("    - 旧文件大小: {}", format_size(old_size));
    println!("    - 新文件大小: {}", format_size(new_size));
    println!("    - 补丁文件大小: {}", format_size(patch_size));
    println!("  {}", "-".repeat(30));

    Ok(())
}

fn write_patch_instructions(patch_dir: &Path) -> anyhow::Result<()> {
    let lines = [
        "这是由 binary_patcher 自动生成的整包补丁目录。",
        "",
        "使用方式：",
        "1. 将整个 Patch 文件夹复制到旧版本根目录。",
        "2. 下载 Release 中的 apply_patch.exe 放到旧版本根目录并双击运行。",
        "3. 程序会按 manifest.json 和原始目录结构自动完成补丁应用。",
    ];
    std::fs::write(patch_dir.join(INSTRUCTIONS_NAME), lines.join("\n"))?;
    Ok(())
}

pub fn init_workspace(base_dir: &Path) -> anyhow::Result<bool> {
    let mut created = Vec::new();

    for folder_name in &["Old", "New", "Patch"] {
        let folder_path = base_dir.join(folder_name);
        if !folder_path.exists() {
            std::fs::create_dir_all(&folder_path)?;
            created.push(*folder_name);
        }
    }

    if !created.is_empty() {
        println!("已初始化工作目录：{}", created.join(", "));
    }

    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");

    let old_empty = std::fs::read_dir(&old_dir)?.next().is_none();
    let new_empty = std::fs::read_dir(&new_dir)?.next().is_none();

    if old_empty || new_empty {
        println!("\n请按以下方式准备文件：");
        println!("- 旧版本完整目录放入: Old/");
        println!("- 新版本完整目录放入: New/");
        println!("- 生成的补丁输出到: Patch/");
        println!("\n准备完成后，再次运行本程序即可自动生成整包补丁。");
        return Ok(false);
    }

    Ok(true)
}
