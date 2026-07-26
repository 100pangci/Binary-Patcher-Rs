use std::path::Path;
use crate::ffi;

const DEFAULT_THREADS: u32 = 4;
const MAX_PATCH_THREADS: u32 = 5; // HDiffPatch -p- supports 1..5

pub fn get_recommended_thread_count() -> u32 {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(DEFAULT_THREADS as usize);
    (cpu_count.saturating_sub(1)).max(1).min(MAX_PATCH_THREADS as usize) as u32
}

/// In-memory patch creation (best quality, suffix-string matching).
/// Caller must provide pre-read data to avoid duplicate I/O.
pub fn run_hdiffz_mem(
    old_data: &[u8],
    new_data: &[u8],
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<u32> {
    crate::utils::ensure_parent_dir(patch_file)?;

    let patch_data = ffi::create_patch(old_data, new_data, thread_count, use_compression, fast_format)
        .map_err(|e| anyhow::anyhow!("创建补丁失败: {e}"))?;
    std::fs::write(patch_file, &patch_data)
        .map_err(|e| anyhow::anyhow!("写入补丁文件失败 {}: {e}", patch_file.display()))?;
    Ok(thread_count)
}

/// File-streaming patch creation (lower memory, larger patch size).
pub fn run_hdiffz_stream(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    thread_count: u32,
    use_compression: bool,
    fast_format: bool,
) -> anyhow::Result<u32> {
    crate::utils::ensure_parent_dir(patch_file)?;

    ffi::create_patch_file(
        &old_file.to_string_lossy(),
        &new_file.to_string_lossy(),
        &patch_file.to_string_lossy(),
        thread_count,
        use_compression,
        fast_format,
    )
    .map_err(|e| anyhow::anyhow!("创建补丁失败: {e}"))?;
    Ok(thread_count)
}

/// Auto-select: in-memory if data fits, otherwise stream from disk.
pub fn run_hdiffz(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    use_compression: bool,
) -> anyhow::Result<u32> {
    let thread_count = get_recommended_thread_count();

    crate::utils::ensure_parent_dir(patch_file)?;

    // Try in-memory first (best patch quality, suffix-string matching)
    let mem_result = (|| -> anyhow::Result<()> {
        let old_data = std::fs::read(old_file)
            .map_err(|e| anyhow::anyhow!("读取旧文件失败 {}: {e}", old_file.display()))?;
        let new_data = std::fs::read(new_file)
            .map_err(|e| anyhow::anyhow!("读取新文件失败 {}: {e}", new_file.display()))?;
        let patch_data = ffi::create_patch(&old_data, &new_data, thread_count, use_compression, false)
            .map_err(|e| anyhow::anyhow!("创建补丁失败: {e}"))?;
        std::fs::write(patch_file, &patch_data)
            .map_err(|e| anyhow::anyhow!("写入补丁文件失败 {}: {e}", patch_file.display()))?;
        Ok(())
    })();

    match mem_result {
        Ok(()) => Ok(thread_count),
        Err(e) => {
            let msg = e.to_string();
            let old_size = std::fs::metadata(old_file).map(|m| m.len()).unwrap_or(0);
            let new_size = std::fs::metadata(new_file).map(|m| m.len()).unwrap_or(0);
            let total_gb = (old_size + new_size) as f64 / (1u64 << 30) as f64;
            if msg.contains("内存") || msg.contains("memory") || msg.contains("OOM") {
                eprintln!("注意: 内存不足（{:.1}GB 文件），自动切换到流式模式", total_gb);
                run_hdiffz_stream(old_file, new_file, patch_file, thread_count, use_compression, false)
            } else {
                Err(e)
            }
        }
    }
}

pub fn run_hpatchz(old_file: &Path, patch_file: &Path, output_file: &Path) -> anyhow::Result<()> {
    let thread_count = get_recommended_thread_count();

    let old_data = std::fs::read(old_file)
        .map_err(|e| anyhow::anyhow!("读取旧文件失败 {}: {e}", old_file.display()))?;
    let patch_data = std::fs::read(patch_file)
        .map_err(|e| anyhow::anyhow!("读取补丁文件失败 {}: {e}", patch_file.display()))?;

    // Try multi-threaded apply first (1..5 threads, same as hpatchz -p-)
    let new_data = ffi::apply_patch(&old_data, &patch_data, thread_count);

    let new_data = match new_data {
        Ok(data) => data,
        Err(e) if thread_count > 1 => {
            // Retry with single thread and warn user
            eprintln!("注意: 多线程应用补丁失败，回退单线程重试 ({e})");
            ffi::apply_patch(&old_data, &patch_data, 1)
                .map_err(|e2| anyhow::anyhow!("单线程重试也失败: {e2}"))?
        }
        Err(e) => return Err(anyhow::anyhow!("{e}")),
    };

    crate::utils::ensure_parent_dir(output_file)?;
    std::fs::write(output_file, &new_data)
        .map_err(|e| anyhow::anyhow!("写入输出文件失败 {}: {e}", output_file.display()))?;

    Ok(())
}
