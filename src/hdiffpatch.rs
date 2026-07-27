//! HDiffPatch 高层封装：线程数推荐、内存/流式模式切换、自动 OOM 回退。

use crate::ffi;
use std::path::Path;

const DEFAULT_THREADS: u32 = 4;
const MAX_PATCH_THREADS: u32 = 5;
const MAX_DIFF_THREADS: u32 = 32;

/// 获得推荐的应用补丁线程数 (min(CPU-1, 5), 至少 1)。
pub fn get_recommended_thread_count() -> u32 {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(DEFAULT_THREADS as usize);
    (cpu_count.saturating_sub(1))
        .max(1)
        .min(MAX_PATCH_THREADS as usize) as u32
}

/// 获得推荐的差分线程数 (min(CPU-1, 32), 至少 1)。
pub fn get_diff_thread_count() -> u32 {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(DEFAULT_THREADS as usize);
    (cpu_count.saturating_sub(1))
        .max(1)
        .min(MAX_DIFF_THREADS as usize) as u32
}

/// 全内存模式创建补丁（加载到内存 → diff → 写文件）。
pub fn run_hdiffz_mem(
    old_data: &[u8],
    new_data: &[u8],
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<u32, ffi::PatchError> {
    crate::utils::ensure_parent_dir(patch_file).map_err(|e| ffi::PatchError {
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
        message: format!("写入补丁文件失败 {}: {e}", patch_file.display()),
    })?;
    Ok(thread_count)
}

/// 流式模式创建补丁（文件直读直写，低内存）。
pub fn run_hdiffz_stream(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<u32, ffi::PatchError> {
    crate::utils::ensure_parent_dir(patch_file).map_err(|e| ffi::PatchError {
        code: -1,
        message: e.to_string(),
    })?;

    ffi::create_patch_file(
        &old_file.to_string_lossy(),
        &new_file.to_string_lossy(),
        &patch_file.to_string_lossy(),
        thread_count,
        use_compression,
        fast_format,
    )?;
    Ok(thread_count)
}

/// 自动模式创建单文件补丁：尝试内存模式，OOM 时自动回退流式。
pub fn run_hdiffz(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<u32> {
    let thread_count = get_diff_thread_count();

    crate::utils::ensure_parent_dir(patch_file)?;

    let mem_result = (|| -> Result<(), ffi::PatchError> {
        let old_data = std::fs::read(old_file).map_err(|e| ffi::PatchError {
            code: -1,
            message: format!("读取旧文件失败 {}: {e}", old_file.display()),
        })?;
        let new_data = std::fs::read(new_file).map_err(|e| ffi::PatchError {
            code: -1,
            message: format!("读取新文件失败 {}: {e}", new_file.display()),
        })?;
        let patch_data = ffi::create_patch(
            &old_data,
            &new_data,
            thread_count,
            use_compression,
            fast_format,
        )?;
        std::fs::write(patch_file, &patch_data).map_err(|e| ffi::PatchError {
            code: -1,
            message: format!("写入补丁文件失败 {}: {e}", patch_file.display()),
        })?;
        Ok(())
    })();

    match mem_result {
        Ok(()) => Ok(thread_count),
        Err(e) if e.is_oom() => {
            let old_size = std::fs::metadata(old_file).map(|m| m.len()).ok();
            let new_size = std::fs::metadata(new_file).map(|m| m.len()).ok();
            match (old_size, new_size) {
                (Some(os), Some(ns)) => {
                    let total_gb = (os + ns) as f64 / (1u64 << 30) as f64;
                    eprintln!(
                        "注意: 内存不足（{:.1}GB 文件），自动切换到流式模式",
                        total_gb
                    );
                }
                _ => {
                    eprintln!("注意: 内存不足，自动切换到流式模式");
                }
            }
            run_hdiffz_stream(
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

/// 多线程应用补丁，失败时自动回退单线程重试。
pub fn apply_patch_with_retry(
    old_data: &[u8],
    patch_data: &[u8],
    thread_count: u32,
) -> Result<Vec<u8>, ffi::PatchError> {
    match ffi::apply_patch(old_data, patch_data, thread_count) {
        Ok(data) => Ok(data),
        Err(e) if thread_count > 1 => {
            eprintln!("注意: 多线程应用补丁失败，回退单线程重试 ({e})");
            ffi::apply_patch(old_data, patch_data, 1)
        }
        Err(e) => Err(e),
    }
}

/// 应用单文件补丁：尝试内存模式，OOM 时自动回退流式。
pub fn run_hpatchz(old_file: &Path, patch_file: &Path, output_file: &Path) -> anyhow::Result<()> {
    let thread_count = get_recommended_thread_count();
    crate::utils::ensure_parent_dir(output_file)?;

    let patch_data = std::fs::read(patch_file)
        .map_err(|e| anyhow::anyhow!("读取补丁文件失败 {}: {e}", patch_file.display()))?;

    let try_apply = || -> Result<Vec<u8>, ffi::PatchError> {
        let old_data = std::fs::read(old_file).map_err(|e| ffi::PatchError {
            code: -1,
            message: format!("读取旧文件失败 {}: {e}", old_file.display()),
        })?;
        apply_patch_with_retry(&old_data, &patch_data, thread_count)
    };

    match try_apply() {
        Ok(new_data) => {
            std::fs::write(output_file, &new_data)
                .map_err(|e| anyhow::anyhow!("写入输出文件失败 {}: {e}", output_file.display()))?;
        }
        Err(e) if e.is_oom() => {
            eprintln!("注意: 内存不足，自动切换为流式模式");
            ffi::apply_patch_file(
                &old_file.to_string_lossy(),
                &patch_data,
                &output_file.to_string_lossy(),
                thread_count,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Err(e) => {
            return Err(anyhow::anyhow!("{e}"));
        }
    }

    Ok(())
}
