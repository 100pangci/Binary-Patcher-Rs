use crate::ffi;
use crate::t;
use std::path::Path;

const DEFAULT_THREADS: u32 = 4;
const MAX_PATCH_THREADS: u32 = 5;
const MAX_DIFF_THREADS: u32 = 32;
/// 内存模式允许的 old + new 总大小上限（字节）。
/// 超过该阈值时直接走流式，避免先把整个文件读进内存再 OOM。
/// 同时用于 apply 侧：输出数据过大时直接流式，避免 Rust 分配 OOM（无法优雅降级）。
const MAX_MEM_DIFF_BYTES: u64 = 1 << 30;

fn clamp_thread_count(max: u32) -> u32 {
    let cpu_count = std::thread::available_parallelism()
        .map_or(DEFAULT_THREADS as usize, std::num::NonZeroUsize::get);
    (cpu_count.saturating_sub(1)).max(1).min(max as usize) as u32
}

fn should_stream_by_size(old_size: u64, new_size: u64) -> bool {
    old_size.saturating_add(new_size) > MAX_MEM_DIFF_BYTES
}

pub fn get_recommended_thread_count() -> u32 {
    clamp_thread_count(MAX_PATCH_THREADS)
}

pub fn get_diff_thread_count() -> u32 {
    clamp_thread_count(MAX_DIFF_THREADS)
}

pub fn run_hdiffz_mem(
    old_data: &[u8],
    new_data: &[u8],
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<u32, ffi::PatchError> {
    crate::path::ensure_parent_dir(patch_file).map_err(|e| ffi::PatchError {
        code: -1,
        message: e.to_string(),
    })?;

    let patch_data = ffi::create_patch(
        old_data,
        new_data,
        thread_count,
        use_compression,
        fast_format,
    )?;
    std::fs::write(patch_file, &patch_data).map_err(|e| ffi::PatchError {
        code: -1,
        message: t!("ffi.write-failed", patch_file.display(), e),
    })?;
    Ok(thread_count)
}

pub fn run_hdiffz_stream(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<u32, ffi::PatchError> {
    crate::path::ensure_parent_dir(patch_file).map_err(|e| ffi::PatchError {
        code: -1,
        message: e.to_string(),
    })?;

    ffi::create_patch_file(
        old_file,
        new_file,
        patch_file,
        thread_count,
        use_compression,
        fast_format,
    )?;
    Ok(thread_count)
}

pub fn run_hdiffz(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<u32> {
    let thread_count = get_diff_thread_count();

    crate::path::ensure_parent_dir(patch_file)?;

    let old_size = std::fs::metadata(old_file).map(|m| m.len()).ok();
    let new_size = std::fs::metadata(new_file).map(|m| m.len()).ok();

    if let (Some(old_size), Some(new_size)) = (old_size, new_size)
        && should_stream_by_size(old_size, new_size)
    {
        let total_gb = (old_size + new_size) as f64 / (1u64 << 30) as f64;
        eprintln!("{}", t!("hdiff.size-fallback", format!("{:.1}", total_gb)));
        return run_hdiffz_stream_forced(
            old_file,
            new_file,
            patch_file,
            thread_count,
            use_compression,
            fast_format,
        )
        .map_err(|e| anyhow::anyhow!("{e}"));
    }

    let old_data = std::fs::read(old_file)
        .map_err(|e| anyhow::anyhow!("{}", t!("ffi.read-old-failed", old_file.display(), e)))?;
    let new_data = std::fs::read(new_file)
        .map_err(|e| anyhow::anyhow!("{}", t!("ffi.read-new-failed", new_file.display(), e)))?;

    let mem_result = run_hdiffz_mem(
        &old_data,
        &new_data,
        patch_file,
        thread_count,
        use_compression,
        fast_format,
    );
    // 无论成功失败，立即释放内存路径读入的文件数据：
    // OOM 流式回退时若仍占用内存，会导致流式再次 OOM
    // （且 HDiffPatch 内部线程池在异常展开时无法安全回收，直接 terminate）。
    drop(old_data);
    drop(new_data);

    match mem_result {
        Ok(_) => Ok(thread_count),
        Err(e) if e.is_oom() => {
            match (old_size, new_size) {
                (Some(os), Some(ns)) => {
                    let total_gb = (os + ns) as f64 / (1u64 << 30) as f64;
                    eprintln!("{}", t!("hdiff.oom-fallback", format!("{:.1}", total_gb)));
                }
                _ => {
                    eprintln!("{}", t!("hdiff.oom-fallback-generic"));
                }
            }
            run_hdiffz_stream_forced(
                old_file,
                new_file,
                patch_file,
                thread_count,
                use_compression,
                fast_format,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

/// 流式创建补丁。precise 格式的流式算法（TDigestMatcher + serialize）在内存
/// 不足时会在 C 层静默截断输出，生成损坏补丁且不报错；fast 格式（window matcher）
/// 内存可控且可靠。因此流式路径强制 fast 格式，precise 仅内存路径可用。
fn run_hdiffz_stream_forced(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<u32, ffi::PatchError> {
    if !fast_format {
        eprintln!("{}", t!("hdiff.stream-fast-forced"));
    }
    run_hdiffz_stream(
        old_file,
        new_file,
        patch_file,
        thread_count,
        use_compression,
        true,
    )
}

pub fn apply_patch_auto(
    old_data: Vec<u8>,
    old_file: &Path,
    patch_data: Vec<u8>,
    output_file: &Path,
    thread_count: u32,
) -> Result<Vec<u8>, anyhow::Error> {
    crate::path::ensure_parent_dir(output_file)?;

    // 先解析补丁头获取输出大小：
    // 输出数据过大时直接走流式，避免 Rust 分配输出缓冲时 OOM（无法优雅降级）。
    let new_size = ffi::patch_new_size(&patch_data).map_err(|e| anyhow::anyhow!("{e}"))?;
    if new_size == 0 {
        std::fs::write(output_file, [])?;
        return Ok(Vec::new());
    }
    if old_data.len() as u64 + new_size as u64 > MAX_MEM_DIFF_BYTES {
        eprintln!(
            "{}",
            t!(
                "hdiff.size-fallback-apply",
                crate::fmt::format_size(new_size as u64)
            )
        );
        // 释放内存模式读入的旧文件数据，给流式腾出内存
        drop(old_data);
        ffi::apply_patch_file(old_file, &patch_data, output_file, thread_count)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        drop(patch_data);
        let new_data = std::fs::read(output_file).map_err(|e| {
            anyhow::anyhow!("{}", t!("ffi.read-output-failed", output_file.display(), e))
        })?;
        return Ok(new_data);
    }

    let result = apply_patch_with_retry(&old_data, &patch_data, thread_count);
    match result {
        Ok(new_data) => {
            std::fs::write(output_file, &new_data).map_err(|e| {
                anyhow::anyhow!(
                    "{}",
                    t!("ffi.write-output-failed", output_file.display(), e)
                )
            })?;
            Ok(new_data)
        }
        Err(e) if e.is_oom() => {
            eprintln!("{}", t!("hdiff.stream-fallback"));
            // 释放内存模式读入的旧文件数据，给流式腾出内存
            drop(old_data);
            ffi::apply_patch_file(old_file, &patch_data, output_file, thread_count)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            drop(patch_data);
            let new_data = std::fs::read(output_file).map_err(|e| {
                anyhow::anyhow!("{}", t!("ffi.read-output-failed", output_file.display(), e))
            })?;
            Ok(new_data)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn apply_patch_with_retry(
    old_data: &[u8],
    patch_data: &[u8],
    thread_count: u32,
) -> Result<Vec<u8>, ffi::PatchError> {
    match ffi::apply_patch(old_data, patch_data, thread_count) {
        Ok(data) => Ok(data),
        Err(e) if e.is_oom() => Err(e),
        Err(e) if thread_count > 1 => {
            eprintln!("{}", t!("hdiff.mt-fallback", e));
            ffi::apply_patch(old_data, patch_data, 1)
        }
        Err(e) => Err(e),
    }
}

pub fn run_hpatchz(old_file: &Path, patch_file: &Path, output_file: &Path) -> anyhow::Result<()> {
    let thread_count = get_recommended_thread_count();
    let old_data = std::fs::read(old_file)
        .map_err(|e| anyhow::anyhow!("{}", t!("ffi.read-old-failed", old_file.display(), e)))?;
    let patch_data = std::fs::read(patch_file)
        .map_err(|e| anyhow::anyhow!("{}", t!("ffi.read-new-failed", patch_file.display(), e)))?;
    apply_patch_auto(old_data, old_file, patch_data, output_file, thread_count)?;
    Ok(())
}
