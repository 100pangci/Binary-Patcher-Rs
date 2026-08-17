use crate::t;
use std::ffi::{CString, c_void};
use std::os::raw::c_char;
use std::path::Path;
use std::ptr::null_mut;
use std::sync::Mutex;

/// Serialize access to HDiffPatch C library which is not thread-safe.
static FFI_LOCK: Mutex<()> = Mutex::new(());

unsafe extern "C" {
    fn hdiffpatch_create(
        old_data: *const u8,
        old_size: usize,
        new_data: *const u8,
        new_size: usize,
        out_patch: *mut *mut u8,
        out_patch_size: *mut usize,
        thread_num: i32,
        fast_format: i32,
    ) -> i32;

    fn hdiffpatch_create_file(
        old_file: *const c_char,
        new_file: *const c_char,
        patch_file: *const c_char,
        thread_num: i32,
        fast_format: i32,
    ) -> i32;

    fn hdiffpatch_apply_new_size(
        patch_data: *const u8,
        patch_size: usize,
        out_new_size: *mut usize,
    ) -> i32;

    fn hdiffpatch_apply(
        old_data: *const u8,
        old_size: usize,
        patch_data: *const u8,
        patch_size: usize,
        out_new_data: *mut u8,
        out_new_size: usize,
        thread_num: i32,
    ) -> i32;

    fn hdiffpatch_apply_file(
        old_file: *const c_char,
        patch_data: *const u8,
        patch_size: usize,
        output_file: *const c_char,
        thread_num: i32,
    ) -> i32;

    fn hdiffpatch_free(ptr: *mut c_void);
}

#[derive(Debug, Clone)]
pub struct PatchError {
    pub code: i32,
    pub message: String,
}

impl PatchError {
    pub fn is_oom(&self) -> bool {
        self.code == -8
    }
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (code: {})", self.message, self.code)
    }
}

impl std::error::Error for PatchError {}

fn error_msg(code: i32) -> String {
    match code {
        -8 => t!("ffi.oom"),
        -1 => t!("ffi.create-failed"),
        -2 => t!("ffi.alloc-failed"),
        -3 => t!("ffi.apply-failed"),
        -4 => t!("ffi.apply-exception"),
        -5 => t!("ffi.cant-open-old"),
        -6 => t!("ffi.cant-open-new"),
        -7 => t!("ffi.cant-create-output"),
        _ => t!("ffi.unknown-error"),
    }
}

/// 将路径转换为 C 字符串传给 C 库。
/// Unix 上按原始字节转换，保留非 UTF-8 文件名；
/// Windows 上转换为 UTF-8，C 侧以 _IS_USED_WIN32_UTF8_WAPI 宽字符 API
/// （UTF-8→UTF-16 + _wfsopen）打开文件，绕开 ANSI 代码页（GBK）限制。
/// 仅 Windows 文件名的未配对代理项会替换为 U+FFFD（现实中几乎不存在）。
fn path_to_cstring(path: &Path) -> Result<CString, PatchError> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        CString::new(path.as_os_str().as_bytes()).map_err(|e| PatchError {
            code: -1,
            message: t!("ffi.invalid-path", e),
        })
    }
    #[cfg(not(unix))]
    {
        CString::new(path.to_string_lossy().as_bytes()).map_err(|e| PatchError {
            code: -1,
            message: t!("ffi.invalid-path", e),
        })
    }
}

pub fn create_patch(
    old_data: &[u8],
    new_data: &[u8],
    thread_num: u32,
    fast_format: bool,
) -> Result<Vec<u8>, PatchError> {
    let _lock = FFI_LOCK.lock().map_err(|_| PatchError {
        code: -1,
        message: "FFI mutex poisoned".to_string(),
    })?;

    let mut out_patch: *mut u8 = null_mut();
    let mut out_patch_size: usize = 0;

    let thread_num_i32 = i32::try_from(thread_num).unwrap_or(i32::MAX);
    let ret = unsafe {
        hdiffpatch_create(
            old_data.as_ptr(),
            old_data.len(),
            new_data.as_ptr(),
            new_data.len(),
            &mut out_patch,
            &mut out_patch_size,
            thread_num_i32,
            i32::from(fast_format),
        )
    };

    if ret != 0 {
        if !out_patch.is_null() {
            unsafe {
                hdiffpatch_free(out_patch as *mut c_void);
            }
        }
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    if out_patch.is_null() && out_patch_size > 0 {
        return Err(PatchError {
            code: -1,
            message: t!("ffi.null-ptr"),
        });
    }

    if out_patch_size == 0 {
        if !out_patch.is_null() {
            unsafe {
                hdiffpatch_free(out_patch as *mut c_void);
            }
        }
        return Ok(Vec::new());
    }

    let patch = unsafe { std::slice::from_raw_parts(out_patch, out_patch_size).to_vec() };
    unsafe {
        hdiffpatch_free(out_patch as *mut c_void);
    }
    Ok(patch)
}

pub fn create_patch_file(
    old_file: &Path,
    new_file: &Path,
    patch_file: &Path,
    thread_num: u32,
    fast_format: bool,
) -> Result<(), PatchError> {
    let _lock = FFI_LOCK.lock().map_err(|_| PatchError {
        code: -1,
        message: "FFI mutex poisoned".to_string(),
    })?;

    let old_c = path_to_cstring(old_file)?;
    let new_c = path_to_cstring(new_file)?;
    let patch_c = path_to_cstring(patch_file)?;

    let thread_num_i32 = i32::try_from(thread_num).unwrap_or(i32::MAX);
    let ret = unsafe {
        hdiffpatch_create_file(
            old_c.as_ptr(),
            new_c.as_ptr(),
            patch_c.as_ptr(),
            thread_num_i32,
            i32::from(fast_format),
        )
    };

    if ret != 0 {
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    Ok(())
}

/// 解析补丁头获取输出数据大小（不分配输出缓冲）。
pub fn patch_new_size(patch_data: &[u8]) -> Result<usize, PatchError> {
    let mut new_size: usize = 0;
    let ret =
        unsafe { hdiffpatch_apply_new_size(patch_data.as_ptr(), patch_data.len(), &mut new_size) };
    if ret != 0 {
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }
    Ok(new_size)
}

pub fn apply_patch(
    old_data: &[u8],
    patch_data: &[u8],
    thread_num: u32,
) -> Result<Vec<u8>, PatchError> {
    let _lock = FFI_LOCK.lock().map_err(|_| PatchError {
        code: -1,
        message: "FFI mutex poisoned".to_string(),
    })?;

    // 先解析补丁头部拿到输出大小，由 Rust 侧分配输出缓冲，
    // C 库直接填充，避免 C 分配 + Rust 复制造成双缓冲峰值内存。
    let new_size = patch_new_size(patch_data)?;

    if new_size == 0 {
        return Ok(Vec::new());
    }

    let mut new_data = vec![0u8; new_size];
    let thread_num_i32 = i32::try_from(thread_num).unwrap_or(i32::MAX);
    let ret = unsafe {
        hdiffpatch_apply(
            old_data.as_ptr(),
            old_data.len(),
            patch_data.as_ptr(),
            patch_data.len(),
            new_data.as_mut_ptr(),
            new_data.len(),
            thread_num_i32,
        )
    };
    if ret != 0 {
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    Ok(new_data)
}

pub fn apply_patch_file(
    old_file: &Path,
    patch_data: &[u8],
    output_file: &Path,
    thread_num: u32,
) -> Result<(), PatchError> {
    let _lock = FFI_LOCK.lock().map_err(|_| PatchError {
        code: -1,
        message: "FFI mutex poisoned".to_string(),
    })?;

    let old_c = path_to_cstring(old_file)?;
    let output_c = path_to_cstring(output_file)?;

    let thread_num_i32 = i32::try_from(thread_num).unwrap_or(i32::MAX);
    let ret = unsafe {
        hdiffpatch_apply_file(
            old_c.as_ptr(),
            patch_data.as_ptr(),
            patch_data.len(),
            output_c.as_ptr(),
            thread_num_i32,
        )
    };

    if ret != 0 {
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    Ok(())
}
