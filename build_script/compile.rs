use std::path::{Path, PathBuf};

pub fn compile_all(hd_path: &Path, zlib_dir: &Path) {
    let src_dir = hd_path.join("libHDiffPatch");
    let parallel_dir = hd_path.join("libParallel");
    let includes = includes_for(hd_path, &src_dir, &parallel_dir, zlib_dir);
    compile_c(&src_dir, &parallel_dir, hd_path, &includes);
    compile_cpp(&src_dir, &parallel_dir, hd_path, &includes);
}

fn includes_for(
    hd_path: &Path,
    src_dir: &Path,
    parallel_dir: &Path,
    zlib_dir: &Path,
) -> Vec<PathBuf> {
    vec![
        hd_path.to_path_buf(),
        src_dir.to_path_buf(),
        src_dir.join("HDiff"),
        src_dir.join("HPatch"),
        src_dir.join("HPatch").join("hpatch_mt"),
        src_dir.join("HPatchLite"),
        parallel_dir.to_path_buf(),
        hd_path.join("dirDiffPatch"),
        hd_path.join("bsdiff_wrapper"),
        hd_path.join("vcdiff_wrapper"),
        zlib_dir.to_path_buf(),
    ]
}

fn new_build(includes: &[PathBuf], cpp: bool) -> cc::Build {
    let mut build = cc::Build::new();
    build
        .define("NDEBUG", None)
        .define("_IS_RUN_MEM_SAFE_CHECK", "0")
        .define("_IS_OUT_DIFF_INFO", "0")
        .opt_level(3)
        .include("vendor/hdiffpatch-sys")
        .cpp(cpp);
    for inc in includes {
        build.include(inc);
    }
    if cpp {
        build.flag_if_supported("-std=c++11");
        build.flag_if_supported("/std:c++11");
        build.define("_CompressPlugin_zlib", None);
    }
    if !cfg!(windows) {
        build.flag("-pthread");
    }
    build
}

fn compile_c(src_dir: &Path, parallel_dir: &Path, hd_path: &Path, includes: &[PathBuf]) {
    let mut c_build = new_build(includes, false);
    c_build.file(src_dir.join("HPatch").join("patch.c"));
    c_build.file(src_dir.join("HPatchLite").join("hpatch_lite.c"));
    c_build.file(hd_path.join("file_for_patch.c"));
    c_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("limit_mem_diff")
            .join("adler_roll.c"),
    );
    c_build.file(
        src_dir
            .join("HPatch")
            .join("hpatch_mt")
            .join("_hpatch_mt.c"),
    );
    c_build.file(
        src_dir
            .join("HPatch")
            .join("hpatch_mt")
            .join("_houtput_mt.c"),
    );
    c_build.file(
        src_dir
            .join("HPatch")
            .join("hpatch_mt")
            .join("_hinput_mt.c"),
    );
    c_build.file(
        src_dir
            .join("HPatch")
            .join("hpatch_mt")
            .join("_hcache_window_old_mt.c"),
    );
    c_build.file(
        src_dir
            .join("HPatch")
            .join("hpatch_mt")
            .join("_hcache_old_mt.c"),
    );
    c_build.file(src_dir.join("HPatch").join("hpatch_mt").join("hpatch_mt.c"));
    c_build.file(parallel_dir.join("parallel_import_c.c"));
    c_build.compile("hdiffpatch_c");
}

fn compile_cpp(src_dir: &Path, parallel_dir: &Path, hd_path: &Path, includes: &[PathBuf]) {
    let mut cpp_build = new_build(includes, true);
    cpp_build.file(src_dir.join("HDiff").join("diff.cpp"));
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("suffix_string.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("bytes_rle.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("compress_detect.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("match_block.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("match_inplace.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("limit_mem_diff")
            .join("digest_matcher.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("limit_mem_diff")
            .join("stream_serialize.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("window_diff")
            .join("window_matcher.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("window_diff")
            .join("covers_range.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("libdivsufsort")
            .join("divsufsort.cpp"),
    );
    cpp_build.file(
        src_dir
            .join("HDiff")
            .join("private_diff")
            .join("libdivsufsort")
            .join("divsufsort64.cpp"),
    );
    cpp_build.file(parallel_dir.join("parallel_channel.cpp"));
    cpp_build.file(hd_path.join("compress_parallel.cpp"));
    cpp_build.file(
        Path::new("vendor")
            .join("hdiffpatch-sys")
            .join("hdiffpatch_wrapper.cpp"),
    );
    cpp_build.compile("hdiffpatch_cpp");
}
