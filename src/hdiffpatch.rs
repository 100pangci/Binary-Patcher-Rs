use crate::ffi;
use crate::t;
use std::path::Path;

const DEFAULT_THREADS: u32 = 4;
const MAX_PATCH_THREADS: u32 = 5;
const MAX_DIFF_THREADS: u32 = 32;

fn clamp_thread_count(max: u32) -> u32 {
    let cpu_count = std::thread::available_parallelism()
        .map_or(DEFAULT_THREADS as usize, std::num::NonZeroUsize::get);
    (cpu_count.saturating_sub(1)).max(1).min(max as usize) as u32
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
        &old_file.to_string_lossy(),
        &new_file.to_string_lossy(),
        &patch_file.to_string_lossy(),
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

    let mem_result = (|| -> Result<(), ffi::PatchError> {
        let old_data = std::fs::read(old_file).map_err(|e| ffi::PatchError {
            code: -1,
            message: t!("ffi.read-old-failed", old_file.display(), e),
        })?;
        let new_data = std::fs::read(new_file).map_err(|e| ffi::PatchError {
            code: -1,
            message: t!("ffi.read-new-failed", new_file.display(), e),
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
            message: t!("ffi.write-failed", patch_file.display(), e),
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
                    eprintln!("{}", t!("hdiff.oom-fallback", format!("{:.1}", total_gb)));
                }
                _ => {
                    eprintln!("{}", t!("hdiff.oom-fallback-generic"));
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

pub fn apply_patch_auto(
    old_data: &[u8],
    old_file: &Path,
    patch_data: &[u8],
    output_file: &Path,
    thread_count: u32,
) -> Result<Vec<u8>, anyhow::Error> {
    crate::path::ensure_parent_dir(output_file)?;
    let result = apply_patch_with_retry(old_data, patch_data, thread_count);
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
            ffi::apply_patch_file(
                &old_file.to_string_lossy(),
                patch_data,
                &output_file.to_string_lossy(),
                thread_count,
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;
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
    apply_patch_auto(&old_data, old_file, &patch_data, output_file, thread_count)?;
    Ok(())
}
