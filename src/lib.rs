//! `binary_patcher` 是一个用于创建和应用二进制补丁的工具库。
//! 底层使用 HDiffPatch C 库通过 FFI 静态链接。

pub mod apply;
pub mod bundle;
pub mod cli;
pub mod ffi;
pub mod hdiffpatch;
pub mod manifest;
pub mod rollback;
pub mod utils;
