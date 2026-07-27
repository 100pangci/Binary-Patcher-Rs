//! 整目录打包：对比 Old/ 与 New/，生成 manifest + 补丁 + 新增文件副本。

use crate::cli::PatchFormat;
use crate::cli::PatchMode;
use crate::hdiffpatch::{get_diff_thread_count, run_hdiffz_mem, run_hdiffz_stream};
use crate::manifest::{AddedEntry, ChangedEntry, DeletedEntry, INSTRUCTIONS_NAME, Manifest};
use crate::utils::{
    ensure_parent_dir, format_size, relative_maps, sha256_of_bytes, sha256_of_file,
};
use std::path::Path;

/// 对比 Old/New 目录，在 Patch/ 生成 manifest.json、补丁文件和新增文件副本。
pub fn build_patch_bundle(
    base_dir: &Path,
    use_compression: bool,
    mode: PatchMode,
    format: PatchFormat,
) -> anyhow::Result<()> {
    let old_dir = base_dir.join("Old");
    let new_dir = base_dir.join("New");
    let patch_dir = base_dir.join("Patch");

    if patch_dir.exists() {
        eprintln!("注意: 将清空已有的 Patch 目录: {}", patch_dir.display());
        std::fs::remove_dir_all(&patch_dir)?;
    }
    std::fs::create_dir_all(&patch_dir)?;

    let (old_files, old_dirs) = relative_maps(&old_dir);
    let (new_files, new_dirs) = relative_maps(&new_dir);

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

    println!("开始扫描 Old / New 并计算 SHA256...");

    for relative_path in all_paths {
        let old_path = old_files.get(&relative_path);
        let new_path = new_files.get(&relative_path);

        match (old_path, new_path) {
            (Some(old), Some(new)) => {
                let patch_output = patch_dir.join(format!("{relative_path}.patch"));

                match mode {
                    PatchMode::Stream => {
                        let old_hash = sha256_of_file(old)?;
                        let new_hash = sha256_of_file(new)?;
                        if old_hash == new_hash {
                            continue;
                        }
                        println!("[变更] {relative_path}");
                        create_patch_stream(old, new, &patch_output, use_compression, fast_format)?;
                        manifest.changed.push(ChangedEntry {
                            path: relative_path.clone(),
                            old_sha256: old_hash,
                            new_sha256: new_hash,
                            patch_file: format!("{relative_path}.patch"),
                        });
                    }
                    PatchMode::Memory => {
                        let old_data = std::fs::read(old).map_err(|e| {
                            anyhow::anyhow!("读取旧文件失败 {}: {e}", old.display())
                        })?;
                        let new_data = std::fs::read(new).map_err(|e| {
                            anyhow::anyhow!("读取新文件失败 {}: {e}", new.display())
                        })?;
                        let old_hash = sha256_of_bytes(&old_data);
                        let new_hash = sha256_of_bytes(&new_data);
                        if old_hash == new_hash {
                            continue;
                        }
                        println!("[变更] {relative_path}");
                        create_patch_mem(
                            &old_data,
                            &new_data,
                            &patch_output,
                            use_compression,
                            fast_format,
                        )?;
                        manifest.changed.push(ChangedEntry {
                            path: relative_path.clone(),
                            old_sha256: old_hash,
                            new_sha256: new_hash,
                            patch_file: format!("{relative_path}.patch"),
                        });
                    }
                    PatchMode::Auto => {
                        let (old_hash, new_hash) = process_changed_auto(
                            old,
                            new,
                            &patch_output,
                            use_compression,
                            fast_format,
                            &relative_path,
                        )?;
                        match (&old_hash, &new_hash) {
                            (Some(oh), Some(nh)) => {
                                manifest.changed.push(ChangedEntry {
                                    path: relative_path.clone(),
                                    old_sha256: oh.clone(),
                                    new_sha256: nh.clone(),
                                    patch_file: format!("{relative_path}.patch"),
                                });
                            }
                            _ => continue,
                        }
                    }
                }
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

    // old_dirs and new_dirs already populated from relative_maps above
    for rel_path in old_dirs.keys() {
        if !new_dirs.contains_key(rel_path) {
            manifest.deleted_dirs.push(rel_path.clone());
            println!("[删除目录] {rel_path}");
            deleted_dirs_count += 1;
        }
    }
    manifest
        .deleted_dirs
        .sort_by(|a, b| b.len().cmp(&a.len()).then(b.cmp(a)));

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

fn print_patch_result(
    old_size: u64,
    new_size: u64,
    patch_file: &Path,
    thread_count: u32,
) -> anyhow::Result<()> {
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

fn create_patch_mem(
    old_data: &[u8],
    new_data: &[u8],
    patch_file: &Path,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<()> {
    ensure_parent_dir(patch_file)?;
    let old_size = old_data.len() as u64;
    let new_size = new_data.len() as u64;

    println!("  正在调用 HDiffPatch 生成补丁...");
    let thread_count = run_hdiffz_mem(
        old_data,
        new_data,
        patch_file,
        get_diff_thread_count(),
        use_compression,
        fast_format,
    )
    .map_err(|e| anyhow::anyhow!("创建补丁失败: {e}"))?;
    print_patch_result(old_size, new_size, patch_file, thread_count)
}

fn create_patch_stream(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<()> {
    ensure_parent_dir(patch_file)?;
    let old_size = std::fs::metadata(old_file)?.len();
    let new_size = std::fs::metadata(new_file)?.len();

    println!("  正在调用 HDiffPatch 生成补丁...");
    let thread_count = run_hdiffz_stream(
        old_file,
        new_file,
        patch_file,
        get_diff_thread_count(),
        use_compression,
        fast_format,
    )
    .map_err(|e| anyhow::anyhow!("创建补丁失败: {e}"))?;
    print_patch_result(old_size, new_size, patch_file, thread_count)
}

/// Auto 模式处理单个变更文件：尝试内存模式，OOM 时回退流式。
/// 返回 (old_sha256, new_sha256)，任一为 None 表示文件未变更需跳过。
fn process_changed_auto(
    old: &Path,
    new: &Path,
    patch_output: &Path,
    use_compression: bool,
    fast_format: bool,
    relative_path: &str,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    let (old_hash, new_hash) = match try_read_old_new(old, new) {
        Ok((od, nd)) => {
            let oh = sha256_of_bytes(&od);
            let nh = sha256_of_bytes(&nd);
            if oh == nh {
                return Ok((None, None));
            }
            println!("[变更] {relative_path}");
            match run_hdiffz_mem(
                &od,
                &nd,
                patch_output,
                get_diff_thread_count(),
                use_compression,
                fast_format,
            ) {
                Ok(_) => {
                    let patch_size = std::fs::metadata(patch_output)?.len();
                    println!("  补丁创建成功！");
                    println!("    - 旧文件大小: {}", format_size(od.len() as u64));
                    println!("    - 新文件大小: {}", format_size(nd.len() as u64));
                    println!("    - 补丁文件大小: {}", format_size(patch_size));
                }
                Err(e) if e.is_oom() => {
                    let total_gb = (od.len() + nd.len()) as f64 / (1u64 << 30) as f64;
                    eprintln!("注意: 内存不足（{:.1}GB），自动切换为流式模式", total_gb);
                    create_patch_stream(old, new, patch_output, use_compression, fast_format)?;
                }
                Err(e) => anyhow::bail!("创建补丁失败 ({relative_path}): {e}"),
            }
            (oh, nh)
        }
        Err(e) if e.is_oom() => {
            let oh = sha256_of_file(old)?;
            let nh = sha256_of_file(new)?;
            if oh == nh {
                return Ok((None, None));
            }
            println!("[变更] {relative_path}");
            create_patch_stream(old, new, patch_output, use_compression, fast_format)?;
            (oh, nh)
        }
        Err(e) => anyhow::bail!("处理文件失败 ({relative_path}): {e}"),
    };
    Ok((Some(old_hash), Some(new_hash)))
}

fn try_read_old_new(old: &Path, new: &Path) -> Result<(Vec<u8>, Vec<u8>), crate::ffi::PatchError> {
    let old_data = std::fs::read(old).map_err(|e| crate::ffi::PatchError {
        code: -1,
        message: format!("读取旧文件失败 {}: {e}", old.display()),
    })?;
    let new_data = std::fs::read(new).map_err(|e| crate::ffi::PatchError {
        code: -1,
        message: format!("读取新文件失败 {}: {e}", new.display()),
    })?;
    Ok((old_data, new_data))
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

/// 如果缺少 Old/New/Patch 目录则创建，返回是否可继续打包。
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
