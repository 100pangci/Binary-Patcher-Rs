mod compile;
mod download;

use std::path::PathBuf;

const HDIFFPATCH_REPO_API: &str = "https://api.github.com/repos/sisong/HDiffPatch/releases/latest";

pub fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_script/mod.rs");
    println!("cargo:rerun-if-changed=build_script/download.rs");
    println!("cargo:rerun-if-changed=build_script/compile.rs");
    println!("cargo:rerun-if-changed=vendor/hdiffpatch-sys/hdiffpatch_wrapper.cpp");
    println!("cargo:rerun-if-changed=vendor/hdiffpatch-sys/hdiffpatch_wrapper.h");

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo"),
    );
    let cache_dir = manifest_dir.join("target").join(".hdiffpatch-cache");
    let hd_path = cache_dir.join("src");
    let zip_path = cache_dir.join("hdiffpatch.zip");

    if !hd_path.exists() {
        download::download_and_extract(&zip_path, &hd_path);
    }

    let zlib_version = "1.3.1";
    let zlib_dir = download::download_zlib(zlib_version, &cache_dir);

    let mut zlib_build = cc::Build::new();
    zlib_build.define("NDEBUG", None);
    zlib_build.opt_level(3);
    zlib_build.include(&zlib_dir);
    // 仅编译 inflate 所需文件：旧版压缩补丁的 apply 侧需要解压支持，
    // create 侧不再压缩（zlib 对随机二进制差异无收益，且压缩序列化在内存不足时
    // 会静默生成损坏补丁）。
    for f in [
        "adler32", "crc32", "inflate", "inftrees", "inffast", "zutil",
    ] {
        zlib_build.file(zlib_dir.join(format!("{f}.c")));
    }
    zlib_build.define("NO_GZCOMPRESS", None);
    zlib_build.define("NO_GZIP", None);
    zlib_build.compile("zlib");

    compile::compile_all(&hd_path, &zlib_dir);
}
