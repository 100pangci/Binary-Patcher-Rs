use std::ffi::c_void;
use std::os::raw::c_char;
use std::ptr::null_mut;

unsafe extern "C" {
    fn hdiffpatch_create(
        old_data: *const u8,
        old_size: usize,
        new_data: *const u8,
        new_size: usize,
        out_patch: *mut *mut u8,
        out_patch_size: *mut usize,
        thread_num: i32,
        use_compression: i32,
    ) -> i32;

    fn hdiffpatch_create_file(
        old_file: *const c_char,
        new_file: *const c_char,
        patch_file: *const c_char,
        thread_num: i32,
        use_compression: i32,
    ) -> i32;

    fn hdiffpatch_apply(
        old_data: *const u8,
        old_size: usize,
        patch_data: *const u8,
        patch_size: usize,
        out_new_data: *mut *mut u8,
        out_new_size: *mut usize,
        thread_num: i32,
    ) -> i32;

    fn hdiffpatch_free(ptr: *mut c_void);
}

pub fn create_patch(
    old_data: &[u8],
    new_data: &[u8],
    thread_num: u32,
    use_compression: bool,
) -> Result<Vec<u8>, String> {
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
            thread_num as i32,
            use_compression as i32,
        )
    };

    if ret != 0 {
        if !out_patch.is_null() {
            unsafe { hdiffpatch_free(out_patch as *mut c_void); }
        }
        return Err("创建补丁失败".to_string());
    }

    let patch = unsafe {
        std::slice::from_raw_parts(out_patch, out_patch_size).to_vec()
    };

    unsafe { hdiffpatch_free(out_patch as *mut c_void); }

    Ok(patch)
}

pub fn create_patch_file(
    old_file: &str,
    new_file: &str,
    patch_file: &str,
    thread_num: u32,
    use_compression: bool,
) -> Result<(), String> {
    let old_c = std::ffi::CString::new(old_file).map_err(|e| format!("无效路径: {e}"))?;
    let new_c = std::ffi::CString::new(new_file).map_err(|e| format!("无效路径: {e}"))?;
    let patch_c = std::ffi::CString::new(patch_file).map_err(|e| format!("无效路径: {e}"))?;

    let ret = unsafe {
        hdiffpatch_create_file(
            old_c.as_ptr(),
            new_c.as_ptr(),
            patch_c.as_ptr(),
            thread_num as i32,
            use_compression as i32,
        )
    };

    if ret != 0 {
        let msg = match ret {
            -1 => "创建补丁失败或内部异常",
            -5 => "无法打开旧文件",
            -6 => "无法打开新文件",
            -7 => "无法创建补丁文件",
            _ => "创建补丁失败",
        };
        return Err(format!("{msg} (错误码: {ret})"));
    }

    Ok(())
}

pub fn apply_patch(
    old_data: &[u8],
    patch_data: &[u8],
    thread_num: u32,
) -> Result<Vec<u8>, String> {
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
            thread_num as i32,
        )
    };

    if ret != 0 {
        if !out_new_data.is_null() {
            unsafe { hdiffpatch_free(out_new_data as *mut c_void); }
        }
        let msg = match ret {
            -1 => "无法解析补丁文件头部信息（补丁格式不兼容或文件损坏）",
            -2 => "内存不足，无法分配输出缓冲区",
            -3 => "应用补丁执行失败",
            -4 => "应用补丁时发生内部异常",
            _ => "应用补丁失败",
        };
        return Err(format!("{msg} (错误码: {ret})"));
    }

    let new_data = unsafe {
        std::slice::from_raw_parts(out_new_data, out_new_size).to_vec()
    };

    unsafe { hdiffpatch_free(out_new_data as *mut c_void); }

    Ok(new_data)
}
