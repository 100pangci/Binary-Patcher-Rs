use std::path::Path;

pub fn compile_all(hd_path: &Path, zlib_dir: &Path) {
    let src_dir = hd_path.join("libHDiffPatch");
    let parallel_dir = hd_path.join("libParallel");

    let includes: &[&Path] = &[
        hd_path,
        &src_dir,
        &src_dir.join("HDiff"),
        &src_dir.join("HPatch"),
        &src_dir.join("HPatch").join("hpatch_mt"),
        &src_dir.join("HPatchLite"),
        &parallel_dir,
        &hd_path.join("dirDiffPatch"),
        &hd_path.join("bsdiff_wrapper"),
        &hd_path.join("vcdiff_wrapper"),
        zlib_dir,
    ];

    let mut c_build = cc::Build::new();
    c_build.define("NDEBUG", None);
    c_build.define("_IS_RUN_MEM_SAFE_CHECK", "0");
    c_build.define("_IS_OUT_DIFF_INFO", "0");
    c_build.opt_level(3);
    for inc in includes {
        c_build.include(inc);
    }
    c_build.include("vendor/hdiffpatch-sys");
    if !cfg!(windows) {
        c_build.flag("-pthread");
    }

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

    let mut cpp_build = cc::Build::new();
    cpp_build.define("NDEBUG", None);
    cpp_build.define("_IS_RUN_MEM_SAFE_CHECK", "0");
    cpp_build.define("_IS_OUT_DIFF_INFO", "0");
    cpp_build.opt_level(3);
    for inc in includes {
        cpp_build.include(inc);
    }
    cpp_build.include("vendor/hdiffpatch-sys");
    cpp_build.cpp(true);
    cpp_build.flag_if_supported("-std=c++11");
    cpp_build.flag_if_supported("/std:c++11");
    if !cfg!(windows) {
        cpp_build.flag("-pthread");
    }
    cpp_build.define("_CompressPlugin_zlib", None);

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
