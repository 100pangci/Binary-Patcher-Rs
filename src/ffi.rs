use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr::null_mut;

unsafe extern "C" {
    /// 创建内存补丁。
    /// # Safety
    /// - `old_data` / `new_data` 必须指向有效内存区域，长度分别等于 `old_size` / `new_size`。
    /// - `out_patch` / `out_patch_size` 必须是有效非空指针。
    /// - 成功时 `*out_patch` 被设置为需要调用 `hdiffpatch_free` 释放的堆内存。
    fn hdiffpatch_create(
        old_data: *const u8,
        old_size: usize,
        new_data: *const u8,
        new_size: usize,
        out_patch: *mut *mut u8,
        out_patch_size: *mut usize,
        thread_num: i32,
        use_compression: i32,
        fast_format: i32,
    ) -> i32;

    /// 使用文件路径创建补丁。
    /// # Safety
    /// - `old_file` / `new_file` / `patch_file` 必须是以 null 结尾的有效 C 字符串。
    fn hdiffpatch_create_file(
        old_file: *const c_char,
        new_file: *const c_char,
        patch_file: *const c_char,
        thread_num: i32,
        use_compression: i32,
        fast_format: i32,
    ) -> i32;

    /// 应用内存补丁。
    /// # Safety
    /// - `old_data` / `patch_data` 必须指向有效内存区域，长度分别等于 `old_size` / `patch_size`。
    /// - `out_new_data` / `out_new_size` 必须是有效非空指针。
    /// - 成功时 `*out_new_data` 被设置为需要调用 `hdiffpatch_free` 释放的堆内存。
    fn hdiffpatch_apply(
        old_data: *const u8,
        old_size: usize,
        patch_data: *const u8,
        patch_size: usize,
        out_new_data: *mut *mut u8,
        out_new_size: *mut usize,
        thread_num: i32,
    ) -> i32;

    /// 应用补丁到文件。
    /// # Safety
    /// - `old_file` / `output_file` 必须是以 null 结尾的有效 C 字符串。
    /// - `patch_data` 必须指向有效内存区域，长度等于 `patch_size`。
    fn hdiffpatch_apply_file(
        old_file: *const c_char,
        patch_data: *const u8,
        patch_size: usize,
        output_file: *const c_char,
        thread_num: i32,
    ) -> i32;

    /// 释放由 hdiffpatch 函数分配的堆内存。
    /// # Safety
    /// - `ptr` 必须是之前由 `hdiffpatch_create` 或 `hdiffpatch_apply` 分配的指针。
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
        write!(f, "{} (错误码: {})", self.message, self.code)
    }
}

impl std::error::Error for PatchError {}

fn error_msg(code: i32) -> String {
    match code {
        -8 => "内存不足，无法分配补丁缓冲区",
        -1 => "创建补丁失败、补丁格式不兼容或内部异常",
        -2 => "内存不足，无法分配输出缓冲区",
        -3 => "应用补丁执行失败",
        -4 => "应用补丁时发生内部异常",
        -5 => "无法打开旧文件",
        -6 => "无法打开新文件",
        -7 => "无法创建输出文件",
        _ => "未知错误",
    }
    .to_string()
}

pub fn create_patch(
    old_data: &[u8],
    new_data: &[u8],
    thread_num: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<Vec<u8>, PatchError> {
    let mut out_patch: *mut u8 = null_mut();
    let mut out_patch_size: usize = 0;

    let ret = unsafe {
        hdiffpatch_create(
            old_data.as_ptr(),
            old_data.len(),
            new_data.as_ptr(),
            new_data.len(),
            &mut out_patch,
            &mut out_patch_size,
            (thread_num.min(i32::MAX as u32)) as i32,
            use_compression as i32,
            fast_format as i32,
        )
    };

    if ret != 0 {
        if !out_patch.is_null() {
            unsafe { hdiffpatch_free(out_patch as *mut c_void); }
        }
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    if out_patch_size == 0 {
        if !out_patch.is_null() {
            unsafe { hdiffpatch_free(out_patch as *mut c_void); }
        }
        return Ok(Vec::new());
    }

    let patch = unsafe { std::slice::from_raw_parts(out_patch, out_patch_size).to_vec() };
    unsafe { hdiffpatch_free(out_patch as *mut c_void); }
    Ok(patch)
}

pub fn create_patch_file(
    old_file: &str,
    new_file: &str,
    patch_file: &str,
    thread_num: u32,
    use_compression: bool,
    fast_format: bool,
) -> Result<(), PatchError> {
    let old_c = std::ffi::CString::new(old_file).map_err(|e| PatchError {
        code: -1,
        message: format!("无效路径: {e}"),
    })?;
    let new_c = std::ffi::CString::new(new_file).map_err(|e| PatchError {
        code: -1,
        message: format!("无效路径: {e}"),
    })?;
    let patch_c = std::ffi::CString::new(patch_file).map_err(|e| PatchError {
        code: -1,
        message: format!("无效路径: {e}"),
    })?;

    let ret = unsafe {
        hdiffpatch_create_file(
            old_c.as_ptr(),
            new_c.as_ptr(),
            patch_c.as_ptr(),
            (thread_num.min(i32::MAX as u32)) as i32,
            use_compression as i32,
            fast_format as i32,
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

pub fn apply_patch(
    old_data: &[u8],
    patch_data: &[u8],
    thread_num: u32,
) -> Result<Vec<u8>, PatchError> {
    let mut out_new_data: *mut u8 = null_mut();
    let mut out_new_size: usize = 0;

    let ret = unsafe {
        hdiffpatch_apply(
            old_data.as_ptr(),
            old_data.len(),
            patch_data.as_ptr(),
            patch_data.len(),
            &mut out_new_data,
            &mut out_new_size,
            (thread_num.min(i32::MAX as u32)) as i32,
        )
    };

    if ret != 0 {
        if !out_new_data.is_null() {
            unsafe { hdiffpatch_free(out_new_data as *mut c_void); }
        }
        return Err(PatchError {
            code: ret,
            message: error_msg(ret),
        });
    }

    if out_new_size == 0 {
        if !out_new_data.is_null() {
            unsafe { hdiffpatch_free(out_new_data as *mut c_void); }
        }
        return Ok(Vec::new());
    }

    let new_data = unsafe { std::slice::from_raw_parts(out_new_data, out_new_size).to_vec() };
    unsafe { hdiffpatch_free(out_new_data as *mut c_void); }
    Ok(new_data)
}

pub fn apply_patch_file(
    old_file: &str,
    patch_data: &[u8],
    output_file: &str,
    thread_num: u32,
) -> Result<(), PatchError> {
    let old_c = std::ffi::CString::new(old_file).map_err(|e| PatchError {
        code: -1,
        message: format!("无效路径: {e}"),
    })?;
    let output_c = std::ffi::CString::new(output_file).map_err(|e| PatchError {
        code: -1,
        message: format!("无效路径: {e}"),
    })?;

    let ret = unsafe {
        hdiffpatch_apply_file(
            old_c.as_ptr(),
            patch_data.as_ptr(),
            patch_data.len(),
            output_c.as_ptr(),
            (thread_num.min(i32::MAX as u32)) as i32,
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
